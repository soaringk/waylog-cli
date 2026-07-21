use crate::error::Result;
use crate::providers::base::{ChatSession, Provider};
use crate::providers::claude::{main_session_files, parse_jsonl_session};
use crate::utils::path;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;

pub struct QoderWorkProvider {
    data_dir: Option<PathBuf>,
}

impl QoderWorkProvider {
    pub fn new() -> Self {
        Self { data_dir: None }
    }

    #[cfg(test)]
    fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
        }
    }

    fn data_dir(&self) -> Result<PathBuf> {
        self.data_dir.clone().map_or_else(
            || Ok(path::home_dir()?.join(".qoderwork").join("projects")),
            Ok,
        )
    }

    async fn session_directories(&self) -> Result<Vec<PathBuf>> {
        let data_dir = self.data_dir()?;
        if !data_dir.exists() {
            return Ok(Vec::new());
        }

        let mut directories = Vec::new();
        let mut entries = fs::read_dir(data_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                directories.push(entry.path());
            }
        }
        Ok(directories)
    }
}

#[async_trait]
impl Provider for QoderWorkProvider {
    fn name(&self) -> &str {
        "qoderwork"
    }

    fn is_project_scoped(&self) -> bool {
        false
    }

    async fn find_session(
        &self,
        _project_path: &Path,
        session_id: &str,
    ) -> Result<Option<PathBuf>> {
        for directory in self.session_directories().await? {
            let session_path = directory.join(format!("{session_id}.jsonl"));
            if session_path.is_file() {
                return Ok(Some(session_path));
            }
        }
        Ok(None)
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        parse_jsonl_session(file_path, self.name()).await
    }

    async fn get_all_sessions(&self, _project_path: &Path) -> Result<Vec<PathBuf>> {
        main_session_files(&self.session_directories().await?).await
    }

    fn has_history(&self) -> bool {
        self.data_dir().is_ok_and(|directory| directory.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn write_session(path: &Path, session_id: &str, project_path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let events = [
            json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": project_path,
                "timestamp": "2026-07-20T07:00:00.000Z",
                "uuid": "user-1",
                "isSidechain": false,
                "message": {"role": "user", "content": [{"type": "text", "text": "hello"}]}
            }),
            json!({
                "type": "assistant",
                "sessionId": session_id,
                "cwd": project_path,
                "timestamp": "2026-07-20T07:00:01.000Z",
                "uuid": "assistant-1",
                "isSidechain": false,
                "message": {"role": "assistant", "content": [{"type": "text", "text": "hi"}], "model": "test-model"}
            }),
        ];
        fs::write(
            path,
            events
                .into_iter()
                .map(|event| event.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn discovers_global_direct_sessions_and_ignores_nested_files() {
        let temp = TempDir::new().unwrap();
        let provider = QoderWorkProvider::with_data_dir(temp.path().to_path_buf());
        let workspace_a = temp.path().join("internal-workspace-a");
        let workspace_b = temp.path().join("legacy-project-b");
        let direct_a = workspace_a.join("session-a.jsonl");
        let direct_b = workspace_b.join("session-b.jsonl");
        let nested = workspace_a.join("subagents/sidechain.jsonl");
        write_session(&direct_a, "session-a", Path::new("/internal/a"));
        write_session(&direct_b, "session-b", Path::new("/legacy/b"));
        write_session(&nested, "sidechain", Path::new("/internal/a"));

        let sessions = provider
            .get_all_sessions(Path::new("/unrelated/project"))
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&direct_a));
        assert!(sessions.contains(&direct_b));
        assert!(!sessions.contains(&nested));
        assert!(!provider.is_project_scoped());
        assert_eq!(
            provider
                .find_session(Path::new("/another/project"), "session-b")
                .await
                .unwrap(),
            Some(direct_b.clone())
        );

        let parsed = provider.parse_session(&direct_b).await.unwrap();
        assert_eq!(parsed.provider, "qoderwork");
        assert_eq!(parsed.session_id, "session-b");
        assert_eq!(parsed.messages.len(), 2);
    }
}
