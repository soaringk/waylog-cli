use crate::error::Result;
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct CodexProvider {
    data_dir: Option<PathBuf>,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self { data_dir: None }
    }

    #[cfg(test)]
    fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
        }
    }

    fn data_dir(&self) -> Result<PathBuf> {
        if let Some(data_dir) = &self.data_dir {
            return Ok(data_dir.clone());
        }

        Ok(path::home_dir()?.join(".codex").join("sessions"))
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        let base_session_dir = self.data_dir()?;
        if !base_session_dir.exists() {
            return Ok(None);
        }

        for entry in walkdir::WalkDir::new(base_session_dir) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                && (stem == session_id || stem.ends_with(session_id))
                && self.probe_project_path(path, project_path).await?
            {
                return Ok(Some(path.to_path_buf()));
            }
        }

        Ok(None)
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let base_session_dir = self.data_dir()?;

        if !base_session_dir.exists() {
            return Ok(Vec::new());
        }

        // Recursively find all .jsonl files in the base session directory
        let mut candidates = Vec::new();
        let walker = walkdir::WalkDir::new(&base_session_dir);

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                // Probe the file for project path match
                if self
                    .probe_project_path(path, project_path)
                    .await
                    .unwrap_or(false)
                {
                    if let Ok(metadata) = fs::metadata(path).await {
                        if let Ok(modified) = metadata.modified() {
                            candidates.push((path.to_path_buf(), modified));
                        }
                    }
                }
            }
        }

        // Sort by modification time, newest first
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

        Ok(candidates.into_iter().map(|(p, _)| p).collect())
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        let file = fs::File::open(file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut messages = Vec::new();
        let mut session_id = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut session_project_path = PathBuf::new();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let event: CodexEvent = serde_json::from_str(&line)?;
            match event.event_type.as_str() {
                "session_meta" => {
                    if let Some(payload) = event.payload {
                        if let Some(id) = payload
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|id| !id.is_empty())
                        {
                            session_id = id.to_string();
                        }
                        if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                            session_project_path = PathBuf::from(cwd);
                        }
                    }
                }
                "turn_context" => {
                    if let Some(cwd) = event
                        .payload
                        .as_ref()
                        .and_then(|payload| payload.get("cwd"))
                        .and_then(Value::as_str)
                    {
                        session_project_path = PathBuf::from(cwd);
                    }
                }
                "response_item" => {
                    if let Some(payload) = event.payload {
                        for msg in self.parse_response_item(payload, event.timestamp.as_deref())? {
                            messages.push(msg);
                        }
                    }
                }
                _ => {}
            }
        }

        let (started_at, updated_at) = message_time_range(&messages);
        Ok(ChatSession {
            session_id,
            provider: self.name().to_string(),
            project_path: session_project_path,
            started_at,
            updated_at,
            messages,
        })
    }

    fn has_history(&self) -> bool {
        self.data_dir().is_ok_and(|directory| directory.exists())
    }

    fn run_command(&self) -> Option<&str> {
        Some("codex")
    }
}

impl CodexProvider {
    async fn probe_project_path(
        &self,
        file_path: &Path,
        target_project_path: &Path,
    ) -> Result<bool> {
        let file = fs::File::open(file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // Scan first 50 lines (session_meta is usually first)
        let mut checked_lines = 0;
        while let Some(line) = lines.next_line().await? {
            if checked_lines >= 50 {
                break;
            }
            checked_lines += 1;

            if let Ok(event) = serde_json::from_str::<CodexEvent>(&line) {
                if let Some(cwd_str) = event
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.get("cwd"))
                    .and_then(Value::as_str)
                {
                    return Ok(paths_equivalent(Path::new(cwd_str), target_project_path).await);
                }
            }
        }
        Ok(false)
    }

    fn parse_response_item(
        &self,
        payload: Value,
        timestamp: Option<&str>,
    ) -> Result<Vec<ChatMessage>> {
        let timestamp = timestamp
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));
        if is_tool_payload(&payload) {
            let message_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            return Ok(vec![ChatMessage::tool(message_id, timestamp, payload)]);
        }

        let role = match payload.get("role").and_then(Value::as_str) {
            Some("user") => MessageRole::User,
            Some("assistant") => MessageRole::Assistant,
            _ => return Ok(Vec::new()),
        };

        let message_id = payload
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Ok(payload
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, item)| {
                item.get("text")
                    .or_else(|| item.get("refusal"))
                    .and_then(Value::as_str)
                    .map(|content| ChatMessage {
                        id: format!("{message_id}:text:{index}"),
                        timestamp,
                        role,
                        content: content.to_string(),
                        metadata: MessageMetadata::default(),
                    })
            })
            .collect())
    }
}

// Codex JSONL event structures
#[derive(Debug, Deserialize)]
struct CodexEvent {
    #[serde(rename = "type")]
    event_type: String,
    timestamp: Option<String>,
    payload: Option<Value>,
}

