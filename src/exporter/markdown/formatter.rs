use crate::exporter::Entry;
use crate::providers::base::{AssistantOutput, ChatMessage, MessageRole};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub(crate) fn format_entries(entries: &[Entry<'_>]) -> String {
    let mut blocks = Vec::new();
    for entry in entries {
        match entry {
            Entry::Standalone(message) => blocks.push(format_part(&[message], 2)),
            Entry::AssistantTurn(parts) => {
                blocks.push(heading(2, "🤖 Assistant", parts[0]));
                blocks.extend(
                    group_tool_exchanges(parts)
                        .iter()
                        .map(|part| format_part(part, 3)),
                );
            }
        }
    }

    if blocks.is_empty() {
        return String::new();
    }
    format!("{}\n", blocks.join("\n\n"))
}

/// Render a tool request and its result as one block, which reads better than two.
fn group_tool_exchanges<'a>(parts: &[&'a ChatMessage]) -> Vec<Vec<&'a ChatMessage>> {
    let mut groups = Vec::<Vec<&ChatMessage>>::new();
    let mut exchanges = HashMap::<&str, usize>::new();

    for part in parts {
        if part.role == MessageRole::Assistant(AssistantOutput::Tool) {
            if let Some(call_id) = part.metadata.tool_call_id.as_deref() {
                if let Some(index) = exchanges.get(call_id) {
                    groups[*index].push(part);
                    continue;
                }
                exchanges.insert(call_id, groups.len());
            }
        }
        groups.push(vec![part]);
    }

    groups
}

fn heading(level: usize, label: &str, message: &ChatMessage) -> String {
    format!(
        "{} {} ({})",
        "#".repeat(level),
        label,
        format_datetime(message.timestamp.as_ref())
    )
}

fn label(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "👤 User",
        MessageRole::System => "⚙️ System",
        MessageRole::Assistant(AssistantOutput::Reasoning) => "🧠 Reasoning",
        MessageRole::Assistant(AssistantOutput::Message) => "💬 Message",
        MessageRole::Assistant(AssistantOutput::Tool) => "🛠️ Tool",
    }
}

/// Format one record, or one tool exchange, at the given heading level. Provider text is
/// reproduced exactly, so block spacing is only ever added around it.
fn format_part(messages: &[&ChatMessage], level: usize) -> String {
    let message = messages[0];
    let mut md = heading(level, label(message.role), message);
    md.push_str("\n\n");

    if message.role == MessageRole::Assistant(AssistantOutput::Tool) {
        let fence = tool_fence(messages);
        md.push_str(&fence);
        for (index, message) in messages.iter().enumerate() {
            md.push_str(if index == 0 { "\n" } else { "\n\n" });
            md.push_str(&message.content);
        }
        md.push('\n');
        md.push_str(&fence);
    } else {
        md.push_str(&message.content);
    }

    if !message.metadata.tool_calls.is_empty() {
        md.push_str("\n\n**Tools Used:**");
        for tool in &message.metadata.tool_calls {
            md.push_str(&format!("\n- `{tool}`"));
        }
    }

    if !message.metadata.thoughts.is_empty() {
        md.push_str("\n\n<details>\n<summary>💭 Thoughts</summary>\n");
        for thought in &message.metadata.thoughts {
            md.push_str(&format!("\n- {thought}"));
        }
        md.push_str("\n\n</details>");
    }

    md
}

fn tool_fence(messages: &[&ChatMessage]) -> String {
    let longest = messages
        .iter()
        .flat_map(|message| message.content.split(|character| character != '`'))
        .map(str::len)
        .max()
        .unwrap_or_default();
    "`".repeat((longest + 1).max(3))
}

