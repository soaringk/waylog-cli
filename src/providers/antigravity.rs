use crate::error::Result;
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct AntigravityProvider {
    data_dir: Option<PathBuf>,
}

impl AntigravityProvider {
    pub fn new() -> Self {
        Self { data_dir: None }
    }

    #[cfg(test)]
    fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
        }
    }
}

#[async_trait]
impl Provider for AntigravityProvider {
    fn name(&self) -> &str {
        "antigravity"
    }

    fn data_dir(&self) -> Result<PathBuf> {
        Ok(self
            .data_dir
            .clone()
            .unwrap_or(path::home_dir()?.join(".gemini").join("antigravity-cli")))
    }

    fn session_dir(&self, _project_path: &Path) -> Result<PathBuf> {
        Ok(self.data_dir()?.join("brain"))
    }

    async fn find_latest_session(&self, project_path: &Path) -> Result<Option<PathBuf>> {
        let candidates = self.get_all_sessions(project_path).await?;
        Ok(candidates.into_iter().next())
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let data_dir = self.data_dir()?;
        if !data_dir.exists() {
            return Ok(Vec::new());
        }

        let conversation_ids = self.conversation_ids_for_project(project_path).await?;
        let mut candidates = Vec::new();

        for conversation_id in conversation_ids {
            let Some(transcript_path) = self
                .transcript_path_for_conversation(&conversation_id)
                .await
            else {
                continue;
            };

            if let Ok(metadata) = fs::metadata(&transcript_path).await {
                if let Ok(modified) = metadata.modified() {
                    candidates.push((transcript_path, modified));
                }
            }
        }

        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));
        Ok(candidates.into_iter().map(|(p, _)| p).collect())
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        let file = fs::File::open(file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let session_id = conversation_id_from_transcript_path(file_path).unwrap_or_else(|| {
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });
        let project_path = self
            .project_path_for_conversation(&session_id)
            .await?
            .unwrap_or_default();

        let mut messages = Vec::new();
        let mut started_at = Utc::now();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let Ok(event) = serde_json::from_str::<AntigravityEvent>(&line) else {
                continue;
            };

            let Some(message) = event.into_chat_message() else {
                continue;
            };

            if messages.is_empty() {
                started_at = message.timestamp;
            }

            let is_duplicate = messages.last().is_some_and(|last: &ChatMessage| {
                last.role == message.role && last.content == message.content
            });
            if !is_duplicate {
                messages.push(message);
            }
        }

        Ok(ChatSession {
            session_id,
            provider: self.name().to_string(),
            project_path,
            started_at,
            updated_at: messages.last().map(|m| m.timestamp).unwrap_or(started_at),
            messages,
        })
    }

    fn is_installed(&self) -> bool {
        self.data_dir().map(|d| d.exists()).unwrap_or(false)
    }

    fn command(&self) -> &str {
        "antigravity-cli"
    }
}

impl AntigravityProvider {
    async fn conversation_ids_for_project(&self, project_path: &Path) -> Result<HashSet<String>> {
        let mut ids = HashSet::new();
        let data_dir = self.data_dir()?;

        let history_path = data_dir.join("history.jsonl");
        if history_path.exists() {
            let file = fs::File::open(&history_path).await?;
            let reader = BufReader::new(file);
            let mut lines = reader.lines();

            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }

                let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) else {
                    continue;
                };

                if paths_equal(Path::new(&entry.workspace), project_path) {
                    if let Some(id) = entry.conversation_id {
                        if !id.is_empty() {
                            ids.insert(id);
                        }
                    }
                }
            }
        }

        let last_conversations_path = data_dir.join("cache").join("last_conversations.json");
        if last_conversations_path.exists() {
            let content = fs::read_to_string(&last_conversations_path).await?;
            let last_conversations: HashMap<String, String> =
                serde_json::from_str(&content).unwrap_or_default();

            for (workspace, id) in last_conversations {
                if paths_equal(Path::new(&workspace), project_path) && !id.is_empty() {
                    ids.insert(id);
                }
            }
        }

        Ok(ids)
    }

    async fn project_path_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<PathBuf>> {
        let data_dir = self.data_dir()?;
        let history_path = data_dir.join("history.jsonl");
        if !history_path.exists() {
            return Ok(None);
        }

        let file = fs::File::open(&history_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let Ok(entry) = serde_json::from_str::<HistoryEntry>(&line) else {
                continue;
            };

            if entry.conversation_id.as_deref() == Some(conversation_id) {
                return Ok(Some(PathBuf::from(entry.workspace)));
            }
        }

        Ok(None)
    }

    async fn transcript_path_for_conversation(&self, conversation_id: &str) -> Option<PathBuf> {
        let log_dir = self
            .data_dir()
            .ok()?
            .join("brain")
            .join(conversation_id)
            .join(".system_generated")
            .join("logs");

        let full = log_dir.join("transcript_full.jsonl");
        if full.exists() {
            return Some(full);
        }

        let transcript = log_dir.join("transcript.jsonl");
        transcript.exists().then_some(transcript)
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches('/')
            .trim_end_matches('\\')
            .to_string()
    };

    normalize(left) == normalize(right)
}

