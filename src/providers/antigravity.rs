use super::jsonl::JsonlReader;
use crate::error::{Result, WaylogError};
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct AntigravityProvider {
    data_dir: Option<PathBuf>,
    name: &'static str,
    directory: &'static str,
}

impl AntigravityProvider {
    pub fn new() -> Self {
        Self::for_surface("antigravity", "antigravity-cli")
    }

    pub fn for_surface(name: &'static str, directory: &'static str) -> Self {
        Self {
            data_dir: None,
            name,
            directory,
        }
    }

    #[cfg(test)]
    fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
            name: "antigravity",
            directory: "antigravity-cli",
        }
    }

    fn data_dir(&self) -> Result<PathBuf> {
        Ok(self
            .data_dir
            .clone()
            .unwrap_or(path::home_dir()?.join(".gemini").join(self.directory)))
    }
}

#[async_trait]
impl Provider for AntigravityProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn is_project_scoped(&self) -> bool {
        false
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let data_dir = self.data_dir()?;
        if !data_dir.exists() {
            return Ok(Vec::new());
        }

        let _ = project_path;
        let mut conversation_ids = HashSet::new();
        let brain = data_dir.join("brain");
        if brain.is_dir() {
            let mut entries = fs::read_dir(brain).await?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_dir() {
                    conversation_ids.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
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
        let (session_id, data_dir) = transcript_location(file_path).ok_or_else(|| {
            WaylogError::PathError(format!(
                "Antigravity transcript must retain brain/<session-id>/.system_generated/logs/transcript[_full].jsonl: {}",
                file_path.display()
            ))
        })?;
        let project_path = Self::project_path_for_conversation(data_dir, session_id)
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(%error, session_id, "Could not read optional project metadata");
                None
            })
            .unwrap_or_default();
        let mut records = JsonlReader::open(file_path).await?;

        let mut messages = Vec::new();
        while let Some(event) = records.next::<AntigravityEvent>().await? {
            for message in event.into_chat_messages() {
                messages.push(message);
            }
        }

        let (started_at, updated_at) = message_time_range(&messages);
        Ok(ChatSession {
            session_id: session_id.to_string(),
            provider: self.name().to_string(),
            project_path,
            started_at,
            updated_at,
            messages,
        })
    }

    async fn select_source_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        Ok(paths
            .iter()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name == "transcript_full.jsonl")
                    || (path
                        .file_name()
                        .is_some_and(|name| name == "transcript.jsonl")
                        && !paths.contains(&path.with_file_name("transcript_full.jsonl")))
            })
            .cloned()
            .collect())
    }

    fn has_history(&self) -> bool {
        self.data_dir().map(|d| d.exists()).unwrap_or(false)
    }
}

