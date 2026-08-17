use crate::error::Result;
use crate::exporter::{entries, project, Entry, ExportHeader, Part};
use crate::providers::base::{AssistantOutput, ChatSession, MessageRole};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

/// Generate the structured representation of a chat session.
pub fn generate(session: &ChatSession, include_tool_calls: bool) -> Result<String> {
    let entries = entries(session, include_tool_calls);
    let document = SessionRecord {
        provider: &session.provider,
        session_id: &session.session_id,
        project: project(session),
        started_at: timestamp(session.started_at.as_ref()),
        updated_at: timestamp(session.updated_at.as_ref()),
        message_count: super::message_count(session, include_tool_calls),
        include_tool_calls,
        turns: entries.iter().map(turn_record).collect(),
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}

/// Read the sync state a structured export records about itself.
pub async fn parse_header(path: &Path) -> Result<ExportHeader> {
    let record: HeaderRecord = serde_json::from_slice(&fs::read(path).await?)?;
    Ok(ExportHeader {
        session_id: record.session_id,
        provider: record.provider,
        message_count: record.message_count,
        include_tool_calls: record.include_tool_calls,
    })
}

fn turn_record<'a>(entry: &Entry<'a>) -> TurnRecord<'a> {
    match entry {
        Entry::Standalone(message) => TurnRecord {
            role: match message.role {
                MessageRole::User => "user",
                _ => "system",
            },
            timestamp: timestamp(message.timestamp.as_ref()),
            content: Some(&message.content),
            parts: Vec::new(),
        },
        Entry::AssistantTurn(parts) => TurnRecord {
            role: "assistant",
            timestamp: timestamp(parts[0][0].timestamp.as_ref()),
            content: None,
            parts: parts.iter().map(|part| part_record(part)).collect(),
        },
    }
}

/// Record one step of model output. A tool call that was matched to its result carries
/// that result alongside it; every other step is a single record.
fn part_record<'a>(records: &Part<'a>) -> PartRecord<'a> {
    let part = records[0];
    PartRecord {
        kind: match part.role {
            MessageRole::Assistant(AssistantOutput::Reasoning) => "reasoning",
            MessageRole::Assistant(AssistantOutput::Tool) => "tool",
            _ => "message",
        },
        timestamp: timestamp(part.timestamp.as_ref()),
        content: &part.content,
        result: records.get(1).map(|result| result.content.as_str()),
        tool_call_id: part.metadata.tool_call_id.as_deref(),
        model: part.metadata.model.as_deref(),
        tokens: part.metadata.tokens.as_ref().map(|tokens| TokenRecord {
            input: tokens.input,
            output: tokens.output,
            cached: tokens.cached,
        }),
        tool_calls: &part.metadata.tool_calls,
        thoughts: &part.metadata.thoughts,
    }
}

/// Absent source timestamps stay null instead of being replaced.
fn timestamp(value: Option<&DateTime<Utc>>) -> Option<String> {
    value.map(DateTime::to_rfc3339)
}

#[derive(Serialize)]
struct SessionRecord<'a> {
    provider: &'a str,
    session_id: &'a str,
    project: Option<std::borrow::Cow<'a, str>>,
    started_at: Option<String>,
    updated_at: Option<String>,
    /// Number of records the export contains, counting every assistant part.
    message_count: usize,
    include_tool_calls: bool,
    turns: Vec<TurnRecord<'a>>,
}

#[derive(Serialize)]
struct TurnRecord<'a> {
    role: &'static str,
    timestamp: Option<String>,
    /// Present on user and system turns, which carry one record.
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    /// Present on assistant turns, holding every step the model produced.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    parts: Vec<PartRecord<'a>>,
}

#[derive(Serialize)]
struct PartRecord<'a> {
    kind: &'static str,
    timestamp: Option<String>,
    /// This step's own record: for a tool step, the call.
    content: &'a str,
    /// What a tool call returned, present when an id matched a result to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<TokenRecord>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    tool_calls: &'a [String],
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    thoughts: &'a [String],
}

#[derive(Serialize)]
struct TokenRecord {
    input: u32,
    output: u32,
    cached: u32,
}

/// Only the fields sync state needs; the turns are skipped.
#[derive(Deserialize)]
struct HeaderRecord {
    provider: Option<String>,
    session_id: Option<String>,
    message_count: Option<usize>,
    include_tool_calls: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{ChatMessage, MessageMetadata, TokenUsage};
    use serde_json::Value;
    use tempfile::TempDir;

