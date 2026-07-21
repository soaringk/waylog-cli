use crate::error::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Represents a chat message from any AI provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub role: MessageRole,
    pub content: String,
    pub metadata: MessageMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageMetadata {
    /// Model used (e.g., "claude-sonnet-4.5", "gemini-2.5-flash")
    pub model: Option<String>,

    /// Token usage
    pub tokens: Option<TokenUsage>,

    /// Tool calls (for Claude Code)
    pub tool_calls: Vec<String>,

    /// Thoughts (for Gemini)
    pub thoughts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u32,
    pub output: u32,
    pub cached: u32,
}

/// Represents a complete chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub session_id: String,
    pub provider: String,
    pub project_path: PathBuf,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatMessage>,
}

/// Converts one provider's discovered or supplied history into chat sessions.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the provider name (e.g., "codex", "claude", "gemini")
    fn name(&self) -> &str;

    /// Whether session discovery is limited to a project path.
    fn is_project_scoped(&self) -> bool {
        true
    }

    /// Find the latest session file for the current project
    async fn find_latest_session(&self, project_path: &Path) -> Result<Option<PathBuf>> {
        Ok(self
            .get_all_sessions(project_path)
            .await?
            .into_iter()
            .next())
    }

    /// Find one session by its provider session ID.
    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        for session_path in self.get_all_sessions(project_path).await? {
            let session = self.parse_session(&session_path).await?;
            if session.session_id == session_id {
                return Ok(Some(session_path));
            }
        }
        Ok(None)
    }

    /// Parse a session file and return a chat session
    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession>;

    /// Get all session files for a specific project
    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>>;

    /// Check whether provider history is available.
    fn has_history(&self) -> bool;

    /// Get the CLI command when this provider can be launched by `waylog run`.
    fn run_command(&self) -> Option<&str> {
        None
    }
}
