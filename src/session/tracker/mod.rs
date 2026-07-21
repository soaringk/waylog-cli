mod restore;

use crate::error::Result;
use crate::providers::base::{ChatSession, Provider};
use crate::session::state::{ProjectState, SessionState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Session tracker - manages active sessions and their sync state
pub struct SessionTracker {
    history_dir: PathBuf,
    provider: Arc<dyn Provider>,
    state: Arc<Mutex<ProjectState>>,
}

impl SessionTracker {
    /// Create a new session tracker
    pub async fn new(project_dir: PathBuf, provider: Arc<dyn Provider>) -> Result<Self> {
        Self::new_with_history_dir(crate::utils::path::get_waylog_dir(&project_dir), provider).await
    }

    pub async fn new_with_history_dir(
        history_dir: PathBuf,
        provider: Arc<dyn Provider>,
    ) -> Result<Self> {
        let tracker = Self {
            history_dir,
            provider,
            state: Arc::new(Mutex::new(ProjectState::default())),
        };

        // Restore state from existing markdown files
        let sessions_map =
            restore::restore_from_disk(&tracker.history_dir, tracker.provider.name()).await?;
        if !sessions_map.is_empty() {
            let mut state = tracker.state.lock().await;
            state.sessions = sessions_map;
        }

        Ok(tracker)
    }

    /// Get one session's sync state.
    pub async fn get_session(&self, session_id: &str) -> Option<SessionState> {
        self.state.lock().await.get_session(session_id).cloned()
    }

    /// Get the number of synced messages for a session
    pub async fn get_synced_count(&self, session_id: &str) -> usize {
        let state = self.state.lock().await;
        state.get_synced_count(session_id)
    }

    /// Get the existing markdown path for a session if it exists
    pub async fn get_markdown_path(&self, session_id: &str) -> Option<PathBuf> {
        let state = self.state.lock().await;
        state
            .sessions
            .get(session_id)
            .map(|s| s.markdown_path.clone())
    }

    /// Update session state after syncing
    pub async fn update_session(
        &self,
        session_id: String,
        file_path: PathBuf,
        markdown_path: PathBuf,
        synced_count: usize,
    ) -> Result<()> {
        let mut state = self.state.lock().await;

        let session_state = SessionState {
            session_id: session_id.clone(),
            provider: self.provider.name().to_string(),
            file_path,
            markdown_path,
            synced_message_count: synced_count,
            last_sync_time: chrono::Utc::now(),
        };

        state.upsert_session(session_state);

        // Persistence disabled
        Ok(())
    }

    /// Process a session file and return new messages
    pub async fn get_new_messages(
        &self,
        file_path: &Path,
    ) -> Result<(ChatSession, Vec<crate::providers::base::ChatMessage>)> {
        // Parse the session
        let session = self.provider.parse_session(file_path).await?;

        // Get the number of already synced messages
        let synced_count = self.get_synced_count(&session.session_id).await;

        // Get new messages
        let new_messages = session
            .messages
            .iter()
            .skip(synced_count)
            .cloned()
            .collect();

        Ok((session, new_messages))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{ChatMessage, MessageMetadata, MessageRole};
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use tempfile::TempDir;

    // Mock Provider for testing
    struct MockProvider {
        name: String,
        sessions: HashMap<PathBuf, ChatSession>,
    }

    impl MockProvider {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                sessions: HashMap::new(),
            }
        }

        fn add_session(&mut self, path: PathBuf, session: ChatSession) {
            self.sessions.insert(path, session);
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn find_latest_session(&self, _project_path: &Path) -> Result<Option<PathBuf>> {
            Ok(None)
        }

        async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
            self.sessions.get(file_path).cloned().ok_or_else(|| {
                crate::error::WaylogError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Session not found: {}", file_path.display()),
                ))
            })
        }

        async fn get_all_sessions(&self, _project_path: &Path) -> Result<Vec<PathBuf>> {
            Ok(self.sessions.keys().cloned().collect())
        }

        fn has_history(&self) -> bool {
            true
        }
    }

    fn create_test_session(session_id: &str, message_count: usize) -> ChatSession {
        let now = Utc::now();
        let mut messages = Vec::new();
        for i in 0..message_count {
            messages.push(ChatMessage {
                id: format!("msg-{}", i),
                timestamp: now,
                role: if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                content: format!("Message {}", i),
                metadata: MessageMetadata::default(),
            });
        }

        ChatSession {
            session_id: session_id.to_string(),
            provider: "test".to_string(),
            project_path: PathBuf::from("/test/project"),
            started_at: now,
            updated_at: now,
            messages,
        }
    }

    #[tokio::test]
    async fn test_new_tracker_empty_state() {
        let temp_dir = TempDir::new().unwrap();
        let mock_provider = MockProvider::new("test");
        let provider = Arc::new(mock_provider);

        let tracker = SessionTracker::new(temp_dir.path().to_path_buf(), provider)
            .await
            .unwrap();

        assert!(tracker.get_session("missing").await.is_none());
    }

    #[tokio::test]
    async fn test_update_session() {
        let temp_dir = TempDir::new().unwrap();
        let mock_provider = MockProvider::new("test");
        let provider = Arc::new(mock_provider);

        let tracker = SessionTracker::new(temp_dir.path().to_path_buf(), provider)
            .await
            .unwrap();

        let session_id = "session-1".to_string();
        let file_path = temp_dir.path().join("session-1.json");
        let markdown_path = temp_dir.path().join("session-1.md");
        let synced_count = 7;

        tracker
            .update_session(
                session_id.clone(),
                file_path.clone(),
                markdown_path.clone(),
                synced_count,
            )
            .await
            .unwrap();

        let session_state = tracker.get_session(&session_id).await.unwrap();

        assert_eq!(session_state.session_id, session_id);
        assert_eq!(session_state.provider, "test");
        assert_eq!(session_state.file_path, file_path);
        assert_eq!(session_state.markdown_path, markdown_path);
        assert_eq!(session_state.synced_message_count, synced_count);
    }

    #[tokio::test]
    async fn test_update_session_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let mock_provider = MockProvider::new("test");
        let provider = Arc::new(mock_provider);

        let tracker = SessionTracker::new(temp_dir.path().to_path_buf(), provider)
            .await
            .unwrap();

        let session_id = "session-1".to_string();

        // First update
        tracker
            .update_session(
                session_id.clone(),
                temp_dir.path().join("session-1.json"),
                temp_dir.path().join("session-1.md"),
                5,
            )
            .await
            .unwrap();

        // Second update with different values
        tracker
            .update_session(
                session_id.clone(),
                temp_dir.path().join("session-1-v2.json"),
                temp_dir.path().join("session-1-v2.md"),
                10,
            )
            .await
            .unwrap();

        let session_state = tracker.get_session(&session_id).await.unwrap();
        assert_eq!(session_state.synced_message_count, 10);
        assert_eq!(
            session_state.markdown_path,
            temp_dir.path().join("session-1-v2.md")
        );
    }

    #[tokio::test]
    async fn test_get_new_messages_with_existing_sync() {
        let temp_dir = TempDir::new().unwrap();
        let mut mock_provider = MockProvider::new("test");

        let session_file = temp_dir.path().join("session.json");
        let session = create_test_session("session-1", 10);
        mock_provider.add_session(session_file.clone(), session.clone());

        let provider = Arc::new(mock_provider);
        let tracker = SessionTracker::new(temp_dir.path().to_path_buf(), provider)
            .await
            .unwrap();

        // Mark first 3 messages as synced
        tracker
            .update_session(
                "session-1".to_string(),
                session_file.clone(),
                temp_dir.path().join("session-1.md"),
                3,
            )
            .await
            .unwrap();

        let (parsed_session, new_messages) = tracker.get_new_messages(&session_file).await.unwrap();

        assert_eq!(parsed_session.session_id, "session-1");
        assert_eq!(new_messages.len(), 7); // Only last 7 messages are new
        assert_eq!(new_messages[0].id, "msg-3");
        assert_eq!(new_messages[6].id, "msg-9");
    }

    #[tokio::test]
    async fn test_get_new_messages_all_synced() {
        let temp_dir = TempDir::new().unwrap();
        let mut mock_provider = MockProvider::new("test");

        let session_file = temp_dir.path().join("session.json");
        let session = create_test_session("session-1", 5);
        mock_provider.add_session(session_file.clone(), session.clone());

        let provider = Arc::new(mock_provider);
        let tracker = SessionTracker::new(temp_dir.path().to_path_buf(), provider)
            .await
            .unwrap();

        // Mark all messages as synced
        tracker
            .update_session(
                "session-1".to_string(),
                session_file.clone(),
                temp_dir.path().join("session-1.md"),
                5,
            )
            .await
            .unwrap();

        let (parsed_session, new_messages) = tracker.get_new_messages(&session_file).await.unwrap();

        assert_eq!(parsed_session.session_id, "session-1");
        assert_eq!(new_messages.len(), 0); // No new messages
    }

    #[tokio::test]
    async fn test_restore_from_disk_multiple_files() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        tokio::fs::create_dir_all(&project_dir).await.unwrap();

        // Create .waylog/history directory (where markdown files are stored)
        let history_dir = crate::utils::path::get_waylog_dir(&project_dir);
        tokio::fs::create_dir_all(&history_dir).await.unwrap();

        // Create multiple markdown files
        let content1 = r#"---