fn conversation_id_from_transcript_path(path: &Path) -> Option<String> {
    path.parent()?
        .parent()?
        .parent()?
        .file_name()?
        .to_str()
        .map(ToOwned::to_owned)
}

#[derive(Debug, Deserialize)]
struct HistoryEntry {
    workspace: String,
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AntigravityEvent {
    #[serde(rename = "type")]
    event_type: String,
    source: String,
    status: Option<String>,
    created_at: String,
    content: Option<String>,
    thinking: Option<String>,
    tool_calls: Option<Vec<AntigravityToolCall>>,
    step_index: Option<u64>,
}

impl AntigravityEvent {
    fn into_chat_message(self) -> Option<ChatMessage> {
        if self
            .status
            .as_deref()
            .is_some_and(|status| status != "DONE")
        {
            return None;
        }

        let role = match (self.event_type.as_str(), self.source.as_str()) {
            ("USER_INPUT", "USER_EXPLICIT") => MessageRole::User,
            ("PLANNER_RESPONSE", "MODEL") => MessageRole::Assistant,
            _ => return None,
        };

        let content = self.content.unwrap_or_default();
        if content.trim().is_empty() {
            return None;
        }

        let timestamp = DateTime::parse_from_rfc3339(&self.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        let tool_calls = self
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| call.name)
            .collect();

        Some(ChatMessage {
            id: self
                .step_index
                .map(|idx| idx.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            timestamp,
            role,
            content,
            metadata: MessageMetadata {
                model: None,
                tokens: None,
                tool_calls,
                thoughts: self.thinking.into_iter().collect(),
            },
        })
    }
}

#[derive(Debug, Deserialize)]
struct AntigravityToolCall {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn conversation_id_is_read_from_brain_path() {
        let path = Path::new(
            "/home/cody/.gemini/antigravity-cli/brain/session-1/.system_generated/logs/transcript_full.jsonl",
        );

        assert_eq!(
            conversation_id_from_transcript_path(path),
            Some("session-1".to_string())
        );
    }

    #[test]
    fn parses_user_and_model_events_only() {
        let user = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"USER_INPUT","source":"USER_EXPLICIT","status":"DONE","created_at":"2026-05-23T01:02:03Z","content":"hello","step_index":1}"#,
        )
        .unwrap()
        .into_chat_message()
        .unwrap();
        assert_eq!(user.role, MessageRole::User);
        assert_eq!(user.content, "hello");

        let model = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"PLANNER_RESPONSE","source":"MODEL","status":"DONE","created_at":"2026-05-23T01:02:04Z","content":"hi","thinking":"plan","tool_calls":[{"name":"view_file"}],"step_index":2}"#,
        )
        .unwrap()
        .into_chat_message()
        .unwrap();
        assert_eq!(model.role, MessageRole::Assistant);
        assert_eq!(model.metadata.thoughts, vec!["plan".to_string()]);
        assert_eq!(model.metadata.tool_calls, vec!["view_file".to_string()]);

        let system = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"EPHEMERAL_MESSAGE","source":"SYSTEM","status":"DONE","created_at":"2026-05-23T01:02:05Z","content":"ignored"}"#,
        )
        .unwrap()
        .into_chat_message();
        assert!(system.is_none());
    }

    #[tokio::test]
    async fn finds_transcripts_for_project_from_history_and_cache() {
        let temp_dir = TempDir::new().unwrap();
        let data_dir = temp_dir.path();
        let project_path = Path::new("/home/cody/project");
        let provider = AntigravityProvider::with_data_dir(data_dir.to_path_buf());

        tokio::fs::create_dir_all(data_dir.join("cache"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(
            data_dir
                .join("brain")
                .join("from-history")
                .join(".system_generated")
                .join("logs"),
        )
        .await
        .unwrap();
        tokio::fs::create_dir_all(
            data_dir
                .join("brain")
                .join("from-cache")
                .join(".system_generated")
                .join("logs"),
        )
        .await
        .unwrap();

        tokio::fs::write(
            data_dir.join("history.jsonl"),
            r#"{"workspace":"/home/cody/project","conversationId":"from-history","timestamp":1,"display":"x"}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            data_dir.join("cache").join("last_conversations.json"),
            r#"{"/home/cody/project":"from-cache"}"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            data_dir
                .join("brain")
                .join("from-history")
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl"),
            "",
        )
        .await
        .unwrap();
        tokio::fs::write(
            data_dir
                .join("brain")
                .join("from-cache")
                .join(".system_generated")
                .join("logs")
                .join("transcript_full.jsonl"),
            "",
        )
        .await
        .unwrap();

        let sessions = provider.get_all_sessions(project_path).await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .any(|path| path.ends_with("transcript.jsonl")));
        assert!(sessions
            .iter()
            .any(|path| path.ends_with("transcript_full.jsonl")));
    }
}
