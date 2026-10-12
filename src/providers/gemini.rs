use super::jsonl::JsonlReader;
use crate::error::{Result, WaylogError};
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::fs;

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

        self.sessions_in(&session_dir).await
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        if file_path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            return self.parse_jsonl_session(file_path).await;
        }

        let content = fs::read_to_string(file_path).await?;
        let session_data: GeminiSession =
            serde_json::from_str(&content).map_err(WaylogError::Json)?;

        self.conversation(file_path, session_data)
    }

    async fn select_source_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        let mut selected = Vec::new();
        for candidate in paths {
            if !matches!(
                candidate.extension().and_then(|value| value.to_str()),
                Some("json" | "jsonl")
            ) {
                continue;
            }
            if candidate.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && candidate.with_extension("json").is_file()
            {
                continue;
            }
            selected.push(self.preferred_recording(candidate).await);
        }
        Ok(selected)
    }

    fn has_history(&self) -> bool {
        // Gemini CLI might not be in PATH, check for data directory instead
        self.data_dir().map(|d| d.exists()).unwrap_or(false)
    }
}

impl GeminiProvider {
    async fn sessions_in(&self, session_dir: &Path) -> Result<Vec<PathBuf>> {
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
                // A migrated recording leaves its old snapshot beside the append-only log.
                if path.extension().and_then(|s| s.to_str()) == Some("jsonl")
                    && path.with_extension("json").is_file()
                {
                    continue;
                }
                let path = self.preferred_recording(path).await;
                let kind = if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                    self.replay_jsonl(&path)
                        .await
                        .ok()
                        .and_then(|(data, _)| data.kind)
                } else {
                    fs::read_to_string(&path)
                        .await
                        .ok()
                        .and_then(|content| serde_json::from_str::<GeminiSession>(&content).ok())
                        .and_then(|data| data.kind)
                };
                if kind.as_deref() == Some("subagent") {
                    continue;
                }
                let modified = fs::metadata(&path).await?.modified()?;
                candidates.push((path, modified));
            }
        }

        // Sort by modification time, newest first
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

        Ok(candidates.into_iter().map(|(p, _)| p).collect())
    }

    async fn preferred_recording(&self, snapshot: PathBuf) -> PathBuf {
        let log = snapshot.with_extension("jsonl");
        if snapshot
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
            || !log.is_file()
        {
            return snapshot;
        }
        let Ok((current, edited)) = self.replay_jsonl(&log).await else {
            return snapshot;
        };
        let legacy = fs::read_to_string(&snapshot)
            .await
            .ok()
            .and_then(|content| serde_json::from_str::<GeminiSession>(&content).ok());
        match legacy {
            Some(legacy)
                if legacy.session_id == current.session_id
                    && !edited
                    && current.messages.len() < legacy.messages.len() =>
            {
                snapshot
            }
            Some(legacy) if legacy.session_id != current.session_id => snapshot,
            _ => log,
        }
    }

    async fn parse_jsonl_session(&self, file_path: &Path) -> Result<ChatSession> {
        let (data, _) = self.replay_jsonl(file_path).await?;
        self.conversation(file_path, data)
    }

    async fn replay_jsonl(&self, file_path: &Path) -> Result<(GeminiSession, bool)> {
        let mut edited = false;
        let mut records = JsonlReader::open(file_path).await?;
        let mut metadata = serde_json::Map::new();
        let mut messages: Vec<Value> = Vec::new();
        while let Some(record) = records.next::<Value>().await? {
            if let Some(target) = record.get("$rewindTo").and_then(Value::as_str) {
                edited = true;
                let index = messages
                    .iter()
                    .position(|message| message["id"] == target)
                    .unwrap_or(0);
                messages.truncate(index);
            } else if let Some(patch) = record.get("$patch") {
                edited = true;
                apply_message_patch(&mut messages, patch);
            } else if record.get("id").and_then(Value::as_str).is_some()
                && record.get("type").is_some()
            {
                edited |= messages.iter().any(|message| message["id"] == record["id"]);
                replace_message(&mut messages, record);
            } else if let Some(update) = record.get("$set").unwrap_or(&record).as_object() {
                if let Some(checkpoint) = update.get("messages").and_then(Value::as_array) {
                    if record.get("$set").is_some() {
                        edited = true;
                        messages.clear();
                    }
                    for message in checkpoint {
                        replace_message(&mut messages, message.clone());
                    }
                }
                metadata.extend(
                    update
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            }
        }
        metadata.insert("messages".to_string(), Value::Array(messages));
        Ok((serde_json::from_value(Value::Object(metadata))?, edited))
    }

    fn conversation(&self, file_path: &Path, data: GeminiSession) -> Result<ChatSession> {
        let mut messages = Vec::new();
        if data.kind.as_deref() != Some("subagent") {
            for value in data.messages {
                if matches!(
                    value.get("type").and_then(Value::as_str),
                    Some("user" | "gemini")
                ) {
                    messages.extend(self.parse_message(serde_json::from_value(value)?));
                }
            }
        }
        let project_path = file_path
            .parent()
            .and_then(Path::parent)
            .and_then(|directory| std::fs::read_to_string(directory.join(".project_root")).ok())
            .map(|value| PathBuf::from(value.trim()))
            .unwrap_or_default();
        Ok(ChatSession {
            session_id: data.session_id,
            provider: self.name().to_string(),
            project_path,
            started_at: parse_timestamp(data.start_time.as_deref()),
            updated_at: parse_timestamp(data.last_updated.as_deref()),
            messages,
        })
    }

    fn parse_message(&self, msg: GeminiMessage) -> Vec<ChatMessage> {
        let role = if msg.message_type == "user" {
            MessageRole::User
        } else {
            MessageRole::Assistant(AssistantOutput::Message)
        };
        let timestamp = parse_timestamp(msg.timestamp.as_deref());
        let display = msg
            .display_content
            .map(GeminiContent::into_text)
            .unwrap_or_default();
        let content = if display.trim().is_empty() {
            msg.content
                .map(GeminiContent::into_text)
                .unwrap_or_default()
        } else {
            display
        };
        let mut records = Vec::new();
        let thoughts = msg
            .thoughts
            .unwrap_or_default()
            .into_iter()
            .map(|thought| format!("{}: {}", thought.subject, thought.description))
            .collect::<Vec<_>>();
        if !content.trim().is_empty() || !thoughts.is_empty() {
            records.push(ChatMessage {
                timestamp,
                role,
                content,
                metadata: MessageMetadata {
                    model: msg.model.clone(),
                    thoughts,
                    ..Default::default()
                },
            });
        }
        if matches!(role, MessageRole::Assistant(_)) {
            for mut call in msg.tool_calls {
                let id = call.get("id").and_then(Value::as_str).map(str::to_string);
                let result = call
                    .as_object_mut()
                    .and_then(|object| object.remove("result"));
                let mut tool = ChatMessage::tool(
                    parse_timestamp(call.get("timestamp").and_then(Value::as_str)),
                    call,
                );
                tool.metadata.model = msg.model.clone();
                records.push(tool);
                if let Some(result) = result.filter(|result| !result.is_null()) {
                    let mut tool = ChatMessage::tool(
                        None,
                        serde_json::json!({"type":"tool_result", "tool_use_id":id, "content":result}),
                    );
                    tool.metadata.model = msg.model.clone();
                    records.push(tool);
                }
            }
        }
        if let Some(first) = records.first_mut() {
            first.metadata.tokens = msg.tokens.map(|tokens| TokenUsage {
                input: tokens.input,
                output: tokens.output,
                cached: tokens.cached,
            });
        }
        records
    }
}

