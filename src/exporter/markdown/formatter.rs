use crate::providers::base::{ChatMessage, MessageRole};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

pub(crate) fn format_messages<'a>(messages: impl IntoIterator<Item = &'a ChatMessage>) -> String {
    let mut groups = Vec::<Vec<&ChatMessage>>::new();
    let mut tool_groups = HashMap::<&str, usize>::new();

    for message in messages {
        if message.role == MessageRole::Tool {
            if let Some(call_id) = message.metadata.tool_call_id.as_deref() {
                if let Some(index) = tool_groups.get(call_id) {
                    groups[*index].push(message);
                    continue;
                }
                tool_groups.insert(call_id, groups.len());
            }
        }
        groups.push(vec![message]);
    }

    let mut md = String::new();
    for group in groups {
        md.push_str(&format_group(&group));
        md.push_str("\n\n");
    }
    md
}

/// Format one message or one grouped tool exchange.
fn format_group(messages: &[&ChatMessage]) -> String {
    let message = messages[0];
    let mut md = String::new();

    // Header with role and timestamp
    let (role_emoji, role_name) = match message.role {
        MessageRole::User => ("👤", "User"),
        MessageRole::Assistant => ("🤖", "Assistant"),
        MessageRole::System => ("⚙️", "System"),
        MessageRole::Tool => ("🛠️", "Tool"),
    };

    md.push_str(&format!(
        "## {} {} ({})\n\n",
        role_emoji,
        role_name,
        format_datetime(message.timestamp.as_ref())
    ));

    // Content
    if message.role == MessageRole::Tool {
        let fence = tool_fence(messages);
        md.push_str(&fence);
        md.push('\n');
        for (index, message) in messages.iter().enumerate() {
            if index > 0 {
                md.push_str("\n\n");
            }
            md.push_str(&message.content);
        }
        md.push('\n');
        md.push_str(&fence);
    } else {
        md.push_str(&message.content);
    }
    md.push('\n');

    if !message.metadata.tool_calls.is_empty() {
        md.push_str("\n**Tools Used:**\n");
        for tool in &message.metadata.tool_calls {
            md.push_str(&format!("- `{tool}`\n"));
        }
    }

    // Thoughts (Gemini)
    if !message.metadata.thoughts.is_empty() {
        md.push_str("\n<details>\n<summary>💭 Thoughts</summary>\n\n");
        for thought in &message.metadata.thoughts {
            md.push_str(&format!("- {}\n", thought));
        }
        md.push_str("\n</details>\n");
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
        .find(|m| matches!(m.role, MessageRole::User))
        .map(|m| {
            // Take first line or first 60 characters (char-boundary safe)
            let first_line = m.content.lines().next().unwrap_or("Untitled Session");
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
    use crate::providers::base::MessageMetadata;

    fn create_test_message(content: &str, role: MessageRole) -> ChatMessage {
        ChatMessage {
            id: "test-id".to_string(),
            role,
            content: content.to_string(),
            timestamp: Some(Utc::now()),
            metadata: MessageMetadata::default(),
        }
    }

    #[test]
    fn extracts_first_line_of_first_user_message() {
        let messages = vec![
            create_test_message("System init", MessageRole::System),
            create_test_message("First user message\nMore detail", MessageRole::User),
            create_test_message("Second user message", MessageRole::User),
        ];
        assert_eq!(extract_title(&messages), "First user message");
    }

    #[test]
    fn truncates_unicode_titles_at_character_boundaries() {
        let exactly_sixty = "界".repeat(60);
        let long = "界".repeat(61);

        assert_eq!(
            extract_title(&[create_test_message(&exactly_sixty, MessageRole::User)]),
            exactly_sixty
        );
        assert_eq!(
            extract_title(&[create_test_message(&long, MessageRole::User)]),
            format!("{}...", "界".repeat(60))
        );
    }

    #[test]
    fn uses_default_title_without_user_content() {
        for messages in [
            Vec::new(),
            vec![create_test_message(
                "Assistant response",
                MessageRole::Assistant,
            )],
            vec![create_test_message("", MessageRole::User)],
        ] {
            assert_eq!(extract_title(&messages), "Untitled Session");
        }
    }
}
