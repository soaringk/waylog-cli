use crate::error::Result;
use crate::exporter::{self, ExportOptions};
use crate::providers::base::Provider;
use crate::session::SessionTracker;
use crate::utils::path;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::debug;

/// Shared synchronization logic for both watcher and batch sync
pub(crate) struct Synchronizer {
    provider: Arc<dyn Provider>,
    history_dir: PathBuf,
    tracker: Arc<SessionTracker>,
    options: ExportOptions,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncStatus {
    Synced { new_messages: usize },
    UpToDate,
    Skipped,
    Failed(String),
}

impl Synchronizer {
    pub(crate) fn new(
        provider: Arc<dyn Provider>,
        history_dir: PathBuf,
        tracker: Arc<SessionTracker>,
        options: ExportOptions,
    ) -> Self {
        Self {
            provider,
            history_dir,
            tracker,
            options,
        }
    }

    /// Sync a known list of session files
    pub(crate) async fn sync_paths(
        &self,
        session_paths: Vec<PathBuf>,
        force: bool,
    ) -> Vec<(PathBuf, SyncStatus)> {
        let mut results = Vec::new();

        for session_path in session_paths {
            let status = match self.sync_session(&session_path, force).await {
                Ok(status) => status,
                Err(e) => SyncStatus::Failed(e.to_string()),
            };
            results.push((session_path, status));
        }

        results
    }

