use crate::error::{Result, WaylogError};
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct GeminiProvider;

impl GeminiProvider {
    pub fn new() -> Self {
        Self
    }

    fn data_dir(&self) -> Result<PathBuf> {
        path::get_ai_data_dir("gemini").map(|path| path.join("tmp"))
    }

    fn session_dir(&self, project_path: &Path) -> Result<PathBuf> {
        let data_dir = self.data_dir()?;
        if let Ok(entries) = std::fs::read_dir(&data_dir) {
            for entry in entries.flatten() {
                let candidate = entry.path();
                let project_root = candidate.join(".project_root");
                if std::fs::read_to_string(project_root)
                    .is_ok_and(|value| Path::new(value.trim()) == project_path)
                {
                    return Ok(candidate.join("chats"));
                }
            }
        }

        Ok(data_dir
            .join(path::encode_path_gemini(project_path))
            .join("chats"))
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let session_dir = self.session_dir(project_path)?;

        if !session_dir.exists() {
            return Ok(Vec::new());
        }

        // Find all .json files
        let mut entries = fs::read_dir(&session_dir).await?;
        let mut candidates = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("json" | "jsonl")
            ) {
                let metadata = fs::metadata(&path).await?;
                let modified = metadata.modified()?;
                candidates.push((path, modified));
            }
        }

        // Sort by modification time, newest first
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

        Ok(candidates.into_iter().map(|(p, _)| p).collect())
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        if file_path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            return self.parse_jsonl_session(file_path).await;
        }

        let content = fs::read_to_string(file_path).await?;
        let session_data: GeminiSession =
            serde_json::from_str(&content).map_err(WaylogError::Json)?;

        let messages = session_data
            .messages
            .into_iter()
            .filter_map(|msg| self.parse_message(msg).ok().flatten())
            .collect::<Vec<_>>();

        let started_at = DateTime::parse_from_rfc3339(&session_data.start_time)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let updated_at = DateTime::parse_from_rfc3339(&session_data.last_updated)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(started_at);

        // Decode project path from hash
        let project_path = file_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();

        Ok(ChatSession {
            session_id: session_data.session_id,
            provider: self.name().to_string(),
            project_path,
            started_at,
            updated_at,
            messages,
        })
    }

    fn has_history(&self) -> bool {
        // Gemini CLI might not be in PATH, check for data directory instead
        self.data_dir().map(|d| d.exists()).unwrap_or(false)
    }

    fn run_command(&self) -> Option<&str> {
        Some("gemini")
    }
}

impl GeminiProvider {
    async fn parse_jsonl_session(&self, file_path: &Path) -> Result<ChatSession> {
        let file = fs::File::open(file_path).await?;
        let mut lines = BufReader::new(file).lines();
        let header = loop {
            let line = lines.next_line().await?.ok_or_else(|| {
                WaylogError::PathError(format!("Empty Gemini session: {}", file_path.display()))
            })?;
            if !line.trim().is_empty() {
                break serde_json::from_str::<GeminiSessionHeader>(&line)?;
            }
        };

        let mut last_updated = header.last_updated.clone();
        let mut messages = Vec::new();
        let mut seen = HashSet::new();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)?;

            if matches!(
                value.get("type").and_then(Value::as_str),
                Some("user" | "gemini")
            ) {
                let message: GeminiMessage = serde_json::from_value(value.clone())?;
                if seen.insert(message.id.clone()) {
                    if let Some(message) = self.parse_message(message)? {
                        messages.push(message);
                    }
                }
            }

