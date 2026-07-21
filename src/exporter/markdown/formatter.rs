use crate::providers::base::{ChatMessage, MessageRole};
use chrono::{DateTime, Utc};

/// Format a single message
pub(crate) fn format_message(message: &ChatMessage) -> String {
    let mut md = String::new();

    // Header with role and timestamp
    let role_emoji = match message.role {
        MessageRole::User => "👤",
        MessageRole::Assistant => "🤖",
        MessageRole::System => "⚙️",
    };

    let role_name = match message.role {
        MessageRole::User => "User",
        MessageRole::Assistant => "Assistant",
        MessageRole::System => "System",
    };

    md.push_str(&format!(
        "## {} {} ({})\n\n",
        role_emoji,
        role_name,
        format_datetime(&message.timestamp)
    ));

    // Content
    md.push_str(&message.content);
    md.push('\n');

    // Tool calls (Claude Code)
    if !message.metadata.tool_calls.is_empty() {
        md.push_str("\n**Tools Used:**\n");
        for tool in &message.metadata.tool_calls {
            md.push_str(&format!("- `{}`\n", tool));
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
pub(crate) fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
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
            timestamp: Utc::now(),
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