    /// Sync a specific session file
    pub(crate) async fn sync_session(
        &self,
        session_path: &Path,
        force: bool,
    ) -> Result<SyncStatus> {
        // 1. Parse session
        let session = match self.provider.parse_session(session_path).await {
            Ok(s) => s,
            Err(e) => return Ok(SyncStatus::Failed(format!("Parse error: {}", e))),
        };

        if session.messages.is_empty() {
            return Ok(SyncStatus::Skipped);
        }

        // 2. Check state
        let (export_path, mut synced_count, previous_include_tool_calls) =
            if let Some(state) = self.tracker.get_session(&session.session_id).await {
                (
                    state.export_path,
                    state.synced_message_count,
                    state.include_tool_calls,
                )
            } else {
                (
                    self.history_dir.join(session_export_filename(
                        &session,
                        self.provider.name(),
                        self.options.format,
                    )),
                    0,
                    self.options.include_tool_calls,
                )
            };

        // 3. Handle force/missing file
        if force
            || previous_include_tool_calls != self.options.include_tool_calls
            || (!export_path.exists() && synced_count > 0)
        {
            synced_count = 0;
        }

        // 4. Calculate new messages. Any difference means the recorded Markdown no longer
        // matches the session, including when parsing now produces fewer messages.
        let total_messages = exporter::message_count(&session, self.options.include_tool_calls);
        if synced_count == total_messages {
            return Ok(SyncStatus::UpToDate);
        }
        let new_messages = total_messages.saturating_sub(synced_count);

        // 5. Write to file
        if let Some(parent) = export_path.parent() {
            path::ensure_dir_exists(parent)?;
        }

        exporter::write_session(&export_path, &session, self.options).await?;

        // 6. Update state
        self.tracker
            .update_session(
                session.session_id.clone(),
                export_path.clone(),
                total_messages,
                self.options.include_tool_calls,
            )
            .await;

        // Log purely for debug, UI is handled by caller
        debug!(
            "Synced {} messages to {}",
            new_messages,
            export_path.display()
        );

        Ok(SyncStatus::Synced { new_messages })
    }
}

pub(crate) fn session_export_filename(
    session: &crate::providers::base::ChatSession,
    provider_name: &str,
    format: exporter::ExportFormat,
) -> String {
    let slug = session
        .messages
        .iter()
        .find(|m| m.role == crate::providers::base::MessageRole::User)
        .map(|m| crate::utils::string::slugify(&m.content))
        .unwrap_or_else(|| crate::utils::string::slugify(&session.session_id));
    let session_id = session_id_filename_component(&session.session_id);
    let timestamp = session
        .started_at
        .as_ref()
        .map(|value| value.format("%Y-%m-%d_%H-%M-%SZ").to_string())
        .unwrap_or_else(|| "unknown-time".to_string());

    format!(
        "{}-{}-{}-{}.{}",
        timestamp,
        provider_name,
        session_id,
        slug,
        format.extension()
    )
}

fn session_id_filename_component(session_id: &str) -> String {
    session_id
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exporter::ExportFormat;
    use crate::providers::base::{
        AssistantOutput, ChatMessage, ChatSession, MessageMetadata, MessageRole, Provider,
    };
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use std::path::Path;
    use tempfile::TempDir;

    struct MockProvider {
        session_path: PathBuf,
        session: ChatSession,
    }

    fn two_message_session(
        session_id: &str,
        project_path: PathBuf,
        timestamp: chrono::DateTime<Utc>,
    ) -> ChatSession {
        ChatSession {
            session_id: session_id.to_string(),
            provider: "mock".to_string(),
            project_path,
            started_at: Some(timestamp),
            updated_at: Some(timestamp),
            messages: vec![
                ChatMessage {
                    timestamp: Some(timestamp),
                    role: MessageRole::User,
                    content: "First message".to_string(),
                    metadata: MessageMetadata::default(),
                },
                ChatMessage {
                    timestamp: Some(timestamp),
                    role: MessageRole::Assistant(AssistantOutput::Message),
                    content: "Second message".to_string(),
                    metadata: MessageMetadata::default(),
                },
            ],
        }
    }

    #[test]
    fn missing_start_time_uses_an_explicit_filename_marker() {
        let session = ChatSession {
            session_id: "session-1".to_string(),
            provider: "mock".to_string(),
            project_path: PathBuf::from("/tmp/project"),
            started_at: None,
            updated_at: None,
            messages: vec![ChatMessage {
                timestamp: None,
                role: MessageRole::User,
                content: "First message".to_string(),
                metadata: MessageMetadata::default(),
            }],
        };

        assert_eq!(
            session_export_filename(&session, "mock", ExportFormat::Markdown),
            "unknown-time-mock-session-1-first-message.md"
        );
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn find_latest_session(&self, _project_path: &Path) -> Result<Option<PathBuf>> {
            Ok(Some(self.session_path.clone()))
        }

        async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
            assert_eq!(file_path, self.session_path);
            Ok(self.session.clone())
        }

        async fn get_all_sessions(&self, _project_path: &Path) -> Result<Vec<PathBuf>> {
            Ok(vec![self.session_path.clone()])
        }

        fn has_history(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn sync_session_rewrites_stale_markdown_instead_of_reappending_suffix() {
        let temp_dir = TempDir::new().unwrap();
        let tracking_root = temp_dir.path().join("tracking-root");
        let target_project = temp_dir.path().join("tracking-root").join("nested-project");
        let session_path = temp_dir.path().join("session.jsonl");
        let history_dir = path::get_waylog_dir(&tracking_root);
        let markdown_path = history_dir.join("old-session.md");

        tokio::fs::create_dir_all(&history_dir).await.unwrap();
        tokio::fs::create_dir_all(&target_project).await.unwrap();

        let now = Utc.with_ymd_and_hms(2026, 4, 7, 3, 39, 25).unwrap();
        let session = two_message_session("session-1", target_project.clone(), now);

        tokio::fs::write(
            &markdown_path,
            r#"---
provider: mock
session_id: session-1
project: /tmp/project
started_at: 2026-04-07T03:39:25Z
updated_at: 2026-04-07T03:39:25Z
message_count: 1
---

# First message

## 👤 User (2026-04-07 03:39:25 UTC)

First message

## 🤖 Assistant (2026-04-07 03:39:25 UTC)

Second message

## 🤖 Assistant (2026-04-07 03:39:25 UTC)

Second message
"#,
        )
        .await
        .unwrap();

        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session,
        });
        let tracker = Arc::new(
            SessionTracker::new(&history_dir, provider.name(), ExportFormat::Markdown)
                .await
                .unwrap(),
        );
        let synchronizer = Synchronizer::new(
            provider,
            history_dir.clone(),
            tracker,
            ExportOptions::default(),
        );

        let status = synchronizer
            .sync_session(&session_path, false)
            .await
            .unwrap();
        assert!(matches!(status, SyncStatus::Synced { new_messages: 1 }));

        let content = tokio::fs::read_to_string(&markdown_path).await.unwrap();
        assert!(content.contains("message_count: 2"));
        assert_eq!(content.matches("Second message").count(), 1);

        let tracker = Arc::new(
            SessionTracker::new(&history_dir, "mock", ExportFormat::Markdown)
                .await
                .unwrap(),
        );
        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session: two_message_session("session-1", target_project.clone(), now),
        });
        let synchronizer =
            Synchronizer::new(provider, history_dir, tracker, ExportOptions::default());

        let status = synchronizer
            .sync_session(&session_path, false)
            .await
            .unwrap();
        assert_eq!(status, SyncStatus::UpToDate);
    }

    #[tokio::test]
    async fn changing_tool_output_mode_rewrites_existing_markdown() {
        let temp_dir = TempDir::new().unwrap();
        let project = temp_dir.path().join("project");
        let session_path = temp_dir.path().join("session.jsonl");
        let history_dir = path::get_waylog_dir(&project);
        tokio::fs::create_dir_all(&project).await.unwrap();
        let now = Utc.with_ymd_and_hms(2026, 7, 22, 0, 0, 0).unwrap();
        let mut session = two_message_session("session-1", project.clone(), now);
        session.messages.insert(
            1,
            ChatMessage {
                timestamp: Some(now),
                role: MessageRole::Assistant(AssistantOutput::Tool),
                content: r#"{"name":"read","input":{"path":"src/main.rs"}}"#.to_string(),
                metadata: MessageMetadata::default(),
            },
        );

        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session: session.clone(),
        });
        let tracker = Arc::new(
            SessionTracker::new(&history_dir, provider.name(), ExportFormat::Markdown)
                .await
                .unwrap(),
        );
        Synchronizer::new(
            provider,
            history_dir.clone(),
            tracker,
            ExportOptions::default(),
        )
        .sync_session(&session_path, false)
        .await
        .unwrap();

        let markdown_path = tokio::fs::read_dir(&history_dir)
            .await
            .unwrap()
            .next_entry()
            .await
            .unwrap()
            .unwrap()
            .path();
        assert!(!tokio::fs::read_to_string(&markdown_path)
            .await
            .unwrap()
            .contains("🛠️ Tool"));

        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session,
        });
        let tracker = Arc::new(
            SessionTracker::new(&history_dir, provider.name(), ExportFormat::Markdown)
                .await
                .unwrap(),
        );
        let status = Synchronizer::new(
            provider,
            history_dir,
            tracker,
            ExportOptions {
                include_tool_calls: true,
                ..Default::default()
            },
        )
        .sync_session(&session_path, false)
        .await
        .unwrap();

        assert!(matches!(status, SyncStatus::Synced { new_messages: 3 }));
        let markdown = tokio::fs::read_to_string(markdown_path).await.unwrap();
        assert!(markdown.contains("include_tool_calls: true"));
        assert!(markdown.contains("🛠️ Tool"));
    }

    #[test]
    fn session_markdown_filename_includes_session_id() {
        let started_at = Utc.with_ymd_and_hms(2026, 4, 7, 3, 39, 25).unwrap();
        let session = ChatSession {
            session_id: "session-1".to_string(),
            provider: "mock".to_string(),
            project_path: PathBuf::from("/project"),
            started_at: Some(started_at),
            updated_at: Some(started_at),
            messages: vec![ChatMessage {
                timestamp: Some(started_at),
                role: MessageRole::User,
                content: "Same title".to_string(),
                metadata: MessageMetadata::default(),
            }],
        };

        assert_eq!(
            session_export_filename(&session, "mock", ExportFormat::Markdown),
            "2026-04-07_03-39-25Z-mock-session-1-same-title.md"
        );
    }

    #[test]
    fn session_id_filename_component_is_safe_and_not_truncated() {
        let session_id = "rollout-2026-07-20T11:41:43-019f7d9d-8583-7260-b494-56fb96900012";

        assert_eq!(
            session_id_filename_component(session_id),
            "rollout-2026-07-20T11%3A41%3A43-019f7d9d-8583-7260-b494-56fb96900012"
        );
    }
}
