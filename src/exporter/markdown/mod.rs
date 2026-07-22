mod formatter;

use crate::error::Result;
use crate::providers::base::{ChatMessage, ChatSession, MessageRole};
use std::path::Path;
use tokio::fs;

/// Generate markdown content from a chat session
pub fn generate_markdown(session: &ChatSession, include_tool_calls: bool) -> String {
    let mut md = String::new();

    // Frontmatter
    md.push_str("---\n");
    md.push_str(&format!("provider: {}\n", session.provider));
    md.push_str(&format!("session_id: {}\n", session.session_id));
    md.push_str(&format!("project: {}\n", session.project_path.display()));
    md.push_str(&format!(
        "started_at: {}\n",
        frontmatter_timestamp(session.started_at.as_ref())
    ));
    md.push_str(&format!(
        "updated_at: {}\n",
        frontmatter_timestamp(session.updated_at.as_ref())
    ));
    md.push_str(&format!(
        "message_count: {}\n",
        message_count(session, include_tool_calls)
    ));
    if include_tool_calls {
        md.push_str("include_tool_calls: true\n");
    }

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
    md.push_str(&formatter::format_messages(messages(
        session,
        include_tool_calls,
    )));

    md
}

fn frontmatter_timestamp(timestamp: Option<&chrono::DateTime<chrono::Utc>>) -> String {
    timestamp
        .map(chrono::DateTime::to_rfc3339)
        .unwrap_or_else(|| "null".to_string())
}

pub fn message_count(session: &ChatSession, include_tool_calls: bool) -> usize {
    messages(session, include_tool_calls).count()
}

fn messages(session: &ChatSession, include_tool_calls: bool) -> impl Iterator<Item = &ChatMessage> {
    session
        .messages
        .iter()
        .filter(move |message| include_tool_calls || message.role != MessageRole::Tool)
}