            if let Some(set) = value.get("$set") {
                if let Some(updated) = set.get("lastUpdated").and_then(Value::as_str) {
                    last_updated = updated.to_string();
                }
                if let Some(patched_messages) = set.get("messages").and_then(Value::as_array) {
                    for value in patched_messages {
                        let message: GeminiMessage = serde_json::from_value(value.clone())?;
                        if seen.insert(message.id.clone()) {
                            if let Some(message) = self.parse_message(message)? {
                                messages.push(message);
                            }
                        }
                    }
                }
            }
        }

        messages.sort_by_key(|message| message.timestamp);
        let started_at = DateTime::parse_from_rfc3339(&header.start_time)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let updated_at = DateTime::parse_from_rfc3339(&last_updated)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or(started_at);
        let project_path = file_path
            .parent()
            .and_then(Path::parent)
            .and_then(|directory| std::fs::read_to_string(directory.join(".project_root")).ok())
            .map(|value| PathBuf::from(value.trim()))
            .unwrap_or_default();

        Ok(ChatSession {
            session_id: header.session_id,
            provider: self.name().to_string(),
            project_path,
            started_at,
            updated_at,
            messages,
        })
    }

    fn parse_message(&self, msg: GeminiMessage) -> Result<Option<ChatMessage>> {
        let role = match msg.message_type.as_str() {
            "user" => MessageRole::User,
            "gemini" => MessageRole::Assistant,
            _ => return Ok(None),
        };

        let content = msg.content.into_text();
        if content.trim().is_empty() {
            return Ok(None);
        }

        let timestamp = DateTime::parse_from_rfc3339(&msg.timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        // Extract thoughts (Gemini-specific feature)
        let thoughts = msg
            .thoughts
            .unwrap_or_default()
            .into_iter()
            .map(|t| format!("{}: {}", t.subject, t.description))
            .collect();

        // Extract token usage
        let tokens = msg.tokens.map(|t| TokenUsage {
            input: t.input,
            output: t.output,
            cached: t.cached,
        });

        Ok(Some(ChatMessage {
            id: msg.id,
            timestamp,
            role,
            content,
            metadata: MessageMetadata {
                model: msg.model,
                tokens,
                tool_calls: Vec::new(),
                thoughts,
            },
        }))
    }
}

// Gemini JSON session structures
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSession {
    session_id: String,
    #[allow(dead_code)]
    project_hash: String,
    start_time: String,
    last_updated: String,
    messages: Vec<GeminiMessage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiMessage {
    id: String,
    timestamp: String,

    #[serde(rename = "type")]
    message_type: String,

    content: GeminiContent,
    model: Option<String>,
    thoughts: Option<Vec<GeminiThought>>,
    tokens: Option<GeminiTokens>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GeminiContent {
    Text(String),
    Parts(Vec<GeminiContentPart>),
}

impl GeminiContent {
    fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Parts(parts) => parts
                .into_iter()
                .filter_map(|part| part.text)
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GeminiContentPart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSessionHeader {
    session_id: String,
    start_time: String,
    last_updated: String,
}

#[derive(Debug, Deserialize)]
struct GeminiThought {
    subject: String,
    description: String,
    #[allow(dead_code)]
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct GeminiTokens {
    input: u32,
    output: u32,
    cached: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_current_jsonl_history() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("project");
        let project_dir = temp_dir.path().join("project-name");
        let chats = project_dir.join("chats");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::write(
            project_dir.join(".project_root"),
            project_path.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let session_path = chats.join("session-test.jsonl");
        std::fs::write(
            &session_path,
            concat!(
                r#"{"sessionId":"session-test","projectHash":"project-name","startTime":"2026-01-01T00:00:00Z","lastUpdated":"2026-01-01T00:00:01Z","kind":"main"}"#,
                "\n",
                r#"{"id":"user-1","timestamp":"2026-01-01T00:00:00Z","type":"user","content":[{"text":"Hello"}]}"#,
                "\n",
                r#"{"id":"assistant-1","timestamp":"2026-01-01T00:00:01Z","type":"gemini","content":"Hi","model":"gemini-test","tokens":{"input":1,"output":2,"cached":0}}"#,
                "\n"
            ),
        )
        .unwrap();

        let provider = GeminiProvider;
        let session = provider.parse_session(&session_path).await.unwrap();

        assert_eq!(session.session_id, "session-test");
        assert_eq!(session.project_path, project_path);
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].content, "Hello");
        assert_eq!(session.messages[1].content, "Hi");
    }
}
