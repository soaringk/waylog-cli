use crate::error::{Result, WaylogError};
use crate::providers::base::*;
use crate::utils::path;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct CursorProvider;
impl CursorProvider {
    pub fn new() -> Self {
        Self
    }
    fn database_path(&self) -> Result<PathBuf> {
        #[cfg(target_os = "macos")]
        let directory = path::home_dir()?.join("Library/Application Support");
        #[cfg(target_os = "windows")]
        let directory = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(path::home_dir()?.join("AppData/Roaming"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let directory = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(path::home_dir()?.join(".config"));
        Ok(directory.join("Cursor/User/globalStorage/state.vscdb"))
    }
}
fn database_error(error: rusqlite::Error) -> WaylogError {
    WaylogError::Internal(format!("Cursor database: {error}"))
}
fn timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    value
        .as_i64()
        .and_then(DateTime::from_timestamp_millis)
        .or_else(|| {
            value
                .as_str()
                .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.with_timezone(&Utc))
        })
}
fn lexical_text(value: &Value) -> String {
    match value.get("type").and_then(Value::as_str) {
        Some("text") => value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        Some("linebreak") => "\n".into(),
        _ => value
            .get("children")
            .and_then(Value::as_array)
            .map(|children| children.iter().map(lexical_text).collect())
            .unwrap_or_default(),
    }
}
fn bubble_text(bubble: &Value) -> Result<String> {
    if let Some(text) = bubble
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        return Ok(text.into());
    }
    if let Some(rich) = bubble
        .get("richText")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        let rich: Value = serde_json::from_str(rich)?;
        return Ok(lexical_text(rich.get("root").unwrap_or(&rich)));
    }
    Ok(String::new())
}

