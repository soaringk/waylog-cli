use crate::error::{Result, WaylogError};
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct OpenCodeProvider {
    data_dir: Option<PathBuf>,
}

impl OpenCodeProvider {
    pub fn new() -> Self {
        Self { data_dir: None }
    }

    #[cfg(test)]
    fn with_data_dir(data_dir: PathBuf) -> Self {
        Self {
            data_dir: Some(data_dir),
        }
    }

    fn database_path(&self) -> Result<PathBuf> {
        Ok(self.data_dir()?.join("opencode.db"))
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open_with_flags(
            self.database_path()?,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sqlite_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(sqlite_error)?;
        Ok(connection)
    }

    fn session_path(&self, session_id: &str) -> Result<PathBuf> {
        Ok(self.data_dir()?.join(format!("{session_id}.session")))
    }

    fn session_id_from_path(session_path: &Path) -> Result<&str> {
        session_path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                WaylogError::PathError(format!(
                    "Invalid OpenCode session path: {}",
                    session_path.display()
                ))
            })
    }

    fn session_ids_for_project(&self, project_path: &Path) -> Result<Vec<String>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id FROM session WHERE directory = ?1 ORDER BY time_updated DESC, id DESC",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([project_path.to_string_lossy().as_ref()], |row| row.get(0))
            .map_err(sqlite_error)?;

        rows.collect::<std::result::Result<Vec<String>, _>>()
            .map_err(sqlite_error)
    }

    fn parse_message(
        &self,
        connection: &Connection,
        message_id: String,
        fallback_time: i64,
        data: String,
    ) -> Result<Option<ChatMessage>> {
        let info: Value = serde_json::from_str(&data)?;
        let role = match info.get("role").and_then(Value::as_str) {
            Some("user") => MessageRole::User,
            Some("assistant") => MessageRole::Assistant,
            _ => return Ok(None),
        };

        let mut statement = connection
            .prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY id")
            .map_err(sqlite_error)?;
        let parts = statement
            .query_map([&message_id], |row| row.get::<_, String>(0))
            .map_err(sqlite_error)?;

        let mut text = Vec::new();
        let mut thoughts = Vec::new();
        let mut tool_calls = Vec::new();

        for part_data in parts {
            let part: Value = serde_json::from_str(&part_data.map_err(sqlite_error)?)?;
            match part.get("type").and_then(Value::as_str) {
                Some("text") if part.get("ignored").and_then(Value::as_bool) != Some(true) => {
                    if let Some(value) = non_empty_string(part.get("text")) {
                        text.push(value.to_string());
                    }
                }
                Some("reasoning") => {
                    if let Some(value) = non_empty_string(part.get("text")) {
                        thoughts.push(value.to_string());
                    }
                }
                Some("tool") => {
                    if let Some(value) = non_empty_string(part.get("tool")) {
                        tool_calls.push(value.to_string());
                    }
                }
                _ => {}
            }
        }

        let content = text.join("\n\n");
        if content.is_empty() {
            return Ok(None);
        }

        let timestamp_ms = info
            .pointer("/time/created")
            .and_then(Value::as_i64)
            .unwrap_or(fallback_time);
        let timestamp = datetime_from_millis(timestamp_ms)?;
        let model_id = info
            .pointer("/model/modelID")
            .or_else(|| info.get("modelID"))
            .and_then(Value::as_str);
        let provider_id = info
            .pointer("/model/providerID")
            .or_else(|| info.get("providerID"))
            .and_then(Value::as_str);
        let model = match (provider_id, model_id) {
            (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
            (_, Some(model)) => Some(model.to_string()),
            _ => None,
        };
        let tokens = info.get("tokens").and_then(|tokens| {
            let input = json_u32(tokens.get("input"));
            let output = json_u32(tokens.get("output"));
            let cached = json_u32(tokens.pointer("/cache/read"));
            (input > 0 || output > 0 || cached > 0).then_some(TokenUsage {
                input,
                output,
                cached,
            })
        });

        Ok(Some(ChatMessage {
            id: message_id,
            timestamp,
            role,
            content,
            metadata: MessageMetadata {
                model,
                tokens,
                tool_calls,
                thoughts,
            },
        }))
    }
}

#[async_trait]
impl Provider for OpenCodeProvider {
    fn name(&self) -> &str {
        "opencode"
    }