/// Create a new markdown file with the full session
pub async fn create_markdown_file(
    file_path: &Path,
    session: &ChatSession,
    include_tool_calls: bool,
) -> Result<()> {
    let content = generate_markdown(session, include_tool_calls);
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
            timestamp: Some(Utc::now()),
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
            started_at: Some(now),
            updated_at: Some(now),
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
        assert_eq!(
            formatter::format_datetime(Some(&dt)),
            "2024-01-01 12:00:00 UTC"
        );
        assert_eq!(formatter::format_datetime(None), "null");
    }

    // format_message tests
    #[test]
    fn test_format_message_user() {
        let message = create_test_message(MessageRole::User, "Hello, world!");
        let formatted = formatter::format_messages([&message]);
        assert!(formatted.contains("👤"));
        assert!(formatted.contains("User"));
        assert!(formatted.contains("Hello, world!"));
    }

    #[test]
    fn test_format_message_assistant() {
        let message = create_test_message(MessageRole::Assistant, "Hello! How can I help?");
        let formatted = formatter::format_messages([&message]);
        assert!(formatted.contains("🤖"));
        assert!(formatted.contains("Assistant"));
        assert!(formatted.contains("Hello! How can I help?"));
    }

    #[test]
    fn test_format_message_system() {
        let message = create_test_message(MessageRole::System, "System prompt");
        let formatted = formatter::format_messages([&message]);
        assert!(formatted.contains("⚙️"));
        assert!(formatted.contains("System"));
        assert!(formatted.contains("System prompt"));
    }

    #[test]
    fn test_format_message_with_tool_names() {
        let mut message = create_test_message(MessageRole::Assistant, "I'll use a tool");
        message.metadata.tool_calls = vec!["read_file".to_string()];

        let formatted = formatter::format_messages([&message]);

        assert!(formatted.contains("**Tools Used:**"));
        assert!(formatted.contains("`read_file`"));
    }

    #[test]
    fn test_format_message_with_thoughts() {
        let mut message = create_test_message(MessageRole::Assistant, "Response");
        message.metadata.thoughts = vec!["Thought 1".to_string(), "Thought 2".to_string()];
        let formatted = formatter::format_messages([&message]);
        assert!(formatted.contains("<details>"));
        assert!(formatted.contains("<summary>💭 Thoughts</summary>"));
        assert!(formatted.contains("Thought 1"));
        assert!(formatted.contains("Thought 2"));
    }

    #[test]
    fn tool_exchanges_are_opt_in_and_grouped_by_call_id() {
        let assistant = create_test_message(MessageRole::Assistant, "Checking the file");
        let mut request_a = create_test_message(
            MessageRole::Tool,
            r#"{"type":"function_call","call_id":"a"}"#,
        );
        request_a.metadata.tool_call_id = Some("a".to_string());
        let mut request_b = create_test_message(
            MessageRole::Tool,
            r#"{"type":"function_call","call_id":"b"}"#,
        );
        request_b.metadata.tool_call_id = Some("b".to_string());
        let mut response_b = create_test_message(
            MessageRole::Tool,
            r#"{"type":"function_call_output","call_id":"b"}"#,
        );
        response_b.metadata.tool_call_id = Some("b".to_string());
        let mut response_a = create_test_message(
            MessageRole::Tool,
            r#"{"type":"function_call_output","call_id":"a","output":"```"}"#,
        );
        response_a.metadata.tool_call_id = Some("a".to_string());
        let session = create_test_session(vec![
            assistant, request_a, request_b, response_b, response_a,
        ]);

        let default_markdown = generate_markdown(&session, false);
        assert!(default_markdown.contains("message_count: 1"));
        assert!(!default_markdown.contains("## 🛠️ Tool"));

        let markdown = generate_markdown(&session, true);
        assert!(markdown.contains("message_count: 5"));
        assert!(markdown.contains("include_tool_calls: true"));
        assert!(!markdown.contains("```json"));
        let groups = markdown.split("## 🛠️ Tool").skip(1).collect::<Vec<_>>();
        assert_eq!(groups.len(), 2);
        assert!(groups[0].contains(r#""call_id":"a""#));
        assert!(groups[0].contains("function_call_output"));
        assert!(groups[0].contains("````\n"));
        assert!(groups[1].contains(r#""call_id":"b""#));
        assert!(groups[1].contains("function_call_output"));
    }

    // generate_markdown tests
    #[test]
    fn test_generate_markdown_basic() {
        let messages = vec![
            create_test_message(MessageRole::User, "Hello"),
            create_test_message(MessageRole::Assistant, "Hi there!"),
        ];
        let session = create_test_session(messages);
        let md = generate_markdown(&session, false);

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
        let md = generate_markdown(&session, false);

        assert!(md.contains("total_tokens: 30")); // 10 + 20
    }

    #[test]
    fn test_generate_markdown_without_tokens() {
        let messages = vec![create_test_message(MessageRole::User, "Test")];
        let session = create_test_session(messages);
        let md = generate_markdown(&session, false);

        assert!(!md.contains("total_tokens"));
    }

    #[test]
    fn test_generate_markdown_empty_messages() {
        let session = create_test_session(vec![]);
        let md = generate_markdown(&session, false);

        assert!(md.contains("message_count: 0"));
        assert!(md.contains("# Untitled Session"));
    }

    #[test]
    fn test_generate_markdown_frontmatter_format() {
        let messages = vec![create_test_message(MessageRole::User, "Test")];
        let session = create_test_session(messages);
        let md = generate_markdown(&session, false);

        // Check frontmatter format
        assert!(md.starts_with("---\n"));
        assert!(md.contains("---\n\n")); // Frontmatter end
        assert!(md.contains("started_at:"));
        assert!(md.contains("updated_at:"));
    }

    #[test]
    fn missing_timestamps_remain_null() {
        let session = ChatSession {
            session_id: "test-session".to_string(),
            provider: "codex".to_string(),
            project_path: std::env::temp_dir().join("test-project"),
            started_at: None,
            updated_at: None,
            messages: vec![ChatMessage {
                id: "message-1".to_string(),
                timestamp: None,
                role: MessageRole::User,
                content: "Hello".to_string(),
                metadata: Default::default(),
            }],
        };

        let markdown = generate_markdown(&session, false);

        assert!(markdown.contains("started_at: null\n"));
        assert!(markdown.contains("updated_at: null\n"));
        assert!(markdown.contains("## 👤 User (null)"));
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

        create_markdown_file(&file_path, &session, false)
            .await
            .unwrap();

        assert!(file_path.exists());
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("Hello"));
        assert!(content.contains("Hi!"));
    }
}