    fn message(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            timestamp: DateTime::parse_from_rfc3339("2026-08-14T01:02:03Z")
                .ok()
                .map(|value| value.with_timezone(&Utc)),
            role,
            content: content.to_string(),
            metadata: MessageMetadata::default(),
        }
    }

    fn session(messages: Vec<ChatMessage>) -> ChatSession {
        ChatSession {
            session_id: "session-1".to_string(),
            provider: "codex".to_string(),
            project_path: std::path::PathBuf::from("/project"),
            started_at: None,
            updated_at: None,
            messages,
        }
    }

    #[test]
    fn nests_model_output_under_one_assistant_turn() {
        let mut tool = message(
            MessageRole::Assistant(AssistantOutput::Tool),
            r#"{"name":"exec"}"#,
        );
        tool.metadata.tool_call_id = Some("call-1".to_string());
        let mut answer = message(MessageRole::Assistant(AssistantOutput::Message), "done");
        answer.metadata.model = Some("test-model".to_string());
        answer.metadata.tokens = Some(TokenUsage {
            input: 3,
            output: 4,
            cached: 5,
        });
        let session = session(vec![
            message(MessageRole::User, "fix it"),
            message(
                MessageRole::Assistant(AssistantOutput::Reasoning),
                "weighing options",
            ),
            tool,
            answer,
            message(MessageRole::System, "injected context"),
        ]);

        let document: Value = serde_json::from_str(&generate(&session, true).unwrap()).unwrap();

        assert_eq!(document["message_count"], 5);
        assert_eq!(document["include_tool_calls"], true);
        assert_eq!(document["project"], "/project");
        assert_eq!(document["started_at"], Value::Null);

        let turns = document["turns"].as_array().unwrap();
        let roles = turns
            .iter()
            .map(|turn| turn["role"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(roles, ["user", "assistant", "system"]);

        // A user turn carries content and no parts.
        assert_eq!(turns[0]["content"], "fix it");
        assert!(turns[0].get("parts").is_none());
        assert_eq!(turns[0]["timestamp"], "2026-08-14T01:02:03+00:00");

        // An assistant turn carries parts and no content of its own.
        assert!(turns[1].get("content").is_none());
        let kinds = turns[1]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part["kind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["reasoning", "tool", "message"]);
        assert_eq!(turns[1]["parts"][1]["tool_call_id"], "call-1");
        assert_eq!(turns[1]["parts"][2]["model"], "test-model");
        assert_eq!(turns[1]["parts"][2]["tokens"]["cached"], 5);
        // Absent metadata stays out of the record instead of appearing as null.
        assert!(turns[1]["parts"][0].get("model").is_none());
        assert!(turns[1]["parts"][0].get("tool_calls").is_none());
    }

    #[test]
    fn tool_records_are_opt_in_and_never_split_a_turn() {
        let mut tool = message(MessageRole::Assistant(AssistantOutput::Tool), "{}");
        tool.metadata.tool_call_id = Some("call-1".to_string());
        let session = session(vec![
            message(MessageRole::User, "hello"),
            message(
                MessageRole::Assistant(AssistantOutput::Reasoning),
                "thinking",
            ),
            tool,
            message(MessageRole::Assistant(AssistantOutput::Message), "hi"),
        ]);

        let document: Value = serde_json::from_str(&generate(&session, false).unwrap()).unwrap();

        assert_eq!(document["message_count"], 3);
        assert_eq!(document["include_tool_calls"], false);
        let turns = document["turns"].as_array().unwrap();
        assert_eq!(turns.len(), 2);
        let kinds = turns[1]["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part["kind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(kinds, ["reasoning", "message"]);
    }

    #[test]
    fn an_unrecorded_project_stays_null() {
        let mut session = session(vec![message(MessageRole::User, "hello")]);
        session.project_path = std::path::PathBuf::new();

        let document: Value = serde_json::from_str(&generate(&session, false).unwrap()).unwrap();

        assert_eq!(document["project"], Value::Null);
    }

    #[tokio::test]
    async fn header_reads_sync_state_without_the_turns() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("session.json");
        let session = session(vec![message(MessageRole::User, "hello")]);
        tokio::fs::write(&path, generate(&session, false).unwrap())
            .await
            .unwrap();

        let header = parse_header(&path).await.unwrap();

        assert_eq!(header.provider.as_deref(), Some("codex"));
        assert_eq!(header.session_id.as_deref(), Some("session-1"));
        assert_eq!(header.message_count, Some(1));
        assert_eq!(header.include_tool_calls, Some(false));
    }
}
