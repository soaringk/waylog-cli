use crate::error::Result;
use crate::providers::base::{ChatSession, Provider};
use crate::providers::claude::{main_session_files, parse_jsonl_session};
use crate::utils::path;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct QoderProvider {
    data_dir: Option<PathBuf>,
}

impl QoderProvider {
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
        self.data_dir
            .clone()
            .map_or_else(|| Ok(path::home_dir()?.join(".qoder").join("projects")), Ok)
    }

    fn session_dir(&self, project_path: &Path) -> Result<PathBuf> {
        Ok(self.data_dir()?.join(encode_project_path(project_path)))
    }
}

#[async_trait]
impl Provider for QoderProvider {
    fn name(&self) -> &str {
        "qoder"
    }

    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        let session_dir = self.session_dir(project_path)?;
        let direct = session_dir.join(format!("{session_id}.jsonl"));
        if direct.exists() {
            return Ok(Some(direct));
        }

        let transcript = session_dir
            .join("transcript")
            .join(format!("{session_id}.jsonl"));
        Ok(transcript.exists().then_some(transcript))
    }

    async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
        parse_jsonl_session(file_path, self.name()).await
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        let session_dir = self.session_dir(project_path)?;
        main_session_files(&[session_dir.clone(), session_dir.join("transcript")]).await
    }

    fn has_history(&self) -> bool {
        self.data_dir().is_ok_and(|directory| directory.exists())
    }
}

fn encode_project_path(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| match character {
            '/' | '\\' => '-',
            _ => character,
        })
        .collect()
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
    async fn finds_direct_and_transcript_sessions_for_dotted_project_path() {
        let temp = TempDir::new().unwrap();
        let project_path = PathBuf::from("/workspace/qoder.project");
        let provider = QoderProvider::with_data_dir(temp.path().to_path_buf());
        let session_dir = provider.session_dir(&project_path).unwrap();
        assert_eq!(session_dir.file_name().unwrap(), "-workspace-qoder.project");

        let direct = session_dir.join("direct.jsonl");
        let transcript = session_dir.join("transcript/transcript.jsonl");
        write_session(&direct, "direct", &project_path);
        write_session(&transcript, "transcript", &project_path);

        let sessions = provider.get_all_sessions(&project_path).await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&direct));
        assert!(sessions.contains(&transcript));
        assert_eq!(
            provider
                .find_session(&project_path, "transcript")
                .await
                .unwrap(),
            Some(transcript.clone())
        );

        let parsed = provider.parse_session(&transcript).await.unwrap();
        assert_eq!(parsed.provider, "qoder");
        assert_eq!(parsed.session_id, "transcript");
        assert_eq!(parsed.project_path, project_path);
        assert_eq!(parsed.messages.len(), 2);
    }
}