fn parse_timestamp(value: Option<&str>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn replace_message(messages: &mut Vec<Value>, value: Value) {
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return;
    };
    if let Some(existing) = messages.iter_mut().find(|message| message["id"] == id) {
        *existing = value;
    } else {
        messages.push(value);
    }
}

fn patch_one(messages: &mut [Value], patch: &Value) {
    let Some(id) = patch.get("id").and_then(Value::as_str) else {
        return;
    };
    let Some(message) = messages.iter_mut().find(|message| message["id"] == id) else {
        return;
    };
    if let Some(content) = patch.get("content") {
        message["content"] = content.clone();
    }
    if let (Some(updates), Some(calls)) = (
        patch.get("toolCalls").and_then(Value::as_array),
        message.get_mut("toolCalls").and_then(Value::as_array_mut),
    ) {
        for update in updates {
            if let Some(call) = calls.iter_mut().find(|call| call["id"] == update["id"]) {
                if let Some(result) = update.get("result") {
                    call["result"] = result.clone();
                }
            }
        }
    }
}

fn apply_message_patch(messages: &mut Vec<Value>, patch: &Value) {
    patch_one(messages, patch);
    if let Some(updates) = patch.get("updates").and_then(Value::as_array) {
        for update in updates {
            patch_one(messages, update);
        }
    }
    if let Some(ids) = patch.get("removeIds").and_then(Value::as_array) {
        messages.retain(|message| !ids.contains(&message["id"]));
    }
    if let Some(ids) = patch.get("orderIds").and_then(Value::as_array) {
        let mut ordered = Vec::new();
        for id in ids {
            if let Some(index) = messages.iter().position(|message| message["id"] == *id) {
                ordered.push(messages.remove(index));
            }
        }
        messages.extend(ordered);
    }
}