    fn data_dir(&self) -> Result<PathBuf> {
        if let Some(data_dir) = &self.data_dir {
            return Ok(data_dir.clone());
        }

        #[cfg(target_os = "windows")]
        return Ok(path::home_dir()?.join(".local/share/opencode"));

        #[cfg(not(target_os = "windows"))]
        if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(xdg_data_home).join("opencode"));
        }

        #[cfg(not(target_os = "windows"))]
        Ok(path::home_dir()?.join(".local/share/opencode"))
    }

    fn session_dir(&self, _project_path: &Path) -> Result<PathBuf> {
        self.data_dir()
    }

    async fn find_latest_session(&self, project_path: &Path) -> Result<Option<PathBuf>> {
        self.session_ids_for_project(project_path)?
            .into_iter()
            .next()
            .map(|session_id| self.session_path(&session_id))
            .transpose()
    }

    async fn find_session(&self, project_path: &Path, session_id: &str) -> Result<Option<PathBuf>> {
        let connection = self.connection()?;
        let found = connection
            .query_row(
                "SELECT id FROM session WHERE id = ?1 AND directory = ?2",
                [session_id, project_path.to_string_lossy().as_ref()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error)?;
        found
            .map(|found_id| self.session_path(&found_id))
            .transpose()
    }

    async fn parse_session(&self, session_path: &Path) -> Result<ChatSession> {
        let session_id = Self::session_id_from_path(session_path)?;
        let connection = self.connection()?;
        let session = connection
            .query_row(
                "SELECT directory, time_created, time_updated FROM session WHERE id = ?1",
                [session_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite_error)?
            .ok_or_else(|| {
                WaylogError::PathError(format!("OpenCode session not found: {session_id}"))
            })?;

        let mut statement = connection
            .prepare(
                "SELECT id, time_created, data FROM message \
                 WHERE session_id = ?1 ORDER BY time_created, id",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(sqlite_error)?;

        let mut messages = Vec::new();
        for row in rows {
            let (message_id, time_created, data) = row.map_err(sqlite_error)?;
            if let Some(message) =
                self.parse_message(&connection, message_id, time_created, data)?
            {
                messages.push(message);
            }
        }

        Ok(ChatSession {
            session_id: session_id.to_string(),
            provider: self.name().to_string(),
            project_path: PathBuf::from(session.0),
            started_at: datetime_from_millis(session.1)?,
            updated_at: datetime_from_millis(session.2)?,
            messages,
        })
    }

    async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
        self.session_ids_for_project(project_path)?
            .into_iter()
            .map(|session_id| self.session_path(&session_id))
            .collect()
    }

    fn is_installed(&self) -> bool {
        which::which(self.command()).is_ok()
    }

    fn command(&self) -> &str {
        "opencode"
    }
}

fn non_empty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn json_u32(value: Option<&Value>) -> u32 {
    value
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

fn datetime_from_millis(value: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(value).ok_or_else(|| {
        WaylogError::Internal(format!("Invalid OpenCode millisecond timestamp: {value}"))
    })
}

fn sqlite_error(error: rusqlite::Error) -> WaylogError {
    WaylogError::Internal(format!("OpenCode database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::TempDir;

    fn create_fixture(data_dir: &Path, project_path: &Path) {
        let connection = Connection::open(data_dir.join("opencode.db")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY,
                    directory TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    time_updated INTEGER NOT NULL
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL,
                    data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY,
                    message_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    data TEXT NOT NULL
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4)",
                params![
                    "ses_test",
                    project_path.to_string_lossy(),
                    1_700_000_000_000_i64,
                    1_700_000_002_000_i64
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                params!["msg_user", "ses_test", 1_700_000_000_000_i64, r#"{"role":"user","time":{"created":1700000000000},"model":{"providerID":"anthropic","modelID":"claude-test"}}"#],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                params!["msg_assistant", "ses_test", 1_700_000_001_000_i64, r#"{"role":"assistant","time":{"created":1700000001000},"providerID":"anthropic","modelID":"claude-test","tokens":{"input":12,"output":8,"cache":{"read":3,"write":0}}}"#],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
                params![
                    "prt_1",
                    "msg_user",
                    "ses_test",
                    r#"{"type":"text","text":"Implement the feature"}"#
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
                params![
                    "prt_2",
                    "msg_assistant",
                    "ses_test",
                    r#"{"type":"reasoning","text":"Inspect the existing design"}"#
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
                params!["prt_3", "msg_assistant", "ses_test", r#"{"type":"tool","tool":"read","callID":"call_1","state":{"status":"completed"}}"#],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
                params![
                    "prt_4",
                    "msg_assistant",
                    "ses_test",
                    r#"{"type":"text","text":"Done"}"#
                ],
            )
            .unwrap();
    }

    #[tokio::test]
    async fn finds_and_parses_one_sqlite_session() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("project");
        create_fixture(temp_dir.path(), &project_path);
        let provider = OpenCodeProvider::with_data_dir(temp_dir.path().to_path_buf());

        let session_path = provider
            .find_session(&project_path, "ses_test")
            .await
            .unwrap()
            .unwrap();
        let session = provider.parse_session(&session_path).await.unwrap();

        assert_eq!(session.session_id, "ses_test");
        assert_eq!(session.project_path, project_path);
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].content, "Implement the feature");
        assert_eq!(session.messages[1].content, "Done");
        assert_eq!(session.messages[1].metadata.tool_calls, vec!["read"]);
        assert_eq!(
            session.messages[1].metadata.thoughts,
            vec!["Inspect the existing design"]
        );
        assert_eq!(
            session.messages[1].metadata.model.as_deref(),
            Some("anthropic/claude-test")
        );
        let tokens = session.messages[1].metadata.tokens.as_ref().unwrap();
        assert_eq!((tokens.input, tokens.output, tokens.cached), (12, 8, 3));
    }

    #[tokio::test]
    async fn session_lookup_is_scoped_to_project_directory() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("project");
        create_fixture(temp_dir.path(), &project_path);
        let provider = OpenCodeProvider::with_data_dir(temp_dir.path().to_path_buf());

        assert!(provider
            .find_session(Path::new("/different/project"), "ses_test")
            .await
            .unwrap()
            .is_none());
    }
}
