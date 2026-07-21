use crate::error::Result;
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct CodexProvider;

impl CodexProvider {
    pub fn new() -> Self {
        Self
    }

    fn data_dir(&self) -> Result<PathBuf> {
        Ok(path::home_dir()?.join(".codex").join("sessions"))
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    async fn find_latest_session(&self, project_path: &Path) -> Result<Option<PathBuf>> {
        // For 'run' mode, only scan recent days (last 7 days) for performance
        let base_session_dir = self.data_dir()?;

        if !base_session_dir.exists() {
            return Ok(None);
        }

        let now = Utc::now();
        let mut candidates = Vec::new();

        // Check last 7 days
        for days_ago in 0..7 {
            let date = now - chrono::Duration::days(days_ago);
            let day_dir = base_session_dir
                .join(date.format("%Y").to_string())
                .join(date.format("%m").to_string())
                .join(date.format("%d").to_string());

            if !day_dir.exists() {
                continue;
            }

            // Scan this day's directory
            if let Ok(mut entries) = fs::read_dir(&day_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().and_then(|s| s.to_str()) == Some("jsonl")
                        && self
                            .probe_project_path(&path, project_path)
                            .await
                            .unwrap_or(false)
                    {
                        if let Ok(metadata) = fs::metadata(&path).await {
                            if let Ok(modified) = metadata.modified() {
                                candidates.push((path, modified));
                            }
                        }
                    }
                }
            }
        }

        // Sort by modification time, newest first
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

        Ok(candidates.into_iter().next().map(|(p, _)| p))
    }

    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        let base_session_dir = self.data_dir()?;
        if !base_session_dir.exists() {
            return Ok(None);
        }

