use crate::error::{Result, WaylogError};
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        Self
    }

    fn data_dir(&self) -> Result<PathBuf> {
        path::get_ai_data_dir("claude").map(|path| path.join("projects"))
    }

    fn session_dir(&self, project_path: &Path) -> Result<PathBuf> {
        Ok(self
            .data_dir()?
            .join(path::encode_path_claude(project_path)))
    }
}

pub(super) async fn main_session_files(directories: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();

    for directory in directories {
        if !directory.exists() {
            continue;
        }

        let mut entries = fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let session_path = entry.path();
            if session_path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && is_main_session(&session_path).await.unwrap_or(false)
            {
                let modified = fs::metadata(&session_path).await?.modified()?;
                candidates.push((session_path, modified));
            }
        }
    }

    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
    Ok(candidates
        .into_iter()
        .map(|candidate| candidate.0)
        .collect())
}

pub(super) async fn parse_jsonl_session(
    file_path: &Path,
    provider_name: &str,
) -> Result<ChatSession> {
    let file = fs::File::open(file_path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut messages = Vec::new();
    let mut session_id = String::new();
    let mut project_path = PathBuf::new();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let event: ClaudeEvent = serde_json::from_str(&line).map_err(WaylogError::Json)?;

        if session_id.is_empty() {
            session_id = event.session_id.clone().unwrap_or_else(|| {
                file_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            });
        }
        if project_path.as_os_str().is_empty() {
            if let Some(cwd) = &event.cwd {
                project_path = PathBuf::from(cwd);
            }
        }

        if matches!(event.event_type.as_str(), "user" | "assistant") {
            let parsed = parse_messages(event)?;
            messages.extend(parsed);
        }
    }

    let (started_at, updated_at) = message_time_range(&messages);
    Ok(ChatSession {
        session_id,
        provider: provider_name.to_string(),
        project_path,
        started_at,
        updated_at,
        messages,
    })
}

#[async_trait]
impl Provider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        let session_path = self
            .session_dir(project_path)?
            .join(format!("{session_id}.jsonl"));
        Ok(session_path.exists().then_some(session_path))
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        main_session_files(&[self.session_dir(project_path)?]).await
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        parse_jsonl_session(file_path, self.name()).await
    }

    fn has_history(&self) -> bool {
        self.data_dir().is_ok_and(|directory| directory.exists())
    }

    fn run_command(&self) -> Option<&str> {
        Some("claude")
    }
}

fn parse_messages(event: ClaudeEvent) -> Result<Vec<ChatMessage>> {
    let role = match event.event_type.as_str() {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        _ => return Ok(Vec::new()),
    };
    let Some(message) = event.message else {
        return Ok(Vec::new());
    };
    let timestamp = parse_timestamp(event.timestamp.as_ref());
    let message_id = event
        .uuid
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let model = message.model;
    let tokens = message.usage.as_ref().map(|usage| TokenUsage {
        input: usage.input_tokens,
        output: usage.output_tokens,
        cached: usage.cache_read_input_tokens.unwrap_or(0),
    });
    let items = match message.content {
        ClaudeContent::Text(text) => vec![serde_json::json!({"type": "text", "text": text})],
        ClaudeContent::Array(items) => items,
    };
    let tool_calls = items
        .iter()
        .filter(|item| is_tool_payload(item))
        .filter_map(|item| {
            item.get("name")
                .or_else(|| item.get("tool"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut messages = Vec::new();
    let mut text_index = 0;
    let mut tool_index = 0;

    for item in items {
        match item.get("type").and_then(serde_json::Value::as_str) {
            Some("text") => {
                let Some(text) = item.get("text").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                messages.push(ChatMessage {
                    id: format!("{message_id}:text:{text_index}"),
                    timestamp,
                    role,
                    content: text.to_string(),
                    metadata: MessageMetadata {
                        model: model.clone(),
                        tokens: if text_index == 0 {
                            tokens.clone()
                        } else {
                            None
                        },
                        tool_calls: if text_index == 0 {
                            tool_calls.clone()
                        } else {
                            Vec::new()
                        },
                        ..Default::default()
                    },
                });
                text_index += 1;
            }
            _ if is_tool_payload(&item) => {
                messages.push(ChatMessage::tool(
                    format!("{message_id}:tool:{tool_index}"),
                    timestamp,
                    item,
                ));
                tool_index += 1;
            }
            _ => {}
        }
    }

    Ok(messages)
}

async fn is_main_session(path: &Path) -> Result<bool> {
    let file = fs::File::open(path).await?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut checked_lines = 0;
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        if checked_lines >= 10 {
            break;
        }
        checked_lines += 1;

        if let Ok(event) = serde_json::from_str::<ClaudeEvent>(&line) {
            if let Some(is_sidechain) = event.is_sidechain {
                return Ok(!is_sidechain);
            }
        }
    }

    Ok(true)
}

fn parse_timestamp(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    match value? {
        serde_json::Value::String(value) => DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|timestamp| timestamp.with_timezone(&Utc)),
        serde_json::Value::Number(value) => DateTime::from_timestamp_millis(value.as_i64()?),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeEvent {
    #[serde(rename = "type")]
    event_type: String,

    #[serde(rename = "sessionId")]
    session_id: Option<String>,

    cwd: Option<String>,
    timestamp: Option<serde_json::Value>,
    uuid: Option<String>,

    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,

    message: Option<ClaudeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    content: ClaudeContent,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ClaudeContent {
    Text(String),
    Array(Vec<serde_json::Value>),
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    input_tokens: u32,
    output_tokens: u32,
    cache_read_input_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_user_event(content: &str) -> ClaudeEvent {
        ClaudeEvent {
            event_type: "user".to_string(),
            session_id: Some("test-session".to_string()),
            cwd: None,
            timestamp: None,
            uuid: None,
            is_sidechain: None,
            message: Some(ClaudeMessage {
                content: ClaudeContent::Text(content.to_string()),
                model: None,
                usage: None,
            }),
        }
    }

    #[test]
    fn preserves_user_text_without_rewriting_provider_markup() {
        let content = "<ide_opened_file>some/path/file.txt</ide_opened_file>";
        let event = create_user_event(content);
        let result = parse_messages(event).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, content);
    }

    #[test]
    fn parses_tool_use_and_result_as_tool_messages() {
        let event = ClaudeEvent {
            event_type: "assistant".to_string(),
            session_id: Some("test-session".to_string()),
            cwd: None,
            timestamp: Some(serde_json::json!("2026-07-22T00:00:00Z")),
            uuid: Some("assistant-1".to_string()),
            is_sidechain: None,
            message: Some(ClaudeMessage {
                content: ClaudeContent::Array(vec![
                    serde_json::json!({"type": "text", "text": "Checking the file"}),
                    serde_json::json!({"type": "tool_use", "id": "tool-1", "name": "Read", "input": {"file_path": "src/main.rs"}}),
                    serde_json::json!({"type": "tool_result", "tool_use_id": "tool-1", "content": "file contents"}),
                ]),
                model: Some("test-model".to_string()),
                usage: None,
            }),
        };

        let messages = parse_messages(event).unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, MessageRole::Assistant);
        assert_eq!(messages[1].role, MessageRole::Tool);
        assert_eq!(messages[1].metadata.tool_call_id.as_deref(), Some("tool-1"));
        assert!(messages[1].content.contains("src/main.rs"));
        assert_eq!(messages[2].role, MessageRole::Tool);
        assert_eq!(messages[2].metadata.tool_call_id.as_deref(), Some("tool-1"));
        assert!(messages[2].content.contains("file contents"));
    }
}
