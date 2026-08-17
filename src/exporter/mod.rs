pub mod json;
pub mod markdown;

use crate::error::Result;
use crate::providers::base::{AssistantOutput, ChatMessage, ChatSession, MessageRole};
use std::path::Path;
use tokio::fs;

/// Representation one exported session is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    /// Markdown for people to read.
    #[default]
    Markdown,
    /// Structured records for programs to consume.
    Json,
}

impl ExportFormat {
    /// File extension identifying exports written in this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
}

/// Shape of the exported session, independent of which sessions are selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub include_tool_calls: bool,
}

/// Sync state recovered from an existing export.
#[derive(Debug, Clone, Default)]
pub struct ExportHeader {
    pub session_id: Option<String>,
    pub provider: Option<String>,
    pub message_count: Option<usize>,
    pub include_tool_calls: Option<bool>,
}

/// Read the sync state an existing export records about itself.
pub async fn read_header(path: &Path, format: ExportFormat) -> Result<ExportHeader> {
    match format {
        ExportFormat::Markdown => markdown::parse_frontmatter(path).await,
        ExportFormat::Json => json::parse_header(path).await,
    }
}

/// Write one session in the requested representation.
pub async fn write_session(
    path: &Path,
    session: &ChatSession,
    options: ExportOptions,
) -> Result<()> {
    let content = match options.format {
        ExportFormat::Markdown => markdown::generate(session, options.include_tool_calls),
        ExportFormat::Json => json::generate(session, options.include_tool_calls)?,
    };
    fs::write(path, content).await?;
    Ok(())
}

/// The project a session belongs to, absent when the provider recorded none.
pub(crate) fn project(session: &ChatSession) -> Option<std::borrow::Cow<'_, str>> {
    (!session.project_path.as_os_str().is_empty()).then(|| session.project_path.to_string_lossy())
}

/// Count the records an export represents. Tool records are opt-in, so they only
/// count when they are written.
pub fn message_count(session: &ChatSession, include_tool_calls: bool) -> usize {
    exported_messages(session, include_tool_calls).count()
}

/// The records an export contains, in the order the provider recorded them.
fn exported_messages(
    session: &ChatSession,
    include_tool_calls: bool,
) -> impl Iterator<Item = &ChatMessage> {
    session.messages.iter().filter(move |message| {
        include_tool_calls || message.role != MessageRole::Assistant(AssistantOutput::Tool)
    })
}

/// One step of a conversation. Both formats render this same shape.
pub(crate) enum Entry<'a> {
    /// One record the user or the provider contributed, kept where it was recorded.
    Standalone(&'a ChatMessage),
    /// A run of model output. This is the only turn boundary a provider makes
    /// observable: everything until the conversation returns to the user.
    AssistantTurn(Vec<Part<'a>>),
}

/// One step of model output. A tool request and its result are one step, so both formats
/// present a call together with what it returned.
pub(crate) type Part<'a> = Vec<&'a ChatMessage>;

/// Split a session into entries in recorded order. Model output groups into a turn;
/// every other record stands where it is, so a later user input stays at the point it
/// changed the assistant's course.
pub(crate) fn entries(session: &ChatSession, include_tool_calls: bool) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    for message in exported_messages(session, include_tool_calls) {
        match (&message.role, entries.last_mut()) {
            (MessageRole::Assistant(_), Some(Entry::AssistantTurn(parts))) => {
                push_output(parts, message)
            }
            (MessageRole::Assistant(_), _) => {
                entries.push(Entry::AssistantTurn(vec![vec![message]]))
            }
            _ => entries.push(Entry::Standalone(message)),
        }
    }
    entries
}

