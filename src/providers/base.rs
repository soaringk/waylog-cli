use crate::error::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One record of a conversation, as the provider recorded it.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub timestamp: Option<DateTime<Utc>>,
    pub role: MessageRole,
    pub content: String,
    pub metadata: MessageMetadata,
}

impl ChatMessage {
    /// Create a normalized provider tool record.
    pub fn tool(timestamp: Option<DateTime<Utc>>, payload: Value) -> Self {
        Self {
            timestamp,
            role: MessageRole::Assistant(AssistantOutput::Tool),
            content: readable_tool_payload(&payload),
            metadata: MessageMetadata {
                tool_call_id: tool_call_id(&payload),
                ..Default::default()
            },
        }
    }

    /// Create an assistant reasoning step from the provider's readable reasoning text.
    pub fn reasoning(timestamp: Option<DateTime<Utc>>, content: String) -> Self {
        Self {
            timestamp,
            role: MessageRole::Assistant(AssistantOutput::Reasoning),
            content,
            metadata: MessageMetadata::default(),
        }
    }
}

/// Who produced a record. Everything the model produces belongs to `Assistant`, which
/// carries which kind of output it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    System,
    Assistant(AssistantOutput),
}

/// A kind of output the model produces while working on one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantOutput {
    /// A readable step of the model's reasoning.
    Reasoning,
    /// Text the model addressed to the user.
    Message,
    /// A tool request or its result.
    Tool,
}

#[derive(Debug, Clone, Default)]
pub struct MessageMetadata {
    /// Model used (e.g., "claude-sonnet-4.5", "gemini-2.5-flash")
    pub model: Option<String>,

    /// Token usage
    pub tokens: Option<TokenUsage>,

    /// Tool names associated with this message
    pub tool_calls: Vec<String>,

    /// Provider call ID used to group a tool request with its response
    pub tool_call_id: Option<String>,

    /// Thoughts (for Gemini)
    pub thoughts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub input: u32,
    pub output: u32,
    pub cached: u32,
}

/// One complete conversation, in the order the provider recorded it.
#[derive(Debug, Clone)]
pub struct ChatSession {
    pub session_id: String,
    pub provider: String,
    pub project_path: PathBuf,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub messages: Vec<ChatMessage>,
}

pub fn message_time_range(
    messages: &[ChatMessage],
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let started_at = messages.iter().find_map(|message| message.timestamp);
    let updated_at = messages.iter().rev().find_map(|message| message.timestamp);
    (started_at, updated_at)
}

/// Join text blocks a provider recorded next to each other in one request. Callers pass
/// only the blocks they export, so text never merges across a record that WayLog keeps,
/// and a block WayLog drops cannot split one submission in two.
pub fn join_consecutive_text(items: Vec<Value>, text_types: &[&str]) -> Vec<Value> {
    let text = |item: &Value| {
        item.get("type")
            .and_then(Value::as_str)
            .filter(|kind| text_types.contains(kind))
            .and_then(|_| item.get("text").and_then(Value::as_str))
            .map(str::to_string)
    };

    let mut joined: Vec<Value> = Vec::new();
    for item in items {
        match (text(&item), joined.last_mut().and_then(|last| text(last))) {
            (Some(next), Some(previous)) => {
                let last = joined.last_mut().expect("checked above");
                last["text"] = Value::String(format!("{previous}\n\n{next}"));
            }
            _ => joined.push(item),
        }
    }
    joined
}

pub fn is_tool_payload(payload: &Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };

    if ["call_id", "callID", "tool_use_id", "caller"]
        .iter()
        .any(|field| object.contains_key(*field))
    {
        return true;
    }

    if object.contains_key("tool") && object.contains_key("state") {
        return true;
    }

    if object
        .get("function")
        .and_then(Value::as_object)
        .is_some_and(|function| function.contains_key("name") && function.contains_key("arguments"))
    {
        return true;
    }

    object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|item_type| {
            item_type
                .split(['_', '-', '.'])
                .any(|part| matches!(part, "call" | "tool" | "tools"))
        })
}

fn tool_call_id(payload: &Value) -> Option<String> {
    ["call_id", "callID", "tool_use_id", "id"]
        .iter()
        .find_map(|field| payload.get(*field).and_then(Value::as_str))
        .map(str::to_string)
}

fn readable_tool_payload(payload: &Value) -> String {
    normalize_tool_payload(payload)
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            value => serde_json::to_string_pretty(&value).ok(),
        })
        .unwrap_or_else(|| pretty_json(payload))
}