#[async_trait]
impl Provider for CursorProvider {
    fn name(&self) -> &str {
        "cursor"
    }
    fn is_project_scoped(&self) -> bool {
        false
    }
    fn has_history(&self) -> bool {
        self.database_path().is_ok_and(|file| file.is_file())
    }
    async fn get_all_sessions(&self, _: &Path) -> Result<Vec<PathBuf>> {
        let file = self.database_path()?;
        Ok(if file.is_file() {
            vec![file]
        } else {
            Vec::new()
        })
    }
    async fn select_source_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        Ok(paths
            .into_iter()
            .filter(|path| {
                matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("vscdb" | "db" | "sqlite" | "sqlite3")
                )
            })
            .collect())
    }
    async fn parse_session(&self, file: &Path) -> Result<ChatSession> {
        let mut sessions = self.parse_sessions(file, None).await?;
        if sessions.len() != 1 {
            return Err(WaylogError::PathError("Cursor database requires batch import when it contains zero or multiple conversations".into()));
        }
        sessions.remove(0)
    }
    async fn parse_sessions(
        &self,
        file: &Path,
        session_id: Option<&str>,
    ) -> Result<Vec<Result<ChatSession>>> {
        let connection = Connection::open_with_flags(
            file,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(database_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(database_error)?;
        // One snapshot keeps headers and bubbles consistent while Cursor is writing.
        connection.execute_batch("BEGIN").map_err(database_error)?;
        let has_headers: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='composerHeaders')", [], |row| row.get(0)).map_err(database_error)?;
        let mut headers = BTreeMap::new();
        if has_headers {
            let mut columns_query = connection
                .prepare("PRAGMA table_info(composerHeaders)")
                .map_err(database_error)?;
            let columns = columns_query
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(database_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(database_error)?;
            let created = if columns.iter().any(|column| column == "createdAt") {
                "createdAt"
            } else {
                "NULL"
            };
            let updated = if columns.iter().any(|column| column == "lastUpdatedAt") {
                "lastUpdatedAt"
            } else {
                "NULL"
            };
            let mut query = connection.prepare(&format!("SELECT composerId, CAST(value AS TEXT), isSubagent, {created}, {updated} FROM composerHeaders")).map_err(database_error)?;
            let rows = query
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                })
                .map_err(database_error)?;
            for row in rows {
                let (id, raw, subagent, created, updated) = row.map_err(database_error)?;
                if session_id.is_none_or(|selected| selected == id) {
                    headers.insert(id, (raw, subagent, created, updated));
                }
            }
        }
        let mut states = BTreeMap::new();
        let mut state_query = connection.prepare("SELECT key, CAST(value AS TEXT) FROM cursorDiskKV WHERE key >= 'composerData:' AND key < 'composerData;'").map_err(database_error)?;
        let state_rows = state_query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(database_error)?;
        for row in state_rows {
            let (key, raw) = row.map_err(database_error)?;
            let id = &key["composerData:".len()..];
            if session_id.is_none_or(|selected| selected == id) {
                states.insert(id.to_string(), raw);
            }
        }
        let mut conversations: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        let mut query = connection.prepare("SELECT key, CAST(value AS TEXT) FROM cursorDiskKV WHERE key >= 'bubbleId:' AND key < 'bubbleId;' ORDER BY rowid").map_err(database_error)?;
        let rows = query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(database_error)?;
        for row in rows {
            let (key, raw) = row.map_err(database_error)?;
            let Some((id, bubble_id)) = key
                .strip_prefix("bubbleId:")
                .and_then(|key| key.split_once(':'))
            else {
                continue;
            };
            if session_id.is_none_or(|selected| selected == id) {
                conversations
                    .entry(id.into())
                    .or_default()
                    .push((bubble_id.into(), raw));
            }
        }
        let ids: BTreeSet<_> = conversations
            .keys()
            .chain(states.keys())
            .chain(headers.keys())
            .cloned()
            .collect();
        let mut sessions = Vec::new();
        for id in ids {
            let parsed = (|| -> Result<Option<ChatSession>> {
                let state = states
                    .get(&id)
                    .map(|raw| serde_json::from_str::<Value>(raw))
                    .transpose()?;
                let state = state.as_ref();
                let header = headers
                    .get(&id)
                    .map(|(raw, subagent, created, updated)| -> Result<_> {
                        let mut metadata: Value = serde_json::from_str(raw)?;
                        if let Some(object) = metadata.as_object_mut() {
                            if let Some(created) = created {
                                object.insert("createdAt".into(), Value::from(*created));
                            }
                            if let Some(updated) = updated {
                                object.insert("lastUpdatedAt".into(), Value::from(*updated));
                            }
                        }
                        Ok((metadata, *subagent))
                    })
                    .transpose()?;
                let header = header.as_ref();
                if state.is_some_and(|state| {
                    state.get("isSubagent").and_then(Value::as_bool) == Some(true)
                }) || header.is_some_and(|(metadata, subagent)| {
                    *subagent || metadata.get("isSubagent").and_then(Value::as_bool) == Some(true)
                }) {
                    return Ok(None);
                }
                let mut bubbles = conversations
                    .remove(&id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(bubble_id, raw)| Ok((bubble_id, serde_json::from_str::<Value>(&raw)?)))
                    .collect::<Result<Vec<_>>>()?;
                if bubbles.is_empty() {
                    if let Some(legacy) = state
                        .and_then(|state| state.get("conversation"))
                        .and_then(Value::as_array)
                    {
                        bubbles = legacy
                            .iter()
                            .enumerate()
                            .map(|(index, bubble)| {
                                (
                                    bubble
                                        .get("bubbleId")
                                        .and_then(Value::as_str)
                                        .map(str::to_string)
                                        .unwrap_or_else(|| index.to_string()),
                                    bubble.clone(),
                                )
                            })
                            .collect();
                    }
                }
                if let Some(order) = state
                    .and_then(|state| state.get("fullConversationHeadersOnly"))
                    .and_then(Value::as_array)
                {
                    let positions: BTreeMap<&str, usize> = order
                        .iter()
                        .enumerate()
                        .filter_map(|(index, header)| {
                            header
                                .get("bubbleId")
                                .and_then(Value::as_str)
                                .map(|id| (id, index))
                        })
                        .collect();
                    bubbles.retain(|(id, _)| positions.contains_key(id.as_str()));
                    bubbles.sort_by_key(|(id, _)| {
                        positions.get(id.as_str()).copied().unwrap_or(usize::MAX)
                    });
                }
                let mut messages = Vec::new();
                let mut model = None;
                for (_, bubble) in bubbles {
                    let role = match bubble.get("type").and_then(Value::as_u64) {
                        Some(1) => MessageRole::User,
                        Some(2) => MessageRole::Assistant(AssistantOutput::Message),
                        _ => continue,
                    };
                    if role == MessageRole::User {
                        model = bubble
                            .pointer("/modelInfo/modelName")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    } else if let Some(recorded) = bubble
                        .pointer("/modelInfo/modelName")
                        .and_then(Value::as_str)
                    {
                        model = Some(recorded.into());
                    }
                    let time = timestamp(bubble.get("createdAt"));
                    let tokens = bubble.get("tokenCount").and_then(|count| {
                        let input = count
                            .get("inputTokens")
                            .and_then(Value::as_u64)
                            .and_then(|value| u32::try_from(value).ok())
                            .unwrap_or(0);
                        let output = count
                            .get("outputTokens")
                            .and_then(Value::as_u64)
                            .and_then(|value| u32::try_from(value).ok())
                            .unwrap_or(0);
                        (input != 0 || output != 0).then_some(TokenUsage {
                            input,
                            output,
                            cached: 0,
                        })
                    });
                    let mut metadata = MessageMetadata {
                        model: model.clone(),
                        tokens,
                        ..Default::default()
                    };
                    if role != MessageRole::User {
                        if let Some(thinking) = bubble
                            .pointer("/thinking/text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            let mut message = ChatMessage::reasoning(time, thinking.into());
                            message.metadata = metadata.clone();
                            metadata.tokens = None;
                            messages.push(message);
                        }
                    }
                    let content = bubble_text(&bubble)?;
                    if !content.is_empty() {
                        messages.push(ChatMessage {
                            timestamp: time,
                            role,
                            content,
                            metadata: metadata.clone(),
                        });
                        metadata.tokens = None;
                    }
                    if role != MessageRole::User {
                        if let Some(native) = bubble
                            .get("toolFormerData")
                            .and_then(Value::as_object)
                            .filter(|tool| !tool.is_empty())
                        {
                            let mut call = native.clone();
                            let result = call.remove("result");
                            let call_id = native.get("toolCallId").and_then(Value::as_str);
                            if let Some(call_id) = call_id {
                                call.insert("call_id".into(), Value::String(call_id.into()));
                            }
                            let mut message = ChatMessage::tool(time, Value::Object(call));
                            message.metadata.model = model.clone();
                            message.metadata.tokens = metadata.tokens.clone();
                            messages.push(message);
                            if matches!(
                                native.get("status").and_then(Value::as_str),
                                Some("completed" | "error")
                            ) {
                                if let Some(result) = result {
                                    let mut payload = serde_json::json!({"content": result});
                                    if let Some(call_id) = call_id {
                                        payload["tool_use_id"] = Value::String(call_id.into());
                                    }
                                    if native.get("status").and_then(Value::as_str) == Some("error")
                                    {
                                        payload["is_error"] = Value::Bool(true);
                                    }
                                    let mut message = ChatMessage::tool(None, payload);
                                    message.metadata.model = model.clone();
                                    messages.push(message);
                                }
                            }
                        }
                    }
                }
                let (first, last) = message_time_range(&messages);
                let metadata = header.map(|(metadata, _)| metadata).or(state);
                Ok(Some(ChatSession {
                    session_id: id.clone(),
                    provider: self.name().into(),
                    project_path: metadata
                        .and_then(|metadata| metadata.pointer("/workspaceIdentifier/uri/fsPath"))
                        .and_then(Value::as_str)
                        .map(PathBuf::from)
                        .unwrap_or_default(),
                    started_at: timestamp(metadata.and_then(|metadata| metadata.get("createdAt")))
                        .or(first),
                    updated_at: timestamp(
                        metadata.and_then(|metadata| metadata.get("lastUpdatedAt")),
                    )
                    .or(last),
                    messages,
                }))
            })();
            match parsed {
                Ok(Some(session)) => sessions.push(Ok(session)),
                Ok(None) => {}
                Err(error) => sessions.push(Err(WaylogError::Internal(format!(
                    "Cursor session {id}: {error}"
                )))),
            }
        }
        sessions.sort_by(|a, b| match (a, b) {
            (Ok(a), Ok(b)) => b
                .updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.session_id.cmp(&b.session_id)),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => std::cmp::Ordering::Equal,
        });
        Ok(sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(file: &Path, headers: bool) -> Connection {
        let connection = Connection::open(file).unwrap();
        connection
            .execute_batch("CREATE TABLE cursorDiskKV (key TEXT UNIQUE, value BLOB)")
            .unwrap();
        if headers {
            connection.execute_batch("CREATE TABLE composerHeaders (composerId TEXT, value TEXT, isSubagent INTEGER)").unwrap();
        }
        connection
    }
    fn insert(connection: &Connection, key: &str, bubble: Value) {
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES (?1,?2)",
                (key, bubble.to_string()),
            )
            .unwrap();
    }
    #[tokio::test]
    async fn imports_all_composers_read_only_preserving_text_models_and_missing_times() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("state.vscdb");
        let connection = fixture(&file, true);
        connection
            .execute(
                "INSERT INTO composerHeaders VALUES ('a',?1,0)",
                [r#"{"workspaceIdentifier":{"uri":{"fsPath":"/project"}}}"#],
            )
            .unwrap();
        connection
            .execute("INSERT INTO composerHeaders VALUES ('child','{}',1)", [])
            .unwrap();
        insert(
            &connection,
            "bubbleId:a:1",
            serde_json::json!({"type":1,"text":"request  ","createdAt":"2026-06-01T00:00:00Z","modelInfo":{"modelName":"selected"}}),
        );
        insert(
            &connection,
            "bubbleId:a:2",
            serde_json::json!({"type":2,"text":"answer","createdAt":"2026-06-01T00:00:01Z"}),
        );
        insert(
            &connection,
            "bubbleId:b:1",
            serde_json::json!({"type":1,"richText":r#"{"root":{"children":[{"type":"text","text":"rich  "},{"type":"linebreak"}]}}"#}),
        );
        insert(
            &connection,
            "bubbleId:child:1",
            serde_json::json!({"type":1,"text":"internal"}),
        );
        drop(connection);
        let before = std::fs::read(&file).unwrap();
        let provider = CursorProvider::new();
        let sessions = provider
            .parse_sessions(&file, None)
            .await
            .unwrap()
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "a");
        assert_eq!(sessions[0].project_path, PathBuf::from("/project"));
        assert_eq!(sessions[0].messages[0].content, "request  ");
        assert_eq!(
            sessions[0].messages[1].metadata.model.as_deref(),
            Some("selected")
        );
        assert_eq!(sessions[1].messages[0].content, "rich  \n");
        assert!(sessions[1].messages[0].timestamp.is_none());
        assert!(provider.parse_session(&file).await.is_err());
        assert_eq!(before, std::fs::read(&file).unwrap());
    }
    #[tokio::test]
    async fn supports_database_without_optional_headers_and_does_not_create_missing_database() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("state.vscdb");
        let connection = fixture(&file, false);
        insert(
            &connection,
            "bubbleId:legacy:one",
            serde_json::json!({"type":1,"text":"legacy"}),
        );
        drop(connection);
        assert_eq!(
            CursorProvider::new()
                .parse_session(&file)
                .await
                .unwrap()
                .session_id,
            "legacy"
        );
        let missing = directory.path().join("missing.vscdb");
        assert!(CursorProvider::new()
            .parse_sessions(&missing, None)
            .await
            .is_err());
        assert!(!missing.exists());
    }
    #[tokio::test]
    async fn native_order_preserves_reasoning_and_tool_only_bubbles() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("state.vscdb");
        let connection = fixture(&file, false);
        insert(
            &connection,
            "composerData:a",
            serde_json::json!({"fullConversationHeadersOnly":[{"bubbleId":"user"},{"bubbleId":"thought"},{"bubbleId":"tool"},{"bubbleId":"answer"}]}),
        );
        insert(
            &connection,
            "bubbleId:a:answer",
            serde_json::json!({"type":2,"text":"done","createdAt":"2026-06-01T00:00:00Z"}),
        );
        insert(
            &connection,
            "bubbleId:a:tool",
            serde_json::json!({"type":2,"tokenCount":{"inputTokens":2,"outputTokens":4},"toolFormerData":{"name":"read_file_v2","toolCallId":"native-id","params":"{\"targetFile\":\"a.txt\"}","status":"completed","result":"contents  "}}),
        );
        insert(
            &connection,
            "bubbleId:a:thought",
            serde_json::json!({"type":2,"thinking":{"text":"reason  ","signature":"hidden"}}),
        );
        insert(
            &connection,
            "bubbleId:a:user",
            serde_json::json!({"type":1,"text":"read","modelInfo":{"modelName":"selected"},"createdAt":"2026-06-01T00:00:05Z"}),
        );
        drop(connection);
        let session = CursorProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages.len(), 5);
        assert_eq!(session.messages[0].content, "read");
        assert_eq!(
            session.messages[1].role,
            MessageRole::Assistant(AssistantOutput::Reasoning)
        );
        assert_eq!(session.messages[1].content, "reason  ");
        assert_eq!(
            session.messages[2].role,
            MessageRole::Assistant(AssistantOutput::Tool)
        );
        assert_eq!(
            session.messages[2].metadata.tool_call_id.as_deref(),
            Some("native-id")
        );
        assert_eq!(
            session.messages[2].metadata.tokens.as_ref().unwrap().output,
            4
        );
        assert_eq!(
            session.messages[3].metadata.tool_call_id.as_deref(),
            Some("native-id")
        );
        assert_eq!(session.messages[3].content, "contents  ");
        assert!(session.messages[3].timestamp.is_none());
        assert_eq!(session.messages[4].content, "done");
        assert_eq!(
            session.messages[4].metadata.model.as_deref(),
            Some("selected")
        );
    }
    #[tokio::test]
    async fn reads_committed_wal_and_excludes_sqlite_sidecars_from_directory_import() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("state.vscdb");
        let connection = fixture(&file, false);
        connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        insert(
            &connection,
            "bubbleId:wal:user",
            serde_json::json!({"type":1,"text":"committed in WAL"}),
        );
        let before = std::fs::read(&file).unwrap();
        let provider = CursorProvider::new();
        let session = provider.parse_session(&file).await.unwrap();
        assert_eq!(session.messages[0].content, "committed in WAL");
        assert_eq!(before, std::fs::read(&file).unwrap());
        let selected = provider
            .select_source_paths(vec![
                file.clone(),
                directory.path().join("state.vscdb-wal"),
                directory.path().join("state.vscdb-shm"),
            ])
            .await
            .unwrap();
        assert_eq!(selected, vec![file]);
        drop(connection);
    }
    #[tokio::test]
    async fn imports_inline_legacy_conversation_and_header_column_dates() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("state.vscdb");
        let connection = fixture(&file, true);
        connection.execute_batch("ALTER TABLE composerHeaders ADD COLUMN createdAt INTEGER; ALTER TABLE composerHeaders ADD COLUMN lastUpdatedAt INTEGER;").unwrap();
        connection
            .execute(
                "INSERT INTO composerHeaders VALUES ('legacy','{}',0,1000,2000)",
                [],
            )
            .unwrap();
        insert(
            &connection,
            "composerData:legacy",
            serde_json::json!({"conversation":[{"type":1,"text":"old request"},{"type":2,"text":"old answer"}]}),
        );
        drop(connection);
        let session = CursorProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[1].content, "old answer");
        assert_eq!(session.started_at.unwrap().timestamp_millis(), 1000);
        assert_eq!(session.updated_at.unwrap().timestamp_millis(), 2000);
    }
}