/// Add one record of model output to the turn being built. A tool result joins the call
/// that carries the same id; without a matching id a record stands on its own, in the
/// order it was recorded, because nothing links it to a call.
fn push_output<'a>(parts: &mut Vec<Part<'a>>, message: &'a ChatMessage) {
    if message.role == MessageRole::Assistant(AssistantOutput::Tool) {
        if let Some(call_id) = message.metadata.tool_call_id.as_deref() {
            let call = parts.iter_mut().find(|part| {
                part.len() == 1
                    && part[0].role == MessageRole::Assistant(AssistantOutput::Tool)
                    && part[0].metadata.tool_call_id.as_deref() == Some(call_id)
            });
            if let Some(call) = call {
                call.push(message);
                return;
            }
        }
    }
    parts.push(vec![message]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::MessageMetadata;

    fn message(role: MessageRole) -> ChatMessage {
        ChatMessage {
            timestamp: None,
            role,
            content: "content".to_string(),
            metadata: MessageMetadata::default(),
        }
    }

    fn session(roles: impl IntoIterator<Item = MessageRole>) -> ChatSession {
        ChatSession {
            session_id: "session".to_string(),
            provider: "provider".to_string(),
            project_path: std::path::PathBuf::new(),
            started_at: None,
            updated_at: None,
            messages: roles.into_iter().map(message).collect(),
        }
    }

    fn tool(call_id: Option<&str>, content: &str) -> ChatMessage {
        let mut message = message(MessageRole::Assistant(AssistantOutput::Tool));
        message.content = content.to_string();
        message.metadata.tool_call_id = call_id.map(str::to_string);
        message
    }

    fn steps(session: &ChatSession) -> Vec<Vec<&str>> {
        entries(session, true)
            .iter()
            .flat_map(|entry| match entry {
                Entry::Standalone(_) => Vec::new(),
                Entry::AssistantTurn(parts) => parts
                    .iter()
                    .map(|part| part.iter().map(|record| record.content.as_str()).collect())
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn a_tool_result_joins_the_call_that_shares_its_id() {
        let mut session = session([MessageRole::User]);
        session.messages.extend([
            tool(Some("a"), "call a"),
            tool(Some("b"), "call b"),
            tool(Some("a"), "result a"),
            tool(Some("b"), "result b"),
        ]);

        // Providers batch parallel calls, so a result is matched by id rather than by
        // sitting next to its call.
        assert_eq!(
            steps(&session),
            vec![vec!["call a", "result a"], vec!["call b", "result b"],]
        );
    }

    #[test]
    fn tool_records_without_a_matching_id_stay_in_recorded_order() {
        let mut session = session([MessageRole::User]);
        session.messages.extend([
            tool(None, "first"),
            tool(None, "second"),
            tool(Some("a"), "call a"),
            tool(Some("b"), "unanswered call b"),
            tool(Some("a"), "result a"),
            tool(Some("a"), "late record reusing id a"),
        ]);

        // Nothing links an id-less record to a call, an unanswered call has no result, and
        // an id already answered does not absorb a third record.
        assert_eq!(
            steps(&session),
            vec![
                vec!["first"],
                vec!["second"],
                vec!["call a", "result a"],
                vec!["unanswered call b"],
                vec!["late record reusing id a"],
            ]
        );
    }

    #[test]
    fn each_format_owns_one_extension() {
        assert_eq!(ExportFormat::default(), ExportFormat::Markdown);
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Json.extension(), "json");
    }

    #[test]
    fn one_run_of_model_output_becomes_one_turn() {
        let session = session([
            MessageRole::User,
            MessageRole::Assistant(AssistantOutput::Reasoning),
            MessageRole::Assistant(AssistantOutput::Tool),
            MessageRole::Assistant(AssistantOutput::Message),
            MessageRole::User,
            MessageRole::Assistant(AssistantOutput::Message),
            MessageRole::System,
        ]);

        let shape = entries(&session, true)
            .iter()
            .map(|entry| match entry {
                Entry::Standalone(message) => format!("{:?}", message.role),
                Entry::AssistantTurn(parts) => format!("turn of {}", parts.len()),
            })
            .collect::<Vec<_>>();

        assert_eq!(shape, ["User", "turn of 3", "User", "turn of 1", "System"]);
    }

    #[test]
    fn excluded_tool_records_do_not_split_a_turn() {
        let session = session([
            MessageRole::Assistant(AssistantOutput::Reasoning),
            MessageRole::Assistant(AssistantOutput::Tool),
            MessageRole::Assistant(AssistantOutput::Message),
        ]);

        assert_eq!(message_count(&session, false), 2);
        match entries(&session, false).as_slice() {
            [Entry::AssistantTurn(parts)] => assert_eq!(parts.len(), 2),
            other => panic!("expected one turn, got {} entries", other.len()),
        }
    }
}