fn normalize_tool_payload(payload: &Value) -> Option<Value> {
    let mut object = payload.as_object()?.clone();

    for field in [
        "type",
        "id",
        "call_id",
        "callID",
        "tool_use_id",
        "internal_chat_message_metadata_passthrough",
    ] {
        object.remove(field);
    }
    if object.get("execution").and_then(Value::as_str) == Some("client") {
        object.remove("execution");
    }
    if object.get("status").and_then(Value::as_str) == Some("completed") {
        object.remove("status");
    }
    if object.get("is_error").and_then(Value::as_bool) == Some(false) {
        object.remove("is_error");
    }

    if let Some(Value::String(arguments)) = object.get("arguments") {
        let parsed = serde_json::from_str(arguments).ok()?;
        object.insert("arguments".to_string(), parsed);
    }

    if object.contains_key("tool") {
        if let Some(state) = object.remove("state") {
            let mut state = state.as_object()?.clone();
            if state.get("status").and_then(Value::as_str) == Some("completed") {
                state.remove("status");
            }
            state.remove("time");
            for (field, value) in state {
                if object.insert(field, value).is_some() {
                    return None;
                }
            }
        }
    }

    if object.is_empty() {
        return None;
    }
    if object.len() == 1 {
        let (field, value) = object.iter().next()?;
        if matches!(
            field.as_str(),
            "content" | "name" | "output" | "result" | "tool"
        ) && value.is_string()
        {
            return Some(value.clone());
        }
    }

    Some(Value::Object(object))
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Converts one provider's discovered or supplied history into chat sessions.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the provider name (e.g., "codex", "claude", "gemini")
    fn name(&self) -> &str;

    /// Whether session discovery is limited to a project path.
    fn is_project_scoped(&self) -> bool {
        true
    }

    /// Find the latest session file for the current project
    async fn find_latest_session(&self, project_path: &Path) -> Result<Option<PathBuf>> {
        Ok(self
            .get_all_sessions(project_path)
            .await?
            .into_iter()
            .next())
    }

    /// Find one session by its provider session ID.
    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        for session_path in self.get_all_sessions(project_path).await? {
            let session = self.parse_session(&session_path).await?;
            if session.session_id == session_id {
                return Ok(Some(session_path));
            }
        }
        Ok(None)
    }

    /// Parse a session file and return a chat session
    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession>;

    /// Get all session files for a specific project
    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>>;

    /// Check whether provider history is available.
    fn has_history(&self) -> bool;

    /// Get the CLI command when this provider can be launched by `waylog run`.
    fn run_command(&self) -> Option<&str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_neighbouring_text_but_keeps_a_later_message_in_place() {
        let items = vec![
            serde_json::json!({"type": "text", "text": "<system-reminder>"}),
            serde_json::json!({"type": "text", "text": "the real request"}),
            serde_json::json!({"type": "tool_use", "id": "call-1", "name": "read"}),
            serde_json::json!({"type": "text", "text": "steer: stop waiting"}),
        ];

        let joined = join_consecutive_text(items, &["text"]);

        assert_eq!(joined.len(), 3);
        assert_eq!(joined[0]["text"], "<system-reminder>\n\nthe real request");
        assert_eq!(joined[1]["type"], "tool_use");
        // A text block after other content keeps its own position.
        assert_eq!(joined[2]["text"], "steer: stop waiting");
    }

    #[test]
    fn tool_detection_matches_protocol_tokens_not_substrings() {
        assert!(is_tool_payload(&serde_json::json!({
            "type": "future_tool_action"
        })));
        assert!(is_tool_payload(&serde_json::json!({
            "type": "function_call"
        })));
        assert!(is_tool_payload(&serde_json::json!({
            "type": "function",
            "function": {"name": "read", "arguments": "{}"}
        })));
        assert!(!is_tool_payload(&serde_json::json!({
            "type": "recall"
        })));
        assert!(!is_tool_payload(&serde_json::json!({
            "name": "report",
            "output": "ordinary data"
        })));
    }

    #[test]
    fn invalid_argument_json_falls_back_to_the_complete_payload() {
        let payload = serde_json::json!({
            "type": "function_call",
            "call_id": "call-1",
            "name": "broken",
            "arguments": "not json"
        });

        let message = ChatMessage::tool(None, payload.clone());

        assert_eq!(
            serde_json::from_str::<Value>(&message.content).unwrap(),
            payload
        );
    }

    #[test]
    fn opencode_state_drops_only_fixed_wrappers() {
        let payload = serde_json::json!({
            "type": "tool",
            "tool": "read",
            "callID": "call-1",
            "state": {
                "status": "completed",
                "time": {"start": 1, "end": 2},
                "input": {"path": "src/main.rs"},
                "output": "contents",
                "metadata": {"truncated": false}
            }
        });

        let message = ChatMessage::tool(None, payload);

        assert_eq!(
            serde_json::from_str::<Value>(&message.content).unwrap(),
            serde_json::json!({
                "tool": "read",
                "input": {"path": "src/main.rs"},
                "output": "contents",
                "metadata": {"truncated": false}
            })
        );
    }

    #[test]
    fn unfamiliar_wrapper_values_are_preserved() {
        let payload = serde_json::json!({
            "type": "tool_search_call",
            "call_id": "call-1",
            "execution": "server",
            "status": "pending",
            "arguments": {"query": "read"}
        });

        let message = ChatMessage::tool(None, payload);

        assert_eq!(
            serde_json::from_str::<Value>(&message.content).unwrap(),
            serde_json::json!({
                "execution": "server",
                "status": "pending",
                "arguments": {"query": "read"}
            })
        );
    }
}
