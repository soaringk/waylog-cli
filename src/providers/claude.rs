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
    let mut started_at = Utc::now();
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
            if let Some(message) = parse_message(event)? {
                if messages.is_empty() {
                    started_at = message.timestamp;
                }
                messages.push(message);
            }
        }
    }

    Ok(ChatSession {
        session_id,
        provider: provider_name.to_string(),
        project_path,
        started_at,
        updated_at: messages
            .last()
            .map(|message| message.timestamp)
            .unwrap_or(started_at),
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

fn parse_message(event: ClaudeEvent) -> Result<Option<ChatMessage>> {
    let role = match event.event_type.as_str() {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        _ => return Ok(None),
    };

    let content = match &event.message {
        Some(msg) => match &msg.content {
            ClaudeContent::Text(text) => text.clone(),
            ClaudeContent::Array(items) => items
                .iter()
                .filter_map(|item| {
                    if item.content_type == "text" {
                        item.text.clone()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        },
        None => return Ok(None),
    };

    if content.is_empty() {
        return Ok(None);
    }

    let content = if role == MessageRole::User {
        let re = regex::Regex::new(r"(?s)<ide_[a-z_]+>.*?</ide_[a-z_]+>")
            .map_err(|error| WaylogError::Internal(error.to_string()))?;
        let clean_content = re.replace_all(&content, "").to_string();

        if clean_content.trim().is_empty() {
            return Ok(None);
        }

        format_claude_xml(clean_content.trim())
    } else {
        content
    };

    let timestamp = parse_timestamp(event.timestamp.as_ref()).unwrap_or_else(Utc::now);

    let (model, tokens, tool_calls) = if let Some(msg) = &event.message {
        let model = msg.model.clone();
        let tokens = msg.usage.as_ref().map(|usage| TokenUsage {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cached: usage.cache_read_input_tokens.unwrap_or(0),
        });

        let tool_calls = if let ClaudeContent::Array(items) = &msg.content {
            items
                .iter()
                .filter(|item| item.content_type == "tool_use")
                .filter_map(|item| item.name.clone())
                .collect()
        } else {
            Vec::new()
        };

        (model, tokens, tool_calls)
    } else {
        (None, None, Vec::new())
    };

    Ok(Some(ChatMessage {
        id: event
            .uuid
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        timestamp,
        role,
        content,
        metadata: MessageMetadata {
            model,
            tokens,
            tool_calls,
            thoughts: Vec::new(),
        },
    }))
}

fn format_claude_xml(content: &str) -> String {
    if let Some(start) = content.find("<command-name>") {
        if let Some(end) = content[start..].find("</command-name>") {
            let cmd = &content[start + 14..start + end];

            if cmd.trim().starts_with('/') {
                return format!("> {}", cmd.trim());
            }
        }
    }

    if let Some(start) = content.find("<local-command-stdout>") {
        if let Some(end) = content[start..].find("</local-command-stdout>") {
            let out = &content[start + 22..start + end];
            return format!("> ⎿ {}", out.trim());
        }
    }

    content.to_string()
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

        if line.contains("\"isSidechain\":true") {
            return Ok(false);
        }
        if line.contains("\"isSidechain\":false") {
            return Ok(true);
        }

        if let Ok(event) = serde_json::from_str::<ClaudeEvent>(&line) {
            if event.is_sidechain == Some(true) {
                return Ok(false);
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
    Array(Vec<ClaudeContentItem>),
}

#[derive(Debug, Deserialize)]
struct ClaudeContentItem {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
    name: Option<String>,
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
    fn test_ide_tag_filtering() {
        let content = "<ide_opened_file>some/path/file.txt</ide_opened_file>";
        let event = create_user_event(content);
        let result = parse_message(event).unwrap();

        assert!(
            result.is_none(),
            "Pure IDE tag message should be filtered out"
        );

        let content = "Check this file.\n<ide_opened_file>path/to/file</ide_opened_file>";
        let event = create_user_event(content);
        let result = parse_message(event).unwrap();

        assert!(result.is_some());
        let msg = result.unwrap();
        assert_eq!(
            msg.content, "Check this file.",
            "Tag should be stripped from mixed content"
        );
    }
}