        for entry in walkdir::WalkDir::new(base_session_dir) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                && (stem == session_id || stem.ends_with(session_id))
                && self.probe_project_path(path, project_path).await?
            {
                return Ok(Some(path.to_path_buf()));
            }
        }

        Ok(None)
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let base_session_dir = self.data_dir()?;

        if !base_session_dir.exists() {
            return Ok(Vec::new());
        }

        // Recursively find all .jsonl files in the base session directory
        let mut candidates = Vec::new();
        let walker = walkdir::WalkDir::new(&base_session_dir);

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                // Probe the file for project path match
                if self
                    .probe_project_path(path, project_path)
                    .await
                    .unwrap_or(false)
                {
                    if let Ok(metadata) = fs::metadata(path).await {
                        if let Ok(modified) = metadata.modified() {
                            candidates.push((path.to_path_buf(), modified));
                        }
                    }
                }
            }
        }

        // Sort by modification time, newest first
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.1));

        Ok(candidates.into_iter().map(|(p, _)| p).collect())
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        let file = fs::File::open(file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        let mut messages = Vec::new();
        let mut session_id = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut started_at = Utc::now();
        let mut session_project_path = PathBuf::new();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(event) = serde_json::from_str::<CodexEvent>(&line) {
                match event.event_type.as_str() {
                    "session_meta" => {
                        if let Some(payload) = event.payload {
                            if let Some(id) = payload.id.filter(|id| !id.is_empty()) {
                                session_id = id;
                            }
                            if let Some(cwd) = payload.cwd {
                                session_project_path = PathBuf::from(cwd);
                            }
                        }
                    }
                    "turn_context" => {
                        if let Some(cwd) = event.payload.and_then(|payload| payload.cwd) {
                            session_project_path = PathBuf::from(cwd);
                        }
                    }
                    "response_item" => {
                        if let Some(payload) = event.payload {
                            if let Some(msg) =
                                self.parse_response_item(payload, &event.timestamp)?
                            {
                                if messages.is_empty() {
                                    started_at = msg.timestamp;
                                }

                                // Simple deduplication
                                let is_duplicate =
                                    messages.last().is_some_and(|last: &ChatMessage| {
                                        last.role == msg.role && last.content == msg.content
                                    });
                                if !is_duplicate {
                                    messages.push(msg);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(ChatSession {
            session_id,
            provider: self.name().to_string(),
            project_path: session_project_path,
            started_at,
            updated_at: messages.last().map(|m| m.timestamp).unwrap_or(started_at),
            messages,
        })
    }

    fn has_history(&self) -> bool {
        self.data_dir().is_ok_and(|directory| directory.exists())
    }

    fn run_command(&self) -> Option<&str> {
        Some("codex")
    }
}

impl CodexProvider {
    async fn probe_project_path(
        &self,
        file_path: &Path,
        target_project_path: &Path,
    ) -> Result<bool> {
        let file = fs::File::open(file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // Scan first 50 lines (session_meta is usually first)
        let mut checked_lines = 0;
        while let Some(line) = lines.next_line().await? {
            if checked_lines >= 50 {
                break;
            }
            checked_lines += 1;

            if let Ok(event) = serde_json::from_str::<CodexEvent>(&line) {
                if let Some(cwd_str) = event.payload.and_then(|p| p.cwd) {
                    return Ok(paths_equivalent(Path::new(&cwd_str), target_project_path).await);
                }
            }
        }
        Ok(false)
    }

    fn parse_response_item(
        &self,
        payload: CodexPayload,
        timestamp: &str,
    ) -> Result<Option<ChatMessage>> {
        let role = match payload.role.as_deref() {
            Some("user") => MessageRole::User,
            Some("assistant") => MessageRole::Assistant,
            _ => return Ok(None),
        };

        // Extract text content
        let content = payload
            .content
            .and_then(|c| c.into_iter().find_map(|item| item.text))
            .unwrap_or_default();

        if content.is_empty() {
            return Ok(None);
        }

        let timestamp = DateTime::parse_from_rfc3339(timestamp)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        // Filter out system injections which Codex logs as "user" messages
        if role == MessageRole::User {
            // 1. Environment context
            if content.contains("<environment_context>") {
                return Ok(None);
            }
            // 2. AGENTS.md instructions
            if content.contains("<INSTRUCTIONS>") || content.contains("# AGENTS.md instructions") {
                return Ok(None);
            }
        }

        Ok(Some(ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp,
            role,
            content,
            metadata: MessageMetadata {
                model: None,
                tokens: None,
                tool_calls: Vec::new(),
                thoughts: Vec::new(),
            },
        }))
    }
}

// Codex JSONL event structures
#[derive(Debug, Deserialize)]
struct CodexEvent {
    #[serde(rename = "type")]
    event_type: String,
    timestamp: String,
    payload: Option<CodexPayload>,
}

#[derive(Debug, Deserialize)]
struct CodexPayload {
    id: Option<String>,
    role: Option<String>,
    cwd: Option<String>,
    content: Option<Vec<CodexContent>>,
}

#[derive(Debug, Deserialize)]
struct CodexContent {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    content_type: String,
    text: Option<String>,
}

async fn paths_equivalent(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    match (fs::canonicalize(left).await, fs::canonicalize(right).await) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn session_meta_line(session_id: &str, cwd: &Path) -> String {
        format!(
            r#"{{"type":"session_meta","timestamp":"2026-04-01T00:00:00Z","payload":{{"id":"{}","cwd":"{}"}}}}"#,
            session_id,
            cwd.display()
        )
    }

    #[tokio::test]
    async fn parses_canonical_session_id_and_requires_same_project() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("rollout-with-prefix.jsonl");
        let project_path = temp_dir.path().join("project");
        tokio::fs::create_dir(&project_path).await.unwrap();
        tokio::fs::write(
            &file_path,
            session_meta_line("canonical-session-id", &project_path),
        )
        .await
        .unwrap();

        let provider = CodexProvider::new();

        assert!(provider
            .probe_project_path(&file_path, &project_path)
            .await
            .unwrap());
        assert!(!provider
            .probe_project_path(&file_path, &project_path.join("nested"))
            .await
            .unwrap());
        assert_eq!(
            provider.parse_session(&file_path).await.unwrap().session_id,
            "canonical-session-id"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_project_path_accepts_symlink_equivalent_cwd() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let real_project = temp_dir.path().join("real-project");
        let linked_project = temp_dir.path().join("linked-project");
        let file_path = temp_dir.path().join("session.jsonl");
        tokio::fs::create_dir(&real_project).await.unwrap();
        symlink(&real_project, &linked_project).unwrap();
        tokio::fs::write(&file_path, session_meta_line("session-id", &linked_project))
            .await
            .unwrap();

        assert!(CodexProvider::new()
            .probe_project_path(&file_path, &real_project)
            .await
            .unwrap());
    }
}