provider: test
session_id: session-1
message_count: 3
---
# Session 1
"#;
        tokio::fs::write(history_dir.join("session-1.md"), content1)
            .await
            .unwrap();

        let content2 = r#"---
provider: test
session_id: session-2
message_count: 7
---
# Session 2
"#;
        tokio::fs::write(history_dir.join("session-2.md"), content2)
            .await
            .unwrap();

        // Create a file without frontmatter (should be ignored)
        tokio::fs::write(history_dir.join("no-frontmatter.md"), "# No frontmatter")
            .await
            .unwrap();

        // Create a non-md file (should be ignored)
        tokio::fs::write(history_dir.join("readme.txt"), "Not a markdown file")
            .await
            .unwrap();

        let mock_provider = MockProvider::new("test");
        let provider = Arc::new(mock_provider);

        let tracker = SessionTracker::new(project_dir, provider).await.unwrap();

        assert_eq!(
            tracker
                .get_session("session-1")
                .await
                .unwrap()
                .synced_message_count,
            3
        );
        assert_eq!(
            tracker
                .get_session("session-2")
                .await
                .unwrap()
                .synced_message_count,
            7
        );
        assert!(tracker.get_session("no-frontmatter").await.is_none());
    }

    #[tokio::test]
    async fn test_restore_from_disk_missing_provider_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        tokio::fs::create_dir_all(&project_dir).await.unwrap();

        // Create .waylog/history directory (where markdown files are stored)
        let history_dir = crate::utils::path::get_waylog_dir(&project_dir);
        tokio::fs::create_dir_all(&history_dir).await.unwrap();

        // Create markdown file without provider in frontmatter
        let content = r#"---
session_id: session-1
message_count: 5
---
# Session
"#;
        tokio::fs::write(history_dir.join("session-1.md"), content)
            .await
            .unwrap();

        let mock_provider = MockProvider::new("test-provider");
        let provider = Arc::new(mock_provider);

        let tracker = SessionTracker::new(project_dir, provider).await.unwrap();

        let session_state = tracker.get_session("session-1").await.unwrap();
        // Should fallback to provider name
        assert_eq!(session_state.provider, "test-provider");
    }
}
