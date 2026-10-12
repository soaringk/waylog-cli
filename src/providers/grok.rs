use super::{base::*, jsonl::JsonlReader};
use crate::error::{Result, WaylogError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct GrokProvider;
impl GrokProvider {
    pub fn new() -> Self {
        Self
    }
}
fn params(event: &Value) -> &Value {
    event.get("params").unwrap_or(event)
}
fn timestamp(event: &Value) -> Option<DateTime<Utc>> {
    params(event)["_meta"]["agentTimestampMs"]
        .as_i64()
        .and_then(DateTime::from_timestamp_millis)
        .or_else(|| {
            event["timestamp"]
                .as_f64()
                .and_then(|t| DateTime::from_timestamp_millis((t * 1000.0) as i64))
        })
}
async fn replay(path: &Path) -> Result<Vec<Value>> {
    let mut reader = JsonlReader::open(path).await?;
    let mut events = Vec::new();
    let mut starts = Vec::new();
    let mut in_user = false;
    let mut previous = None;
    let mut marked = false;
    while let Some(event) = reader.next::<Value>().await? {
        let update = &params(&event)["update"];
        let kind = update["sessionUpdate"].as_str();
        if event["method"] == "_x.ai/session/update" && kind == Some("rewind_marker") {
            if let Some(target) = update["target_prompt_index"]
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
            {
                if target < starts.len() {
                    events.truncate(starts[target]);
                    starts.truncate(target);
                }
            }
            in_user = false;
            previous = None;
            events.push(event);
            continue;
        }
        let index = update["_meta"]["promptIndex"].as_u64();
        let is_user = event["method"].as_str().unwrap_or("session/update") == "session/update"
            && kind == Some("user_message_chunk")
            && update["_meta"]["hostTurn"] != true;
        if is_user {
            marked |= index.is_some();
            if (!in_user || (marked && index != previous)) && (!marked || index.is_some()) {
                starts.push(events.len());
            }
        }
        in_user = is_user;
        previous = if is_user { index } else { None };
        events.push(event);
    }
    Ok(events)
}
#[async_trait]
impl Provider for GrokProvider {
    fn name(&self) -> &str {
        "grok"
    }
    fn has_history(&self) -> bool {
        crate::utils::path::home_dir().is_ok_and(|h| h.join(".grok/sessions").is_dir())
    }
    async fn parse_session(&self, path: &Path) -> Result<ChatSession> {
        let summary: Value =
            serde_json::from_slice(&tokio::fs::read(path.with_file_name("summary.json")).await?)?;
        let id = summary["info"]["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| WaylogError::Internal("Grok summary session ID is missing".into()))?;
        let events = replay(path).await?;
        let mut messages: Vec<ChatMessage> = Vec::new();
        let mut previous: Option<(MessageRole, Value, Value)> = None;
        let mut model = None;
        if !summary["session_kind"]
            .as_str()
            .is_some_and(|k| k.starts_with("subagent"))
        {
            for event in events {
                let p = params(&event);
                let update = &p["update"];
                let meta = &update["_meta"];
                if p["sessionId"].as_str().is_some_and(|other| other != id) {
                    continue;
                }
                let kind = update["sessionUpdate"].as_str().unwrap_or_default();
                if kind == "rewind_marker" {
                    previous = None;
                    model = None;
                    continue;
                }
                if event["method"].as_str().unwrap_or("session/update") != "session/update" {
                    previous = None;
                    continue;
                }
                let role = match kind {
                    "user_message_chunk" => MessageRole::User,
                    "agent_message_chunk" => MessageRole::Assistant(AssistantOutput::Message),
                    "agent_thought_chunk" => MessageRole::Assistant(AssistantOutput::Reasoning),
                    _ => {
                        previous = None;
                        if kind == "tool_call" || kind == "tool_call_update" {
                            let mut payload = update.clone();
                            if let Some(obj) = payload.as_object_mut() {
                                if let Some(id) = obj.remove("toolCallId") {
                                    obj.insert("call_id".into(), id);
                                }
                                obj.insert(
                                    "type".into(),
                                    Value::String(
                                        if kind == "tool_call_update" {
                                            "tool_result"
                                        } else {
                                            "tool_call"
                                        }
                                        .into(),
                                    ),
                                );
                            }
                            let mut message = ChatMessage::tool(timestamp(&event), payload);
                            message.metadata.model = model.clone();
                            messages.push(message);
                        }
                        continue;
                    }
                };
                if role == MessageRole::User {
                    model = meta["modelId"].as_str().map(str::to_string).or(model);
                }
                let hidden_prompt = p["_meta"]["promptId"].as_str().is_some_and(|id| {
                    [
                        "task-completed-",
                        "subagent-completed-",
                        "workflow-completed-",
                        "notifications-",
                        "goal-summary-",
                        "goal-classifier-nudge-",
                    ]
                    .iter()
                    .any(|prefix| id.starts_with(prefix))
                });
                if hidden_prompt
                    || meta["hostTurn"] == true
                    || meta["hideFromScrollback"] == true
                    || update["content"]["_meta"]["bash_command"].as_bool() == Some(true)
                {
                    previous = None;
                    continue;
                }
                let content = &update["content"];
                if content["type"] != "text" {
                    continue;
                }
                let Some(text) = content["_meta"]["displayText"]
                    .as_str()
                    .or_else(|| content["text"].as_str())
                else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                let key = (
                    role,
                    meta["promptIndex"].clone(),
                    p["_meta"]["promptId"].clone(),
                );
                if previous.as_ref() == Some(&key) {
                    messages.last_mut().unwrap().content.push_str(text);
                } else {
                    messages.push(ChatMessage {
                        timestamp: timestamp(&event),
                        role,
                        content: text.to_string(),
                        metadata: MessageMetadata {
                            model: if role == MessageRole::User {
                                None
                            } else {
                                model.clone()
                            },
                            ..Default::default()
                        },
                    });
                }
                previous = Some(key);
            }
        }
        let (first, last) = message_time_range(&messages);
        Ok(ChatSession {
            session_id: id.into(),
            provider: self.name().into(),
            project_path: summary["info"]["cwd"]
                .as_str()
                .map(PathBuf::from)
                .unwrap_or_default(),
            started_at: summary["created_at"]
                .as_str()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc))
                .or(first),
            updated_at: last,
            messages,
        })
    }
    async fn select_source_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        Ok(paths
            .into_iter()
            .filter(|p| p.file_name().is_some_and(|n| n == "updates.jsonl"))
            .collect())
    }
    async fn get_all_sessions(&self, project: &Path) -> Result<Vec<PathBuf>> {
        let root = crate::utils::path::home_dir()?.join(".grok/sessions");
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut paths = Vec::new();
        for entry in walkdir::WalkDir::new(root).min_depth(3).max_depth(3) {
            let entry = entry.map_err(|e| WaylogError::Io(std::io::Error::other(e)))?;
            if !entry.file_type().is_file() || entry.file_name() != "updates.jsonl" {
                continue;
            }
            let summary: Value =
                match tokio::fs::read(entry.path().with_file_name("summary.json")).await {
                    Ok(bytes) => match serde_json::from_slice(&bytes) {
                        Ok(summary) => summary,
                        Err(error) => {
                            tracing::warn!(%error,"Skipping malformed Grok discovery summary");
                            continue;
                        }
                    },
                    Err(error) => {
                        tracing::warn!(%error,"Skipping unreadable Grok discovery summary");
                        continue;
                    }
                };
            if summary["info"]["cwd"].as_str().map(Path::new) == Some(project)
                && !summary["session_kind"]
                    .as_str()
                    .is_some_and(|k| k.starts_with("subagent"))
            {
                paths.push((
                    entry
                        .metadata()
                        .map_err(|e| WaylogError::Io(std::io::Error::other(e)))?
                        .modified()?,
                    entry.path().to_path_buf(),
                ));
            }
        }
        paths.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
        Ok(paths.into_iter().map(|(_, p)| p).collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn chunk(kind: &str, text: &str, index: u64) -> Value {
        json!({"method":"session/update","timestamp":1,"params":{"sessionId":"session","update":{"sessionUpdate":kind,"content":{"type":"text","text":text},"_meta":{"promptIndex":index,"modelId":"grok-model"}}}})
    }
    #[tokio::test]
    async fn joins_streams_preserves_reasoning_and_replays_rewind() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.jsonl");
        std::fs::write(
            dir.path().join("summary.json"),
            json!({"info":{"id":"session","cwd":"/project"}}).to_string(),
        )
        .unwrap();
        let mut events = vec![
            chunk("user_message_chunk", "question ", 0),
            chunk("user_message_chunk", "  ", 0),
            chunk("agent_thought_chunk", "think", 0),
            chunk("agent_message_chunk", "answer ", 0),
            chunk("agent_message_chunk", "end  ", 0),
            chunk("user_message_chunk", "abandoned", 1),
        ];
        events.push(json!({"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"rewind_marker","target_prompt_index":1}}}));
        events.push(chunk("user_message_chunk", "replacement", 1));
        std::fs::write(
            &file,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let s = GrokProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.messages[0].content, "question   ");
        assert_eq!(s.messages[2].content, "answer end  ");
        assert_eq!(s.messages[3].content, "replacement");
        assert_eq!(s.messages[2].metadata.model.as_deref(), Some("grok-model"));
    }
    #[tokio::test]
    async fn omitted_media_does_not_split_text_within_one_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.jsonl");
        std::fs::write(
            dir.path().join("summary.json"),
            json!({"info":{"id":"session"}}).to_string(),
        )
        .unwrap();
        let mut image = chunk("user_message_chunk", "", 0);
        image["params"]["update"]["content"] = json!({"type":"image","data":"fixture"});
        let events = [
            chunk("user_message_chunk", "inspect ", 0),
            image,
            chunk("user_message_chunk", "this image  ", 0),
            chunk("agent_thought_chunk", "checking", 0),
            chunk("agent_message_chunk", "answer", 0),
            chunk("user_message_chunk", "next prompt", 1),
        ];
        std::fs::write(
            &file,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let session = GrokProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages.len(), 4);
        assert_eq!(session.messages[0].content, "inspect this image  ");
        assert_eq!(
            session.messages[1].role,
            MessageRole::Assistant(AssistantOutput::Reasoning)
        );
        assert_eq!(session.messages[3].content, "next prompt");
    }

    #[tokio::test]
    async fn rewind_resets_model_and_hidden_echo_advances_model() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.jsonl");
        std::fs::write(
            dir.path().join("summary.json"),
            json!({"info":{"id":"session"}}).to_string(),
        )
        .unwrap();
        let mut hidden = chunk("user_message_chunk", "hidden", 1);
        hidden["params"]["_meta"] = json!({"promptId":"task-completed-job"});
        hidden["params"]["update"]["_meta"]["modelId"] = json!("new-model");
        let events = vec![
            chunk("user_message_chunk", "first", 0),
            chunk("agent_message_chunk", "old", 0),
            chunk("user_message_chunk", "abandoned", 1),
            json!({"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"rewind_marker","target_prompt_index":1}}}),
            {
                let mut v = chunk("agent_message_chunk", "unattributed", 1);
                v["params"]["update"]["_meta"]
                    .as_object_mut()
                    .unwrap()
                    .remove("modelId");
                v
            },
            hidden,
            chunk("agent_message_chunk", "new", 2),
        ];
        std::fs::write(
            &file,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let session = GrokProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages.len(), 4);
        assert_eq!(session.messages[2].content, "unattributed");
        assert!(session.messages[2].metadata.model.is_none());
        assert_eq!(
            session.messages[3].metadata.model.as_deref(),
            Some("new-model")
        );
    }
    #[tokio::test]
    async fn directory_source_ignores_summary_and_keeps_updates() {
        let paths = vec![
            PathBuf::from("a/summary.json"),
            PathBuf::from("a/updates.jsonl"),
            PathBuf::from("a/chat_history.jsonl"),
        ];
        assert_eq!(
            GrokProvider::new()
                .select_source_paths(paths)
                .await
                .unwrap(),
            vec![PathBuf::from("a/updates.jsonl")]
        );
    }
    #[tokio::test]
    async fn rewind_to_zero_returns_same_empty_identity() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.jsonl");
        std::fs::write(
            dir.path().join("summary.json"),
            json!({"info":{"id":"session"}}).to_string(),
        )
        .unwrap();
        std::fs::write(&file,format!("{}\n{}\n",chunk("user_message_chunk","old",0),json!({"method":"_x.ai/session/update","params":{"update":{"sessionUpdate":"rewind_marker","target_prompt_index":0}}}))).unwrap();
        let s = GrokProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(s.session_id, "session");
        assert!(s.messages.is_empty());
        assert!(s.started_at.is_none());
    }
}