async fn paths_equivalent(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    match (fs::canonicalize(left).await, fs::canonicalize(right).await) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn session_meta_line(session_id: &str, cwd: &Path) -> String {
        serde_json::json!({
            "type": "session_meta",
            "timestamp": "2026-04-01T00:00:00Z",
            "payload": { "id": session_id, "cwd": cwd.to_string_lossy() }
        })
        .to_string()
    }

    #[tokio::test]
    async fn finds_latest_session_outside_recent_date_directories() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("project");
        let session_path = temp_dir
            .path()
            .join("2020")
            .join("01")
            .join("01")
            .join("session.jsonl");
        tokio::fs::create_dir_all(session_path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::create_dir(&project_path).await.unwrap();
        tokio::fs::write(
            &session_path,
            session_meta_line("long-running-session", &project_path),
        )
        .await
        .unwrap();

        let found = CodexProvider::with_data_dir(temp_dir.path().to_path_buf())
            .find_latest_session(&project_path)
            .await
            .unwrap();

        assert_eq!(found, Some(session_path));
    }

    #[tokio::test]
    async fn parses_canonical_session_id_and_requires_same_project() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("rollout-with-prefix.jsonl");
        let project_path = temp_dir.path().join("project");
        tokio::fs::create_dir(&project_path).await.unwrap();
        tokio::fs::write(
            &file_path,
            session_meta_line("canonical-session-id", &project_path),
        )
        .await
        .unwrap();

        let provider = CodexProvider::new();

        assert!(provider
            .probe_project_path(&file_path, &project_path)
            .await
            .unwrap());
        assert!(!provider
            .probe_project_path(&file_path, &project_path.join("nested"))
            .await
            .unwrap());
        assert_eq!(
            provider.parse_session(&file_path).await.unwrap().session_id,
            "canonical-session-id"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_project_path_accepts_symlink_equivalent_cwd() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let real_project = temp_dir.path().join("real-project");
        let linked_project = temp_dir.path().join("linked-project");
        let file_path = temp_dir.path().join("session.jsonl");
        tokio::fs::create_dir(&real_project).await.unwrap();
        symlink(&real_project, &linked_project).unwrap();
        tokio::fs::write(&file_path, session_meta_line("session-id", &linked_project))
            .await
            .unwrap();

        assert!(CodexProvider::new()
            .probe_project_path(&file_path, &real_project)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn detects_and_normalizes_tool_payloads_without_a_type_allowlist() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("session.jsonl");
        let payloads = [
            serde_json::json!({
                "type": "function_call",
                "name": "read_file",
                "arguments": "{\"path\":\"src/main.rs\"}",
                "call_id": "call-1",
                "provider_extension": {
                    "content": [{"type": "input_file", "file_id": "file-1"}]
                }
            }),
            serde_json::json!({
                "type": "function_call_output",
                "call_id": "call-1",
                "output": [{"type": "input_text", "text": "file contents", "extra": true}]
            }),
            serde_json::json!({
                "type": "tool_search_call",
                "call_id": "call-2",
                "arguments": {"query": "read"}
            }),
            serde_json::json!({
                "type": "tool_search_output",
                "call_id": "call-2",
                "tools": [{"name": "read_file", "defer_loading": true}]
            }),
            serde_json::json!({
                "type": "future_tool_action",
                "call_id": "call-3",
                "provider_payload": {"kept": true}
            }),
        ];
        let expected = [
            serde_json::json!({
                "name": "read_file",
                "arguments": {"path": "src/main.rs"},
                "provider_extension": {
                    "content": [{"type": "input_file", "file_id": "file-1"}]
                }
            }),
            serde_json::json!({
                "output": [{"type": "input_text", "text": "file contents", "extra": true}]
            }),
            serde_json::json!({"arguments": {"query": "read"}}),
            serde_json::json!({
                "tools": [{"name": "read_file", "defer_loading": true}]
            }),
            serde_json::json!({"provider_payload": {"kept": true}}),
        ];
        let lines = payloads.iter().enumerate().map(|(index, payload)| {
            serde_json::json!({
                "type": "response_item",
                "timestamp": format!("2026-07-22T00:00:0{index}Z"),
                "payload": payload
            })
        });
        tokio::fs::write(
            &file_path,
            lines
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .await
        .unwrap();

        let session = CodexProvider::new()
            .parse_session(&file_path)
            .await
            .unwrap();

        assert_eq!(session.messages.len(), payloads.len());
        assert!(session
            .messages
            .iter()
            .all(|message| message.role == MessageRole::Tool));
        for (message, expected) in session.messages.iter().zip(expected) {
            assert_eq!(
                serde_json::from_str::<Value>(&message.content).unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn preserves_injected_user_text_and_each_content_item() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("session.jsonl");
        let event = serde_json::json!({
            "type": "response_item",
            "timestamp": "invalid",
            "payload": {
                "type": "message",
                "id": "message-1",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "<environment_context>keep me</environment_context>"},
                    {"type": "input_text", "text": "second block"}
                ]
            }
        });
        tokio::fs::write(&file_path, event.to_string())
            .await
            .unwrap();

        let session = CodexProvider::new()
            .parse_session(&file_path)
            .await
            .unwrap();

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.started_at, None);
        assert_eq!(session.updated_at, None);
        assert!(session
            .messages
            .iter()
            .all(|message| message.timestamp.is_none()));
        assert_eq!(
            session.messages[0].content,
            "<environment_context>keep me</environment_context>"
        );
        assert_eq!(session.messages[1].content, "second block");
    }
}
