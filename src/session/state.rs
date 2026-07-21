use std::collections::HashMap;
use std::path::PathBuf;

/// Session sync state - tracks which messages have been synced
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SessionState {
    /// Session ID
    pub session_id: String,

    /// Provider name (codex, claude, gemini)
    pub provider: String,

    /// Path to the session file
    pub file_path: PathBuf,

    /// Path to the generated markdown file
    pub markdown_path: PathBuf,

    /// Number of messages that have been synced
    pub synced_message_count: usize,

    /// Last sync timestamp
    pub last_sync_time: chrono::DateTime<chrono::Utc>,
}

/// Global state for all sessions in a project
#[derive(Debug, Default)]
pub struct ProjectState {
    /// Map of session_id -> SessionState
    pub sessions: HashMap<String, SessionState>,
}

impl ProjectState {
    /// Get session state by ID
    pub fn get_session(&self, session_id: &str) -> Option<&SessionState> {
        self.sessions.get(session_id)
    }

    /// Update or insert session state
    pub fn upsert_session(&mut self, state: SessionState) {
        self.sessions.insert(state.session_id.clone(), state);
    }

    /// Get the number of synced messages for a session
    pub fn get_synced_count(&self, session_id: &str) -> usize {
        self.sessions
            .get(session_id)
            .map(|s| s.synced_message_count)
            .unwrap_or(0)
    }
}