/// Extract a title from the first user message
pub(crate) fn extract_title(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .find(|message| message.role == MessageRole::User)
        .map(|message| {
            // Take first line or first 60 characters (char-boundary safe)
            let first_line = message.content.lines().next().unwrap_or("Untitled Session");
            let char_count = first_line.chars().count();
            if char_count > 60 {
                let truncated: String = first_line.chars().take(60).collect();
                format!("{}...", truncated)
            } else {
                first_line.to_string()
            }
        })
        .unwrap_or_else(|| "Untitled Session".to_string())
}

/// Format datetime in a human-readable way
pub(crate) fn format_datetime(timestamp: Option<&DateTime<Utc>>) -> String {
    timestamp
        .map(|value| value.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "null".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exporter::entries;
    use crate::providers::base::{ChatSession, MessageMetadata};

    fn create_test_message(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
            timestamp: Some(Utc::now()),
            metadata: MessageMetadata::default(),
        }
    }

    #[test]
    fn nests_model_output_under_one_turn_in_recorded_order() {
        let tool = MessageRole::Assistant(AssistantOutput::Tool);
        let mut request = create_test_message(tool, r#"{"name":"read"}"#);
        request.metadata.tool_call_id = Some("call-1".to_string());
        let mut result = create_test_message(tool, r#"{"output":"contents"}"#);
        result.metadata.tool_call_id = Some("call-1".to_string());
        let session = ChatSession {
            session_id: "session".to_string(),
            provider: "provider".to_string(),
            project_path: std::path::PathBuf::new(),
            started_at: None,
            updated_at: None,
            messages: vec![
                create_test_message(MessageRole::User, "Fix the parser"),
                create_test_message(
                    MessageRole::Assistant(AssistantOutput::Reasoning),
                    "Consider the two formats",
                ),
                request,
                result,
                create_test_message(MessageRole::Assistant(AssistantOutput::Message), "Done"),
                create_test_message(MessageRole::User, "And next?"),
                create_test_message(
                    MessageRole::Assistant(AssistantOutput::Message),
                    "Nothing left",
                ),
            ],
        };

        let markdown = format_entries(&entries(&session, true));

        let headings = markdown
            .lines()
            .filter(|line| line.starts_with('#'))
            .map(|line| line.split_once(" (").map_or(line, |split| split.0))
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            vec![
                "## 👤 User",
                "## 🤖 Assistant",
                "### 🧠 Reasoning",
                "### 🛠️ Tool",
                "### 💬 Message",
                "## 👤 User",
                "## 🤖 Assistant",
                "### 💬 Message",
            ]
        );
        assert_eq!(markdown.matches(r#"{"output":"contents"}"#).count(), 1);
    }

    #[test]
    fn formats_timestamps_and_marks_absent_ones_null() {
        let timestamp = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(format_datetime(Some(&timestamp)), "2024-01-01 12:00:00 UTC");
        assert_eq!(format_datetime(None), "null");
    }

    #[test]
    fn extracts_first_line_of_first_user_message() {
        let messages = vec![
            create_test_message(MessageRole::System, "System init"),
            create_test_message(MessageRole::User, "First user message\nMore detail"),
            create_test_message(MessageRole::User, "Second user message"),
        ];
        assert_eq!(extract_title(&messages), "First user message");
    }

    #[test]
    fn truncates_unicode_titles_at_character_boundaries() {
        let exactly_sixty = "界".repeat(60);
        let long = "界".repeat(61);

        assert_eq!(
            extract_title(&[create_test_message(MessageRole::User, &exactly_sixty)]),
            exactly_sixty
        );
        assert_eq!(
            extract_title(&[create_test_message(MessageRole::User, &long)]),
            format!("{}...", "界".repeat(60))
        );
    }

    #[test]
    fn uses_default_title_without_user_content() {
        for messages in [
            Vec::new(),
            vec![create_test_message(
                MessageRole::Assistant(AssistantOutput::Message),
                "Assistant response",
            )],
            vec![create_test_message(MessageRole::User, "")],
        ] {
            assert_eq!(extract_title(&messages), "Untitled Session");
        }
    }
}
