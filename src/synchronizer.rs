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
    tracking_root: PathBuf,
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
        Self {
            provider,
            tracking_root,
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
        let mut results = Vec::new();

        for session_path in sessions {
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
        let state = self.tracker.get_state().await;
        let (markdown_path, mut synced_count) =
            if let Some(s) = state.get_session(&session.session_id) {
                (s.markdown_path.clone(), s.synced_message_count)
            } else {
                // New session: generate filename
                let slug = session
                    .messages
                    .iter()
                    .find(|m| m.role == crate::providers::base::MessageRole::User)
                    .map(|m| crate::utils::string::slugify(&m.content))
                    .unwrap_or_else(|| session.session_id.clone());

                let timestamp = session.started_at.format("%Y-%m-%d_%H-%M-%SZ");
                let filename = format!("{}-{}-{}.md", timestamp, self.provider.name(), slug);
                let path = path::get_waylog_dir(&self.tracking_root).join(filename);

                (path, 0)
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

        if synced_count == 0 {
            exporter::create_markdown_file(&markdown_path, &session).await?;
        } else {
            exporter::append_messages(&markdown_path, &new_messages).await?;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{
        ChatMessage, ChatSession, MessageMetadata, MessageRole, Provider,
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    struct MockProvider {
        session_path: PathBuf,
        session: ChatSession,
        queried_projects: Arc<Mutex<Vec<PathBuf>>>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn data_dir(&self) -> Result<PathBuf> {
            Ok(std::env::temp_dir())
        }

        fn session_dir(&self, _project_path: &Path) -> Result<PathBuf> {
            Ok(std::env::temp_dir())
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

        fn is_installed(&self) -> bool {
            true
        }

        fn command(&self) -> &str {
            "mock"
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
}
