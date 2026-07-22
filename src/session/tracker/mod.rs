mod restore;

use crate::error::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct SessionState {
    pub(crate) markdown_path: PathBuf,
    pub(crate) synced_message_count: usize,
    pub(crate) include_tool_calls: bool,
}

/// In-memory sync state restored from generated Markdown.
pub(crate) struct SessionTracker {
    sessions: Mutex<HashMap<String, SessionState>>,
}

impl SessionTracker {
    pub(crate) async fn new(history_dir: &Path, provider_name: &str) -> Result<Self> {
        Ok(Self {
            sessions: Mutex::new(restore::restore_from_disk(history_dir, provider_name).await?),
        })
    }

    pub(crate) async fn get_session(&self, session_id: &str) -> Option<SessionState> {
        self.sessions.lock().await.get(session_id).cloned()
    }

    pub(crate) async fn update_session(
        &self,
        session_id: String,
        markdown_path: PathBuf,
        synced_count: usize,
        include_tool_calls: bool,
    ) {
        self.sessions.lock().await.insert(
            session_id,
            SessionState {
                markdown_path,
                synced_message_count: synced_count,
                include_tool_calls,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn updates_session_state() {
        let temp_dir = TempDir::new().unwrap();
        let tracker = SessionTracker::new(temp_dir.path(), "test").await.unwrap();

        assert!(tracker.get_session("missing").await.is_none());
        tracker
            .update_session(
                "session-1".to_string(),
                temp_dir.path().join("old.md"),
                5,
                false,
            )
            .await;
        tracker
            .update_session(
                "session-1".to_string(),
                temp_dir.path().join("current.md"),
                10,
                true,
            )
            .await;

        let state = tracker.get_session("session-1").await.unwrap();
        assert_eq!(state.markdown_path, temp_dir.path().join("current.md"));
        assert_eq!(state.synced_message_count, 10);
        assert!(state.include_tool_calls);
    }

    #[tokio::test]
    async fn restores_only_matching_provider_state() {
        let temp_dir = TempDir::new().unwrap();
        let matching = r#"---
provider: test
session_id: matching
message_count: 3
include_tool_calls: true
---
"#;
        let other_provider = r#"---
provider: other
session_id: ignored
message_count: 9
---
"#;
        let legacy = r#"---
session_id: legacy
message_count: 5
---
"#;
        tokio::fs::write(temp_dir.path().join("matching.md"), matching)
            .await
            .unwrap();
        tokio::fs::write(temp_dir.path().join("ignored.md"), other_provider)
            .await
            .unwrap();
        tokio::fs::write(temp_dir.path().join("legacy.md"), legacy)
            .await
            .unwrap();

        let tracker = SessionTracker::new(temp_dir.path(), "test").await.unwrap();

        let matching = tracker.get_session("matching").await.unwrap();
        assert_eq!(matching.synced_message_count, 3);
        assert!(matching.include_tool_calls);
        assert!(tracker.get_session("legacy").await.is_some());
        assert!(tracker.get_session("ignored").await.is_none());
    }
}
