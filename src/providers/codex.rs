use super::jsonl::JsonlReader;
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
    data_dirs: Option<Vec<PathBuf>>,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self { data_dirs: None }
    }

    #[cfg(test)]
    fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dirs: Some(vec![data_dir]),
        }
    }

    #[cfg(test)]
    fn with_data_dirs(directories: Vec<PathBuf>) -> Self {
        Self {
            data_dirs: Some(directories),
        }
    }

    fn data_dirs(&self) -> Result<Vec<PathBuf>> {
        if let Some(directories) = &self.data_dirs {
            return Ok(directories.clone());
        }

        let root = path::home_dir()?.join(".codex");
        Ok(vec![root.join("sessions"), root.join("archived_sessions")])
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        for base_session_dir in self.data_dirs()? {
            for entry in walkdir::WalkDir::new(base_session_dir)
                .into_iter()
                .filter_map(|entry| entry.ok())
            {
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                if path.is_file()
                    && path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                    && (stem == session_id || stem.ends_with(session_id))
                    && self.probe_project_path(path, project_path).await?
                {
                    return Ok(Some(path.to_path_buf()));
                }
            }
        }

        Ok(None)
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let mut candidates = Vec::new();
        for directory in self.data_dirs()? {
            for entry in walkdir::WalkDir::new(directory)
                .into_iter()
                .filter_map(|entry| entry.ok())
            {
                let path = entry.path();
                if path.is_file()
                    && path.extension().and_then(|s| s.to_str()) == Some("jsonl")
                    && self
                        .probe_project_path(path, project_path)
                        .await
                        .unwrap_or(false)
                {
                    if let Ok(modified) = fs::metadata(path)
                        .await
                        .and_then(|metadata| metadata.modified())
                    {
                        candidates.push((path.to_path_buf(), modified));
                    }
                }
            }
        }

        // Sort by modification time, newest first
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

        Ok(candidates.into_iter().map(|(p, _)| p).collect())
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        let mut records = JsonlReader::open(file_path).await?;

        let mut messages = Vec::new();
        let mut legacy_messages = Vec::new();
        let mut completed_messages = Vec::new();
        let mut session_id = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut session_project_path = PathBuf::new();
        // A resumed or forked rollout replays the sessions it continues, so only the
        // first `session_meta` identifies this file.
        let mut identified = false;
        let mut current_model: Option<String> = None;

        while let Some(event) = records.next::<CodexEvent>().await? {
            match event.event_type.as_str() {
                "session_meta" if !identified => {
                    identified = true;
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
                    current_model = event
                        .payload
                        .as_ref()
                        .and_then(|payload| payload.get("model"))
                        .and_then(Value::as_str)
                        .filter(|model| !model.is_empty())
                        .map(str::to_string);
                    if let Some(cwd) = event
                        .payload
                        .as_ref()
                        .and_then(|payload| payload.get("cwd"))
                        .and_then(Value::as_str)
                    {
                        session_project_path = PathBuf::from(cwd);
                    }
                }
                "event_msg" => {
                    if let Some(payload) = event.payload {
                        let kind = payload.get("type").and_then(Value::as_str);
                        if kind == Some("agent_message")
                            && (payload.get("author").is_some()
                                || payload.get("recipient").is_some())
                        {
                            continue;
                        }
                        let (role, content, completed) = match kind {
                            Some("user_message") => (
                                Some(MessageRole::User),
                                payload
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                false,
                            ),
                            Some("agent_message") => (
                                Some(MessageRole::Assistant(AssistantOutput::Message)),
                                payload
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                false,
                            ),
                            Some("item_completed") => {
                                let item = &payload["item"];
                                let role = match item["type"].as_str() {
                                    Some("UserMessage") => Some(MessageRole::User),
                                    Some("AgentMessage") => {
                                        Some(MessageRole::Assistant(AssistantOutput::Message))
                                    }
                                    _ => None,
                                };
                                let content =
                                    item.get("content").and_then(Value::as_array).map(|parts| {
                                        parts
                                            .iter()
                                            .filter_map(|part| {
                                                part.get("text").and_then(Value::as_str)
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n\n")
                                    });
                                (role, content, true)
                            }
                            _ => (None, None, false),
                        };
                        if let (Some(role), Some(content)) =
                            (role, content.filter(|text| !text.is_empty()))
                        {
                            let record = ChatMessage {
                                timestamp: event
                                    .timestamp
                                    .as_deref()
                                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                                    .map(|value| value.with_timezone(&Utc)),
                                role,
                                content,
                                metadata: MessageMetadata {
                                    model: matches!(role, MessageRole::Assistant(_))
                                        .then(|| current_model.clone())
                                        .flatten(),
                                    ..Default::default()
                                },
                            };
                            if completed {
                                completed_messages.push(record);
                            } else {
                                legacy_messages.push(record);
                            }
                        }
                    }
                }
                "response_item" => {
                    if let Some(payload) = event.payload {
                        for mut msg in
                            self.parse_response_item(payload, event.timestamp.as_deref())?
                        {
                            if matches!(msg.role, MessageRole::Assistant(_)) {
                                msg.metadata.model = current_model.clone();
                            }
                            messages.push(msg);
                        }
                    }
                }
                _ => {}
            }
        }

        // Event messages mirror response items in modern rollouts; never export both.
        if messages.is_empty() {
            messages = if legacy_messages.is_empty() {
                completed_messages
            } else {
                legacy_messages
            };
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
        self.data_dirs()
            .is_ok_and(|directories| directories.iter().any(|directory| directory.is_dir()))
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
            return Ok(vec![ChatMessage::tool(timestamp, payload)]);
        }

        if payload.get("type").and_then(Value::as_str) == Some("reasoning") {
            // Codex stores the verbatim chain of thought encrypted; only the summary is readable.
            let reasoning = payload
                .get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n");
            return Ok(if reasoning.trim().is_empty() {
                Vec::new()
            } else {
                vec![ChatMessage::reasoning(timestamp, reasoning)]
            });
        }

        let role = match payload.get("role").and_then(Value::as_str) {
            Some("user") => MessageRole::User,
            Some("assistant") => MessageRole::Assistant(AssistantOutput::Message),
            Some("developer") => MessageRole::System,
            _ => return Ok(Vec::new()),
        };

        // Content holds text and media WayLog drops, so one request is one message.
        let items = payload
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item.get("text").or_else(|| item.get("refusal")).is_some())
            .cloned()
            .collect();
        Ok(join_consecutive_text(items, &["input_text", "output_text"])
            .into_iter()
            .filter_map(|item| {
                item.get("text")
                    .or_else(|| item.get("refusal"))
                    .and_then(Value::as_str)
                    .filter(|content| !content.is_empty())
                    .map(str::to_string)
            })
            .map(|content| ChatMessage {
                timestamp,
                role,
                content,
                metadata: MessageMetadata::default(),
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
    async fn event_only_histories_export_without_duplicating_modern_mirrors() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("events.jsonl");
        let lines = [
            serde_json::json!({"type":"session_meta","payload":{"id":"own"}}),
            serde_json::json!({"type":"turn_context","payload":{"model":"native-model"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"request  "}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"agent_message","message":"answer"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"agent_message","message":"child traffic","author":"child","recipient":"parent"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","content":[{"text":"answer"}]}}}),
        ];
        let content = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&file, &content).await.unwrap();
        let session = CodexProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].content, "request  ");
        assert_eq!(
            session.messages[1].metadata.model.as_deref(),
            Some("native-model")
        );
        let modern = serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"response stream"}]}});
        fs::write(&file, format!("{content}\n{modern}"))
            .await
            .unwrap();
        let session = CodexProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.messages[0].content, "response stream");
        fs::write(&file, serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"text":"only completed items"}]}}}).to_string()).await.unwrap();
        assert_eq!(
            CodexProvider::new()
                .parse_session(&file)
                .await
                .unwrap()
                .messages[0]
                .content,
            "only completed items"
        );
    }

    #[tokio::test]
    async fn discovers_archived_sessions_when_active_history_is_missing() {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        fs::create_dir(&project).await.unwrap();
        let active = dir.path().join("sessions");
        let archive = dir.path().join("archived_sessions");
        fs::create_dir(&archive).await.unwrap();
        let rollout = archive.join("rollout-archived-id.jsonl");
        fs::write(&rollout, session_meta_line("archived-id", &project))
            .await
            .unwrap();
        fs::write(
            archive.join("rollout-other-id.jsonl"),
            session_meta_line("other-id", &dir.path().join("other-project")),
        )
        .await
        .unwrap();
        let provider = CodexProvider::with_data_dirs(vec![active, archive]);
        assert!(provider.has_history());
        assert_eq!(
            provider.get_all_sessions(&project).await.unwrap(),
            vec![rollout.clone()]
        );
        assert_eq!(
            provider
                .find_session(&project, "archived-id")
                .await
                .unwrap(),
            Some(rollout.clone())
        );
        assert_eq!(
            provider
                .get_all_sessions(&project)
                .await
                .unwrap()
                .into_iter()
                .next(),
            Some(rollout)
        );
    }

    #[tokio::test]
    async fn records_model_changes_without_borrowing_a_future_turn_model() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("models.jsonl");
        let mut lines = vec![session_meta_line("own-id", dir.path())];
        for model in [None, Some("model-one"), Some("model-two"), None] {
            lines.push(
                serde_json::json!({"type":"turn_context","payload":{"model":model}}).to_string(),
            );
            lines.push(serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]}}).to_string());
            lines.push(serde_json::json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"summary"}]}}).to_string());
        }
        fs::write(&file, lines.join("\n") + "\n{\"type\":")
            .await
            .unwrap();
        let session = CodexProvider::new().parse_session(&file).await.unwrap();
        let models = session
            .messages
            .iter()
            .map(|message| message.metadata.model.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            models,
            vec![
                None,
                None,
                Some("model-one"),
                Some("model-one"),
                Some("model-two"),
                Some("model-two"),
                None,
                None
            ]
        );
        assert_eq!(session.session_id, "own-id");
    }

    #[tokio::test]
    async fn discovers_session_outside_recent_date_directories() {
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
            .get_all_sessions(&project_path)
            .await
            .unwrap();

        assert_eq!(found, vec![session_path]);
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
            .all(|message| message.role == MessageRole::Assistant(AssistantOutput::Tool)));
        for (message, expected) in session.messages.iter().zip(expected) {
            assert_eq!(
                serde_json::from_str::<Value>(&message.content).unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn keeps_injected_user_text_in_one_message_per_request() {
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

        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.started_at, None);
        assert_eq!(session.updated_at, None);
        assert!(session
            .messages
            .iter()
            .all(|message| message.timestamp.is_none()));
        assert_eq!(
            session.messages[0].content,
            "<environment_context>keep me</environment_context>\n\nsecond block"
        );
    }

    #[tokio::test]
    async fn dropped_media_does_not_split_one_request() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("session.jsonl");
        let event = serde_json::json!({
            "type": "response_item",
            "timestamp": "2026-08-14T00:00:00Z",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "before the screenshot"},
                    {"type": "input_image", "image_url": "data:..."},
                    {"type": "input_text", "text": "after the screenshot"}
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

        // The image is not exported, so it cannot split what the user sent at once.
        assert_eq!(
            session
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            ["before the screenshot\n\nafter the screenshot"]
        );
    }

    #[tokio::test]
    async fn identity_comes_from_the_rollouts_own_session_meta() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("resumed.jsonl");
        let project_path = temp_dir.path().join("project");
        tokio::fs::create_dir(&project_path).await.unwrap();
        // A resumed rollout replays the session it continues, including its metadata.
        tokio::fs::write(
            &file_path,
            format!(
                "{}\n{}",
                session_meta_line("resumed-session", &project_path),
                session_meta_line("replayed-earlier-session", &project_path),
            ),
        )
        .await
        .unwrap();

        let session = CodexProvider::new()
            .parse_session(&file_path)
            .await
            .unwrap();

        assert_eq!(session.session_id, "resumed-session");
    }

    #[tokio::test]
    async fn records_readable_reasoning_and_skips_encrypted_only_reasoning() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("session.jsonl");
        let payloads = [
            serde_json::json!({
                "type": "reasoning",
                "id": "reasoning-1",
                "summary": [
                    {"type": "summary_text", "text": "**Planning the search**"},
                    {"type": "summary_text", "text": "Checking the submodule"}
                ],
                "encrypted_content": "opaque"
            }),
            serde_json::json!({
                "type": "reasoning",
                "id": "reasoning-2",
                "summary": [],
                "encrypted_content": "opaque"
            }),
            serde_json::json!({
                "type": "message",
                "id": "message-1",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Found it"}]
            }),
            serde_json::json!({
                "type": "message",
                "id": "message-2",
                "role": "developer",
                "content": [{"type": "input_text", "text": "<permissions instructions>"}]
            }),
        ];
        let lines = payloads
            .iter()
            .map(|payload| {
                serde_json::json!({
                    "type": "response_item",
                    "timestamp": "2026-08-13T00:00:00Z",
                    "payload": payload
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        tokio::fs::write(&file_path, lines).await.unwrap();

        let session = CodexProvider::new()
            .parse_session(&file_path)
            .await
            .unwrap();

        let recorded = session
            .messages
            .iter()
            .map(|message| (message.role, message.content.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            recorded,
            vec![
                (
                    MessageRole::Assistant(AssistantOutput::Reasoning),
                    "**Planning the search**\n\nChecking the submodule"
                ),
                (MessageRole::Assistant(AssistantOutput::Message), "Found it"),
                (MessageRole::System, "<permissions instructions>"),
            ]
        );
    }
}
