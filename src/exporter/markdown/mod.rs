mod formatter;
mod frontmatter;

pub use frontmatter::parse_frontmatter;

use crate::exporter::{entries, project};
use crate::providers::base::ChatSession;

/// Generate the readable representation of a chat session.
pub fn generate(session: &ChatSession, include_tool_calls: bool) -> String {
    let mut md = String::new();

    // Frontmatter
    md.push_str("---\n");
    md.push_str(&format!("provider: {}\n", session.provider));
    md.push_str(&format!("session_id: {}\n", session.session_id));
    md.push_str(&format!(
        "project: {}\n",
        frontmatter_value(project(session))
    ));
    md.push_str(&format!(
        "started_at: {}\n",
        frontmatter_value(session.started_at.map(|value| value.to_rfc3339()))
    ));
    md.push_str(&format!(
        "updated_at: {}\n",
        frontmatter_value(session.updated_at.map(|value| value.to_rfc3339()))
    ));
    md.push_str(&format!(
        "message_count: {}\n",
        crate::exporter::message_count(session, include_tool_calls)
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
    md.push_str(&formatter::format_entries(&entries(
        session,
        include_tool_calls,
    )));

    md
}

/// Values the provider did not record are written as `null`, never as an empty value.
fn frontmatter_value(value: Option<impl std::fmt::Display>) -> String {
    value.map_or_else(|| "null".to_string(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{AssistantOutput, ChatMessage, MessageRole, TokenUsage};
    use chrono::Utc;
    use tempfile::TempDir;

    fn create_test_message(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
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

    #[test]
    fn renders_each_role_with_its_own_label() {
        for (role, label, content) in [
            (MessageRole::User, "## 👤 User", "Hello, world!"),
            (MessageRole::System, "## ⚙️ System", "System prompt"),
            (
                MessageRole::Assistant(AssistantOutput::Message),
                "### 💬 Message",
                "How can I help?",
            ),
            (
                MessageRole::Assistant(AssistantOutput::Reasoning),
                "### 🧠 Reasoning",
                "Weighing options",
            ),
        ] {
            let session = create_test_session(vec![create_test_message(role, content)]);
            let markdown = generate(&session, false);
            assert!(markdown.contains(label), "missing {label}");
            assert!(markdown.contains(content));
        }
    }

    #[test]
    fn renders_message_metadata_recorded_by_the_provider() {
        let mut message = create_test_message(
            MessageRole::Assistant(AssistantOutput::Message),
            "I'll use a tool",
        );
        message.metadata.tool_calls = vec!["read_file".to_string()];
        message.metadata.thoughts = vec!["Thought 1".to_string(), "Thought 2".to_string()];

        let markdown = generate(&create_test_session(vec![message]), false);

        assert!(markdown.contains("**Tools Used:**"));
        assert!(markdown.contains("`read_file`"));
        assert!(markdown.contains("<summary>💭 Thoughts</summary>"));
        assert!(markdown.contains("Thought 1"));
        assert!(markdown.contains("Thought 2"));
    }

    #[test]
    fn tool_exchanges_are_opt_in_and_grouped_by_call_id() {
        let assistant = create_test_message(
            MessageRole::Assistant(AssistantOutput::Message),
            "Checking the file",
        );
        let mut request_a = create_test_message(
            MessageRole::Assistant(AssistantOutput::Tool),
            r#"{"type":"function_call","call_id":"a"}"#,
        );
        request_a.metadata.tool_call_id = Some("a".to_string());
        let mut request_b = create_test_message(
            MessageRole::Assistant(AssistantOutput::Tool),
            r#"{"type":"function_call","call_id":"b"}"#,
        );
        request_b.metadata.tool_call_id = Some("b".to_string());
        let mut response_b = create_test_message(
            MessageRole::Assistant(AssistantOutput::Tool),
            r#"{"type":"function_call_output","call_id":"b"}"#,
        );
        response_b.metadata.tool_call_id = Some("b".to_string());
        let mut response_a = create_test_message(
            MessageRole::Assistant(AssistantOutput::Tool),
            r#"{"type":"function_call_output","call_id":"a","output":"```"}"#,
        );
        response_a.metadata.tool_call_id = Some("a".to_string());
        let session = create_test_session(vec![
            assistant, request_a, request_b, response_b, response_a,
        ]);

        let default_markdown = generate(&session, false);
        assert!(default_markdown.contains("message_count: 1"));
        assert!(!default_markdown.contains("🛠️ Tool"));

        let markdown = generate(&session, true);
        assert!(markdown.contains("message_count: 5"));
        assert!(markdown.contains("include_tool_calls: true"));
        assert!(!markdown.contains("```json"));
        let groups = markdown.split("### 🛠️ Tool").skip(1).collect::<Vec<_>>();
        assert_eq!(groups.len(), 2);
        assert!(groups[0].contains(r#""call_id":"a""#));
        assert!(groups[0].contains("function_call_output"));
        assert!(groups[0].contains("````\n"));
        assert!(groups[1].contains(r#""call_id":"b""#));
        assert!(groups[1].contains("function_call_output"));
    }

    #[test]
    fn records_session_facts_in_frontmatter() {
        let mut user = create_test_message(MessageRole::User, "Hello");
        user.metadata.tokens = Some(TokenUsage {
            input: 10,
            output: 20,
            cached: 5,
        });
        let session = create_test_session(vec![
            user,
            create_test_message(
                MessageRole::Assistant(AssistantOutput::Message),
                "Hi there!",
            ),
        ]);

        let markdown = generate(&session, false);

        assert!(markdown.starts_with("---\n"));
        assert!(markdown.contains("provider: claude\n"));
        assert!(markdown.contains("session_id: test-session\n"));
        assert!(markdown.contains("message_count: 2\n"));
        assert!(markdown.contains("started_at:"));
        assert!(markdown.contains("updated_at:"));
        assert!(markdown.contains("total_tokens: 30\n"));
        assert!(markdown.contains("---\n\n"));
        // The title comes from the first user message.
        assert!(markdown.contains("# Hello\n"));
        assert!(markdown.contains("Hi there!"));
    }

    #[test]
    fn omits_absent_facts_and_titles_a_session_without_user_text() {
        let empty = generate(&create_test_session(Vec::new()), false);
        assert!(empty.contains("message_count: 0\n"));
        assert!(empty.contains("# Untitled Session\n"));
        assert!(!empty.contains("total_tokens"));
    }

    #[test]
    fn unrecorded_session_facts_remain_null() {
        let session = ChatSession {
            session_id: "test-session".to_string(),
            provider: "codex".to_string(),
            project_path: std::path::PathBuf::new(),
            started_at: None,
            updated_at: None,
            messages: vec![ChatMessage {
                timestamp: None,
                role: MessageRole::User,
                content: "Hello".to_string(),
                metadata: Default::default(),
            }],
        };

        let markdown = generate(&session, false);

        assert!(markdown.contains("project: null\n"));
        assert!(markdown.contains("started_at: null\n"));
        assert!(markdown.contains("updated_at: null\n"));
        assert!(markdown.contains("## 👤 User (null)"));
    }

    #[tokio::test]
    async fn writes_a_readable_file_through_the_shared_seam() {
        use crate::exporter::{write_session, ExportFormat, ExportOptions};

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.md");
        let session = create_test_session(vec![
            create_test_message(MessageRole::User, "Hello"),
            create_test_message(MessageRole::Assistant(AssistantOutput::Message), "Hi!"),
        ]);

        write_session(
            &file_path,
            &session,
            ExportOptions {
                format: ExportFormat::Markdown,
                include_tool_calls: false,
            },
        )
        .await
        .unwrap();

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert!(content.contains("## 👤 User"));
        assert!(content.contains("### 💬 Message"));
        assert!(content.contains("Hi!"));
    }
}