// Gemini JSON session structures
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiSession {
    session_id: String,
    start_time: Option<String>,
    last_updated: Option<String>,
    #[serde(default)]
    messages: Vec<Value>,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiMessage {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    message_type: String,
    content: Option<GeminiContent>,
    display_content: Option<GeminiContent>,
    model: Option<String>,
    thoughts: Option<Vec<GeminiThought>>,
    tokens: Option<GeminiTokens>,
    #[serde(default)]
    tool_calls: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GeminiContent {
    Text(String),
    Parts(Vec<GeminiContent>),
    Part(GeminiContentPart),
}

impl GeminiContent {
    fn into_text(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Parts(parts) => parts.into_iter().map(Self::into_text).collect::<String>(),
            Self::Part(part) => {
                if part.thought {
                    String::new()
                } else {
                    part.text.unwrap_or_default()
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct GeminiContentPart {
    text: Option<String>,
    #[serde(default)]
    thought: bool,
}

#[derive(Debug, Deserialize)]
struct GeminiThought {
    subject: String,
    description: String,
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
    async fn latest_discovery_excludes_newer_subagent_recordings() {
        let dir = TempDir::new().unwrap();
        let main = dir.path().join("session-main.json");
        let child = dir.path().join("session-child.jsonl");
        fs::write(
            &main,
            serde_json::json!({"sessionId":"main","messages":[{"type":"user","content":"main"}]})
                .to_string(),
        )
        .await
        .unwrap();
        fs::write(&child, serde_json::json!({"sessionId":"child","kind":"subagent","messages":[{"type":"user","content":"child"}]}).to_string()).await.unwrap();
        assert_eq!(
            GeminiProvider.sessions_in(dir.path()).await.unwrap(),
            vec![main]
        );
    }

    async fn parse_records(records: &[Value]) -> ChatSession {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("session.jsonl");
        let mut lines = vec![serde_json::json!({"sessionId":"test","projectHash":"hash","startTime":"not-a-time","lastUpdated":null}).to_string()];
        lines.extend(records.iter().map(Value::to_string));
        fs::write(&file, lines.join("\n")).await.unwrap();
        GeminiProvider.parse_session(&file).await.unwrap()
    }

    #[tokio::test]
    async fn accepts_native_single_parts_and_mixed_part_lists() {
        let session = parse_records(&[
            serde_json::json!({"id":"u","type":"user","content":{"text":"single part  "}}),
            serde_json::json!({"id":"a","type":"gemini","content":["prefix ",{"text":"body"},{"inlineData":{"mimeType":"image/png","data":"fixture"}}," suffix  "]}),
        ]).await;
        assert_eq!(session.messages[0].content, "single part  ");
        assert_eq!(session.messages[1].content, "prefix body suffix  ");
    }

    #[tokio::test]
    async fn preserves_message_usage_once_with_or_without_text_before_tools() {
        for content in ["", "Reading the file"] {
            let session = parse_records(&[serde_json::json!({
                "id":"a","type":"gemini","content":content,"model":"gemini-model",
                "tokens":{"input":12,"output":6,"cached":2},
                "toolCalls":[
                    {"id":"first","name":"read_file","args":{"path":"a"},"result":[{"text":"contents"}]},
                    {"id":"second","name":"read_file","args":{"path":"b"}}
                ]
            })]).await;
            let usage: Vec<_> = session
                .messages
                .iter()
                .filter_map(|message| message.metadata.tokens.as_ref())
                .collect();
            assert_eq!(usage.len(), 1);
            assert_eq!(
                (usage[0].input, usage[0].output, usage[0].cached),
                (12, 6, 2)
            );
            assert!(session.messages[0].metadata.tokens.is_some());
            assert_eq!(
                session.messages[0].role,
                MessageRole::Assistant(if content.is_empty() {
                    AssistantOutput::Tool
                } else {
                    AssistantOutput::Message
                })
            );
        }
    }

    #[tokio::test]
    async fn migration_keeps_complete_snapshot_until_log_catches_up_but_honors_rewind() {
        let dir = TempDir::new().unwrap();
        let snapshot = dir.path().join("session.json");
        let log = dir.path().join("session.jsonl");
        let header = serde_json::json!({"sessionId":"migrating","projectHash":"hash"});
        let user = serde_json::json!({"id":"u","type":"user","content":"ask"});
        let answer = serde_json::json!({"id":"a","type":"gemini","content":"answer"});
        let mut legacy = header.clone();
        legacy["messages"] = serde_json::json!([user, answer]);
        fs::write(&snapshot, legacy.to_string()).await.unwrap();
        fs::write(&log, format!("{header}\n{user}\n{{\"id\":"))
            .await
            .unwrap();
        assert_eq!(
            GeminiProvider.preferred_recording(snapshot.clone()).await,
            snapshot
        );
        fs::write(&log, format!("{header}\n{user}\n{answer}"))
            .await
            .unwrap();
        assert_eq!(
            GeminiProvider.preferred_recording(snapshot.clone()).await,
            log
        );
        fs::write(
            &log,
            format!(
                "{header}\n{user}\n{answer}\n{}",
                serde_json::json!({"$rewindTo":"a"})
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            GeminiProvider.preferred_recording(snapshot.clone()).await,
            log
        );
        assert_eq!(
            GeminiProvider
                .parse_session(&log)
                .await
                .unwrap()
                .messages
                .len(),
            1
        );
        fs::write(
            &log,
            format!("{header}\n{}", serde_json::json!({"$set":{"messages":[]}})),
        )
        .await
        .unwrap();
        assert_eq!(GeminiProvider.preferred_recording(snapshot).await, log);
    }

    #[tokio::test]
    async fn replays_replacements_checkpoints_patches_and_rewinds_in_native_order() {
        let session = parse_records(&[
            serde_json::json!({"id":"u","type":"user","content":"old"}),
            serde_json::json!({"$set":{"messages":[
                {"id":"u","type":"user","content":"ask","timestamp":"2026-01-01T00:00:05Z"},
                {"id":"a","type":"gemini","content":"draft","timestamp":"2026-01-01T00:00:01Z"},
                {"id":"cut","type":"gemini","content":"discard"}
            ]}}),
            serde_json::json!({"id":"a","type":"gemini","content":"replacement","timestamp":"2026-01-01T00:00:01Z"}),
            serde_json::json!({"$patch":{"updates":[{"id":"a","content":"patched"}]}}),
            serde_json::json!({"$rewindTo":"cut"}),
            serde_json::json!({"id":"a2","type":"gemini","content":"later"}),
            serde_json::json!({"$patch":{"removeIds":["a2"],"orderIds":["a"]}}),
        ]).await;
        assert_eq!(
            session
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["ask", "patched"]
        );
        assert!(session.messages[0].timestamp > session.messages[1].timestamp);
        assert!(session.started_at.is_none());
        assert!(session.updated_at.is_none());
        assert!(session.project_path.as_os_str().is_empty());
        let cleared = parse_records(&[
            serde_json::json!({"id":"u","type":"user","content":"ask"}),
            serde_json::json!({"$rewindTo":"absent"}),
        ])
        .await;
        assert!(cleared.messages.is_empty());
    }

    #[tokio::test]
    async fn patch_order_keeps_unmentioned_messages_first() {
        let session = parse_records(&[
            serde_json::json!({"id":"one","type":"user","content":"one"}),
            serde_json::json!({"id":"two","type":"user","content":"two"}),
            serde_json::json!({"id":"three","type":"user","content":"three"}),
            serde_json::json!({"$patch":{"orderIds":["three","one"]}}),
        ])
        .await;
        assert_eq!(
            session
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "three", "one"]
        );
    }

    #[tokio::test]
    async fn exports_display_text_thoughts_and_patched_tool_results() {
        let session = parse_records(&[
            serde_json::json!({"id":"u","type":"user","content":"expanded context","displayContent":[{"text":"human "},{"text":"request  "}]}),
            serde_json::json!({"id":"a","type":"gemini","content":"","model":"recorded-model","thoughts":[{"subject":"plan","description":"check"}],"toolCalls":[{"id":"c","name":"read","args":{"path":"x"},"status":"executing"}]}),
            serde_json::json!({"$patch":{"id":"a","toolCalls":[{"id":"c","result":[{"text":"contents  "}]}]}}),
        ]).await;
        assert_eq!(session.messages[0].content, "human request  ");
        assert_eq!(session.messages[1].metadata.thoughts, vec!["plan: check"]);
        assert_eq!(
            session.messages[2].metadata.model.as_deref(),
            Some("recorded-model")
        );
        assert!(session.messages[2].timestamp.is_none());
        let value: Value =
            serde_json::from_str(&crate::exporter::json::generate(&session, true).unwrap())
                .unwrap();
        assert_eq!(value["turns"][1]["parts"][1]["tool_call_id"], "c");
        assert!(value["turns"][1]["parts"][1]["result"]
            .as_str()
            .unwrap()
            .contains("contents  "));
        let without_tools: Value =
            serde_json::from_str(&crate::exporter::json::generate(&session, false).unwrap())
                .unwrap();
        assert_eq!(
            without_tools["turns"][1]["parts"].as_array().unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn legacy_json_does_not_invent_dates_or_a_project_path() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("session.json");
        fs::write(&file, serde_json::json!({"sessionId":"legacy","projectHash":"opaque-hash","startTime":"bad","messages":[
            {"id":"u","type":"user","content":[{"text":"line1\n"},{"text":"line2  "}]},
            {"id":"a","type":"gemini","timestamp":"invalid","content":"answer","displayContent":" "}
        ]}).to_string()).await.unwrap();
        let session = GeminiProvider.parse_session(&file).await.unwrap();
        assert!(session.started_at.is_none() && session.updated_at.is_none());
        assert!(session.project_path.as_os_str().is_empty());
        assert_eq!(session.messages[0].content, "line1\nline2  ");
        assert_eq!(session.messages[1].content, "answer");
        assert!(session
            .messages
            .iter()
            .all(|message| message.timestamp.is_none()));
    }

    #[tokio::test]
    async fn subagent_recordings_are_not_attributed_to_the_main_assistant() {
        let session = parse_records(&[
            serde_json::json!({"$set":{"kind":"subagent"}}),
            serde_json::json!({"id":"u","type":"user","content":"child"}),
        ])
        .await;
        assert!(session.messages.is_empty());
    }

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