impl AntigravityProvider {
    async fn project_path_for_conversation(
        data_dir: &Path,
        conversation_id: &str,
    ) -> Result<Option<PathBuf>> {
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

                if entry.conversation_id.as_deref() == Some(conversation_id) {
                    return Ok(Some(PathBuf::from(entry.workspace)));
                }
            }
        }
        let cache_path = data_dir.join("cache/last_conversations.json");
        if cache_path.is_file() {
            let mapping: HashMap<String, String> =
                serde_json::from_slice(&fs::read(cache_path).await?)?;
            if let Some((workspace, _)) = mapping.into_iter().find(|(_, id)| id == conversation_id)
            {
                return Ok(Some(PathBuf::from(workspace)));
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

fn transcript_location(path: &Path) -> Option<(&str, &Path)> {
    if !matches!(
        path.file_name()?.to_str()?,
        "transcript.jsonl" | "transcript_full.jsonl"
    ) {
        return None;
    }
    let logs = path.parent()?;
    let generated = logs.parent()?;
    let conversation = generated.parent()?;
    let brain = conversation.parent()?;
    if logs.file_name()? != "logs"
        || generated.file_name()? != ".system_generated"
        || brain.file_name()? != "brain"
    {
        return None;
    }
    Some((conversation.file_name()?.to_str()?, brain.parent()?))
}

fn user_request_text(content: String) -> String {
    let mut remaining = content.as_str();
    let mut requests = Vec::new();
    while let Some((_, rest)) = remaining.split_once("<USER_REQUEST>") {
        let Some((request, tail)) = rest.split_once("</USER_REQUEST>") else {
            break;
        };
        requests.push(request);
        remaining = tail;
    }
    if requests.is_empty() {
        content
    } else {
        requests.join("\n\n")
    }
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
    created_at: Option<String>,
    content: Option<String>,
    thinking: Option<String>,
    tool_calls: Option<Vec<Value>>,
}

impl AntigravityEvent {
    fn into_chat_messages(self) -> Vec<ChatMessage> {
        let role = match (self.event_type.as_str(), self.source.as_str()) {
            ("USER_INPUT", "USER_EXPLICIT") => MessageRole::User,
            ("PLANNER_RESPONSE", "MODEL") => MessageRole::Assistant(AssistantOutput::Message),
            _ => return Vec::new(),
        };

        let content = self.content.unwrap_or_default();
        let content = if role == MessageRole::User {
            user_request_text(content)
        } else {
            content
        };
        let timestamp = self
            .created_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));

        let tool_calls = self.tool_calls.unwrap_or_default();
        let tool_names = tool_calls
            .iter()
            .filter_map(|call| call.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        let mut messages = Vec::new();
        if !content.trim().is_empty() {
            messages.push(ChatMessage {
                timestamp,
                role,
                content,
                metadata: MessageMetadata {
                    tool_calls: tool_names,
                    thoughts: self.thinking.clone().into_iter().collect(),
                    ..Default::default()
                },
            });
        }
        if messages.is_empty() {
            if let Some(thinking) = self.thinking.filter(|text| !text.is_empty()) {
                messages.push(ChatMessage::reasoning(timestamp, thinking));
            }
        }
        messages.extend(
            tool_calls
                .into_iter()
                .map(|call| ChatMessage::tool(timestamp, call)),
        );
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn all_surfaces_discover_unmapped_brain_histories_and_choose_full_transcript() {
        for (name, surface) in [
            ("antigravity", "antigravity-cli"),
            ("antigravity-2", "antigravity"),
            ("antigravity-ide", "antigravity-ide"),
        ] {
            let dir = TempDir::new().unwrap();
            let logs = dir.path().join("brain/session/.system_generated/logs");
            fs::create_dir_all(&logs).await.unwrap();
            fs::write(logs.join("transcript.jsonl"), "").await.unwrap();
            let full = logs.join("transcript_full.jsonl");
            fs::write(&full, "{\"type\":\"USER_INPUT\",\"source\":\"USER_EXPLICIT\",\"content\":\"request  \"}\n").await.unwrap();
            let provider = AntigravityProvider {
                data_dir: Some(dir.path().to_path_buf()),
                name,
                directory: surface,
            };
            assert!(!provider.is_project_scoped());
            assert_eq!(
                provider
                    .get_all_sessions(Path::new("/unmapped-project"))
                    .await
                    .unwrap(),
                vec![full.clone()]
            );
            let session = provider.parse_session(&full).await.unwrap();
            assert_eq!(session.messages[0].content, "request  ");
            assert!(session.project_path.as_os_str().is_empty());
            assert_eq!(
                provider
                    .select_source_paths(vec![logs.join("transcript.jsonl"), full.clone()])
                    .await
                    .unwrap(),
                vec![full]
            );
        }
    }

    #[test]
    fn conversation_id_is_read_from_brain_path() {
        let path = Path::new(
            "/home/cody/.gemini/antigravity-cli/brain/session-1/.system_generated/logs/transcript_full.jsonl",
        );

        assert_eq!(
            transcript_location(path),
            Some(("session-1", Path::new("/home/cody/.gemini/antigravity-cli")))
        );
    }

    #[tokio::test]
    async fn relocated_native_trees_preserve_identity_and_use_only_adjacent_metadata() {
        let temp = TempDir::new().unwrap();
        let local = temp.path().join("local");
        fs::create_dir_all(&local).await.unwrap();
        fs::write(
            local.join("history.jsonl"),
            r#"{"workspace":"/wrong-local-project","conversationId":"one"}"#,
        )
        .await
        .unwrap();
        let provider = AntigravityProvider::with_data_dir(local);
        for root in ["original", "relocated"] {
            let surface = temp.path().join(root);
            fs::create_dir_all(surface.join("cache")).await.unwrap();
            fs::write(
                surface.join("history.jsonl"),
                r#"{"workspace":"/recorded-project","conversationId":"one"}"#,
            )
            .await
            .unwrap();
            fs::write(
                surface.join("cache/last_conversations.json"),
                r#"{"/cached-project":"two"}"#,
            )
            .await
            .unwrap();
            for (id, project) in [
                ("one", "/recorded-project"),
                ("two", "/cached-project"),
                ("unmapped", ""),
            ] {
                let logs = surface
                    .join("brain")
                    .join(id)
                    .join(".system_generated/logs");
                fs::create_dir_all(&logs).await.unwrap();
                let transcript = logs.join("transcript_full.jsonl");
                fs::write(
                    &transcript,
                    r#"{"type":"USER_INPUT","source":"USER_EXPLICIT","content":"request"}"#,
                )
                .await
                .unwrap();
                let session = provider.parse_session(&transcript).await.unwrap();
                assert_eq!(session.session_id, id);
                assert_eq!(session.project_path, Path::new(project));
            }
            fs::remove_file(surface.join("history.jsonl"))
                .await
                .unwrap();
            fs::remove_file(surface.join("cache/last_conversations.json"))
                .await
                .unwrap();
            let session = provider
                .parse_session(
                    &surface.join("brain/one/.system_generated/logs/transcript_full.jsonl"),
                )
                .await
                .unwrap();
            assert!(session.project_path.as_os_str().is_empty());
        }
    }

    #[tokio::test]
    async fn detached_transcripts_without_native_identity_are_rejected() {
        let temp = TempDir::new().unwrap();
        let provider = AntigravityProvider::with_data_dir(temp.path().join("unused"));
        for relative in [
            "upload/one/transcript_full.jsonl",
            "upload/two/transcript_full.jsonl",
            "other/session/.system_generated/logs/transcript_full.jsonl",
            "brain/session/.system_generated/logs/renamed.jsonl",
        ] {
            let transcript = temp.path().join(relative);
            fs::create_dir_all(transcript.parent().unwrap())
                .await
                .unwrap();
            fs::write(
                &transcript,
                r#"{"type":"USER_INPUT","source":"USER_EXPLICIT","content":"request"}"#,
            )
            .await
            .unwrap();
            let error = provider.parse_session(&transcript).await.unwrap_err();
            assert!(error.to_string().contains("must retain brain/<session-id>"));
        }
    }

    #[test]
    fn extracts_native_user_requests_without_trimming_or_requiring_done_status() {
        let messages = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"USER_INPUT","source":"USER_EXPLICIT","status":"RUNNING","content":"context<USER_REQUEST> first  </USER_REQUEST>context<USER_REQUEST>second\n</USER_REQUEST>context"}"#,
        ).unwrap().into_chat_messages();
        assert_eq!(messages[0].content, " first  \n\nsecond\n");
        assert_eq!(user_request_text("plain  ".to_string()), "plain  ");
    }

    #[test]
    fn parses_user_and_model_events_only() {
        let user = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"USER_INPUT","source":"USER_EXPLICIT","status":"DONE","created_at":"2026-05-23T01:02:03Z","content":"hello","step_index":1}"#,
        )
        .unwrap()
        .into_chat_messages();
        let user = &user[0];
        assert_eq!(user.role, MessageRole::User);
        assert_eq!(user.content, "hello");

        let model = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"PLANNER_RESPONSE","source":"MODEL","status":"DONE","created_at":"2026-05-23T01:02:04Z","content":"hi","thinking":"plan","tool_calls":[{"name":"view_file","arguments":{"path":"src/main.rs"},"provider_extension":{"nested":true}}],"step_index":2}"#,
        )
        .unwrap()
        .into_chat_messages();
        let model_message = &model[0];
        assert_eq!(
            model_message.role,
            MessageRole::Assistant(AssistantOutput::Message)
        );
        assert_eq!(model_message.metadata.thoughts, vec!["plan".to_string()]);
        assert_eq!(model[1].role, MessageRole::Assistant(AssistantOutput::Tool));
        assert_eq!(
            serde_json::from_str::<Value>(&model[1].content).unwrap(),
            serde_json::json!({
                "name": "view_file",
                "arguments": {"path": "src/main.rs"},
                "provider_extension": {"nested": true}
            })
        );

        let system = serde_json::from_str::<AntigravityEvent>(
            r#"{"type":"EPHEMERAL_MESSAGE","source":"SYSTEM","status":"DONE","created_at":"2026-05-23T01:02:05Z","content":"ignored"}"#,
        )
        .unwrap()
        .into_chat_messages();
        assert!(system.is_empty());
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
