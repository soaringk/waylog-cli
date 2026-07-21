use crate::error::Result;
use crate::exporter;
use crate::providers::base::Provider;
use crate::session::SessionTracker;
use crate::utils::path;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::debug;

/// Shared synchronization logic for both watcher and batch sync
pub struct Synchronizer {
    provider: Arc<dyn Provider>,
    history_dir: PathBuf,
    target_project_dir: PathBuf,
    tracker: Arc<SessionTracker>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncStatus {
    Synced { new_messages: usize },
    UpToDate,
    Skipped,
    Failed(String),
}

impl Synchronizer {
    pub fn new(
        provider: Arc<dyn Provider>,
        tracking_root: PathBuf,
        target_project_dir: PathBuf,
        tracker: Arc<SessionTracker>,
    ) -> Self {
        Self::new_with_history_dir(
            provider,
            path::get_waylog_dir(&tracking_root),
            target_project_dir,
            tracker,
        )
    }

    pub fn new_with_history_dir(
        provider: Arc<dyn Provider>,
        history_dir: PathBuf,
        target_project_dir: PathBuf,
        tracker: Arc<SessionTracker>,
    ) -> Self {
        Self {
            provider,
            history_dir,
            target_project_dir,
            tracker,
        }
    }

    /// Sync all available sessions from the provider
    /// Returns stats: (Synced, UpToDate, Skipped, Failed)
    pub async fn sync_all(&self, force: bool) -> Result<Vec<(PathBuf, SyncStatus)>> {
        let sessions = self
            .provider
            .get_all_sessions(&self.target_project_dir)
            .await?;
        self.sync_paths(sessions, force).await
    }

    /// Sync a known list of session files
    pub async fn sync_paths(
        &self,
        session_paths: Vec<PathBuf>,
        force: bool,
    ) -> Result<Vec<(PathBuf, SyncStatus)>> {
        let mut results = Vec::new();

        for session_path in session_paths {
            let status = match self.sync_session(&session_path, force).await {
                Ok(status) => status,
                Err(e) => SyncStatus::Failed(e.to_string()),
            };
            results.push((session_path, status));
        }

        Ok(results)
    }

    /// Sync a specific session file
    pub async fn sync_session(&self, session_path: &Path, force: bool) -> Result<SyncStatus> {
        // 1. Parse session
        let session = match self.provider.parse_session(session_path).await {
            Ok(s) => s,
            Err(e) => return Ok(SyncStatus::Failed(format!("Parse error: {}", e))),
        };

        if session.messages.is_empty() {
            return Ok(SyncStatus::Skipped);
        }

        // 2. Check state
        let (markdown_path, mut synced_count) =
            if let Some(state) = self.tracker.get_session(&session.session_id).await {
                (state.markdown_path, state.synced_message_count)
            } else {
                (
                    self.history_dir
                        .join(session_markdown_filename(&session, self.provider.name())),
                    0,
                )
            };

        // 3. Handle force/missing file
        if force || (!markdown_path.exists() && synced_count > 0) {
            synced_count = 0;
        }

        // 4. Calculate new messages
        let total_messages = session.messages.len();
        if synced_count >= total_messages {
            return Ok(SyncStatus::UpToDate);
        }

        let new_messages: Vec<_> = session
            .messages
            .iter()
            .skip(synced_count)
            .cloned()
            .collect();

        if new_messages.is_empty() {
            return Ok(SyncStatus::UpToDate);
        }

        // 5. Write to file
        if let Some(parent) = markdown_path.parent() {
            path::ensure_dir_exists(parent)?;
        }

        exporter::create_markdown_file(&markdown_path, &session).await?;

        // 6. Update state
        self.tracker
            .update_session(
                session.session_id.clone(),
                session_path.to_path_buf(),
                markdown_path.clone(),
                total_messages,
            )
            .await?;

        // Log purely for debug, UI is handled by caller
        debug!(
            "Synced {} messages to {}",
            new_messages.len(),
            markdown_path.display()
        );

        Ok(SyncStatus::Synced {
            new_messages: new_messages.len(),
        })
    }
}

pub(crate) fn session_markdown_filename(
    session: &crate::providers::base::ChatSession,
    provider_name: &str,
) -> String {
    let slug = session
        .messages
        .iter()
        .find(|m| m.role == crate::providers::base::MessageRole::User)
        .map(|m| crate::utils::string::slugify(&m.content))
        .unwrap_or_else(|| crate::utils::string::slugify(&session.session_id));
    let session_id = session_id_filename_component(&session.session_id);
    let timestamp = session.started_at.format("%Y-%m-%d_%H-%M-%SZ");

    format!("{}-{}-{}-{}.md", timestamp, provider_name, session_id, slug)
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
    use crate::providers::base::{
        ChatMessage, ChatSession, MessageMetadata, MessageRole, Provider,
    };
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    struct MockProvider {
        session_path: PathBuf,
        session: ChatSession,
        queried_projects: Arc<Mutex<Vec<PathBuf>>>,
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
            started_at: timestamp,
            updated_at: timestamp,
            messages: vec![
                ChatMessage {
                    id: "msg-1".to_string(),
                    timestamp,
                    role: MessageRole::User,
                    content: "First message".to_string(),
                    metadata: MessageMetadata::default(),
                },
                ChatMessage {
                    id: "msg-2".to_string(),
                    timestamp,
                    role: MessageRole::Assistant,
                    content: "Second message".to_string(),
                    metadata: MessageMetadata::default(),
                },
            ],
        }
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

        async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
            self.queried_projects
                .lock()
                .await
                .push(project_path.to_path_buf());
            Ok(vec![self.session_path.clone()])
        }

