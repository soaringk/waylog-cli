mod formatter;

use crate::error::Result;
use crate::providers::base::ChatSession;
use std::path::Path;
use tokio::fs;

/// Generate markdown content from a chat session
pub fn generate_markdown(session: &ChatSession) -> String {
    let mut md = String::new();

    // Frontmatter
    md.push_str("---\n");
    md.push_str(&format!("provider: {}\n", session.provider));
    md.push_str(&format!("session_id: {}\n", session.session_id));
    md.push_str(&format!("project: {}\n", session.project_path.display()));
    md.push_str(&format!(
        "started_at: {}\n",
        session.started_at.to_rfc3339()
    ));
    md.push_str(&format!(
        "updated_at: {}\n",
        session.updated_at.to_rfc3339()
    ));
    md.push_str(&format!("message_count: {}\n", session.messages.len()));

    // Calculate total tokens if available
    let total_tokens: u32 = session
        .messages
        .iter()
        .filter_map(|m| m.metadata.tokens.as_ref())
        .map(|t| t.input + t.output)
        .sum();

    if total_tokens > 0 {
        md.push_str(&format!("total_tokens: {}\n", total_tokens));
    }

    md.push_str("---\n\n");

    // Title
    let title = formatter::extract_title(&session.messages);
    md.push_str(&format!("# {}\n\n", title));

    // Messages
    for message in &session.messages {
        md.push_str(&formatter::format_message(message));
        md.push_str("\n\n");
    }

    md
}

/// Create a new markdown file with the full session
pub async fn create_markdown_file(file_path: &Path, session: &ChatSession) -> Result<()> {
    let content = generate_markdown(session);
    fs::write(file_path, content).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{ChatMessage, MessageRole, TokenUsage};
    use chrono::Utc;
    use tempfile::TempDir;

    fn create_test_message(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            id: "1".to_string(),
            timestamp: Utc::now(),
            role,
            content: content.to_string(),
            metadata: Default::default(),
        }
    }

    fn create_test_session(messages: Vec<ChatMessage>) -> ChatSession {
        let now = Utc::now();
        ChatSession {
            session_id: "test-session".to_string(),
            provider: "claude".to_string(),
            project_path: std::env::temp_dir().join("test-project"),
            started_at: now,
            updated_at: now,
            messages,
        }
    }

    // format_datetime tests
    #[test]
    fn test_format_datetime() {
        use chrono::DateTime;
        let dt = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let formatted = formatter::format_datetime(&dt);
        assert_eq!(formatted, "2024-01-01 12:00:00 UTC");
    }

    // format_message tests
    #[test]
    fn test_format_message_user() {
        let message = create_test_message(MessageRole::User, "Hello, world!");
        let formatted = formatter::format_message(&message);
        assert!(formatted.contains("👤"));
        assert!(formatted.contains("User"));
        assert!(formatted.contains("Hello, world!"));
    }

    #[test]
    fn test_format_message_assistant() {
        let message = create_test_message(MessageRole::Assistant, "Hello! How can I help?");
        let formatted = formatter::format_message(&message);
        assert!(formatted.contains("🤖"));
        assert!(formatted.contains("Assistant"));
        assert!(formatted.contains("Hello! How can I help?"));
    }

    #[test]
    fn test_format_message_system() {
        let message = create_test_message(MessageRole::System, "System prompt");
        let formatted = formatter::format_message(&message);
        assert!(formatted.contains("⚙️"));
        assert!(formatted.contains("System"));
        assert!(formatted.contains("System prompt"));
    }

    #[test]
    fn test_format_message_with_tool_calls() {
        let mut message = create_test_message(MessageRole::Assistant, "I'll use some tools");
        message.metadata.tool_calls = vec!["read_file".to_string(), "write_file".to_string()];
        let formatted = formatter::format_message(&message);
        assert!(formatted.contains("**Tools Used:**"));
        assert!(formatted.contains("`read_file`"));
        assert!(formatted.contains("`write_file`"));
    }

    #[test]
    fn test_format_message_with_thoughts() {
        let mut message = create_test_message(MessageRole::Assistant, "Response");
        message.metadata.thoughts = vec!["Thought 1".to_string(), "Thought 2".to_string()];
        let formatted = formatter::format_message(&message);
        assert!(formatted.contains("<details>"));
        assert!(formatted.contains("<summary>💭 Thoughts</summary>"));
        assert!(formatted.contains("Thought 1"));
        assert!(formatted.contains("Thought 2"));
    }

    // generate_markdown tests
    #[test]
    fn test_generate_markdown_basic() {
        let messages = vec![
            create_test_message(MessageRole::User, "Hello"),
            create_test_message(MessageRole::Assistant, "Hi there!"),
        ];
        let session = create_test_session(messages);
        let md = generate_markdown(&session);

        assert!(md.contains("provider: claude"));
        assert!(md.contains("session_id: test-session"));
        assert!(md.contains("message_count: 2"));
        assert!(md.contains("# Hello"));
        assert!(md.contains("Hello"));
        assert!(md.contains("Hi there!"));
    }

    #[test]
    fn test_generate_markdown_with_tokens() {
        let mut message = create_test_message(MessageRole::User, "Test");
        message.metadata.tokens = Some(TokenUsage {
            input: 10,
            output: 20,
            cached: 5,
        });
        let session = create_test_session(vec![message]);
        let md = generate_markdown(&session);

        assert!(md.contains("total_tokens: 30")); // 10 + 20
    }

    #[test]
    fn test_generate_markdown_without_tokens() {
        let messages = vec![create_test_message(MessageRole::User, "Test")];
        let session = create_test_session(messages);
        let md = generate_markdown(&session);

        assert!(!md.contains("total_tokens"));
    }

    #[test]
    fn test_generate_markdown_empty_messages() {
        let session = create_test_session(vec![]);
        let md = generate_markdown(&session);

        assert!(md.contains("message_count: 0"));
        assert!(md.contains("# Untitled Session"));
    }

    #[test]
    fn test_generate_markdown_frontmatter_format() {
        let messages = vec![create_test_message(MessageRole::User, "Test")];
        let session = create_test_session(messages);
        let md = generate_markdown(&session);

        // Check frontmatter format
        assert!(md.starts_with("---\n"));
        assert!(md.contains("---\n\n")); // Frontmatter end
        assert!(md.contains("started_at:"));
        assert!(md.contains("updated_at:"));
    }

    // Async function tests
    #[tokio::test]
    async fn test_create_markdown_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.md");

        let messages = vec![
            create_test_message(MessageRole::User, "Hello"),
            create_test_message(MessageRole::Assistant, "Hi!"),
        ];
        let session = create_test_session(messages);

        create_markdown_file(&file_path, &session).await.unwrap();

        assert!(file_path.exists());
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("Hello"));
        assert!(content.contains("Hi!"));
    }
}