        fn has_history(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn sync_all_uses_target_project_for_lookup_and_tracking_root_for_output() {
        let temp_dir = TempDir::new().unwrap();
        let tracking_root = temp_dir.path().join("tracking-root");
        let target_project = temp_dir.path().join("tracking-root").join("nested-project");
        let session_path = temp_dir.path().join("session.jsonl");

        tokio::fs::create_dir_all(&tracking_root).await.unwrap();
        tokio::fs::create_dir_all(&target_project).await.unwrap();

        let now = Utc::now();
        let session = ChatSession {
            session_id: "session-1".to_string(),
            provider: "mock".to_string(),
            project_path: target_project.clone(),
            started_at: now,
            updated_at: now,
            messages: vec![ChatMessage {
                id: "msg-1".to_string(),
                timestamp: now,
                role: MessageRole::User,
                content: "Nested project session".to_string(),
                metadata: MessageMetadata::default(),
            }],
        };

        let queried_projects = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session,
            queried_projects: queried_projects.clone(),
        });

        let tracker = Arc::new(
            SessionTracker::new(tracking_root.clone(), provider.clone())
                .await
                .unwrap(),
        );
        let synchronizer = Synchronizer::new(
            provider,
            tracking_root.clone(),
            target_project.clone(),
            tracker,
        );

        let results = synchronizer.sync_all(false).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, session_path);
        assert!(matches!(
            results[0].1,
            SyncStatus::Synced { new_messages: 1 }
        ));

        let queried = queried_projects.lock().await;
        assert_eq!(queried.as_slice(), &[target_project]);
        drop(queried);

        let history_dir = path::get_waylog_dir(&tracking_root);
        let mut entries = tokio::fs::read_dir(&history_dir).await.unwrap();
        let markdown = entries.next_entry().await.unwrap().unwrap().path();
        assert!(markdown.starts_with(&history_dir));
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

        let queried_projects = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session,
            queried_projects,
        });
        let tracker = Arc::new(
            SessionTracker::new(tracking_root.clone(), provider.clone())
                .await
                .unwrap(),
        );
        let synchronizer =
            Synchronizer::new(provider, tracking_root, target_project.clone(), tracker);

        let status = synchronizer
            .sync_session(&session_path, false)
            .await
            .unwrap();
        assert!(matches!(status, SyncStatus::Synced { new_messages: 1 }));

        let content = tokio::fs::read_to_string(&markdown_path).await.unwrap();
        assert!(content.contains("message_count: 2"));
        assert_eq!(content.matches("Second message").count(), 1);

        let tracking_root = target_project.parent().unwrap().to_path_buf();
        let tracker_provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session: two_message_session("session-1", target_project.clone(), now),
            queried_projects: Arc::new(Mutex::new(Vec::new())),
        });
        let tracker = Arc::new(
            SessionTracker::new(tracking_root.clone(), tracker_provider)
                .await
                .unwrap(),
        );
        let queried_projects = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(MockProvider {
            session_path: session_path.clone(),
            session: two_message_session("session-1", target_project.clone(), now),
            queried_projects,
        });
        let synchronizer = Synchronizer::new(provider, tracking_root, target_project, tracker);

        let status = synchronizer
            .sync_session(&session_path, false)
            .await
            .unwrap();
        assert_eq!(status, SyncStatus::UpToDate);
    }

    #[test]
    fn session_markdown_filename_includes_session_id() {
        let started_at = Utc.with_ymd_and_hms(2026, 4, 7, 3, 39, 25).unwrap();
        let session = ChatSession {
            session_id: "session-1".to_string(),
            provider: "mock".to_string(),
            project_path: PathBuf::from("/project"),
            started_at,
            updated_at: started_at,
            messages: vec![ChatMessage {
                id: "msg-1".to_string(),
                timestamp: started_at,
                role: MessageRole::User,
                content: "Same title".to_string(),
                metadata: MessageMetadata::default(),
            }],
        };

        assert_eq!(
            session_markdown_filename(&session, "mock"),
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
