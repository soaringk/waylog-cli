use super::base::*;
use crate::error::{Result, WaylogError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

pub struct DshProvider;
impl DshProvider {
    pub fn new() -> Self {
        Self
    }
}
fn generation(name: &str) -> Option<u64> {
    let name = name.strip_suffix(".zstd").unwrap_or(name);
    if name == "session.jsonl" {
        return Some(0);
    }
    let version = name.strip_prefix("session.v")?.strip_suffix(".jsonl")?;
    if version.starts_with('0') || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    version
        .parse::<u64>()
        .ok()
        .filter(|v| *v <= 9_007_199_254_740_991)
}
fn time(v: &Value) -> Option<DateTime<Utc>> {
    v.as_i64().and_then(DateTime::from_timestamp_millis)
}
fn model(v: &Value) -> Option<String> {
    let m = v.get("model")?.as_str()?;
    Some(
        match v
            .get("provider")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            Some(p) => format!("{p}/{m}"),
            None => m.to_string(),
        },
    )
}
fn blocks(
    content: &Value,
    role: MessageRole,
    timestamp: Option<DateTime<Utc>>,
    model: Option<String>,
) -> Vec<ChatMessage> {
    let Some(items) = content.as_array() else {
        return Vec::new();
    };
    let exported: Vec<_> = items
        .iter()
        .filter(|v| v.get("text").and_then(Value::as_str).is_some() || is_tool_payload(v))
        .cloned()
        .collect();
    join_consecutive_text(exported, &["text"])
        .into_iter()
        .map(|item| {
            let mut message = if is_tool_payload(&item) {
                ChatMessage::tool(timestamp, item)
            } else {
                ChatMessage {
                    timestamp,
                    role: if item["type"] == "reasoning" {
                        MessageRole::Assistant(AssistantOutput::Reasoning)
                    } else {
                        role
                    },
                    content: item["text"].as_str().unwrap_or_default().to_string(),
                    metadata: MessageMetadata::default(),
                }
            };
            message.metadata.model = model.clone();
            message
        })
        .collect()
}
fn artifact_reader(path: &Path) -> Result<Box<dyn Read>> {
    let file = std::fs::File::open(path)?;
    Ok(if path.extension().is_some_and(|v| v == "zstd") {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        Box::new(file)
    })
}
fn session_header(path: &Path) -> Result<Value> {
    let mut reader = BufReader::new(artifact_reader(path)?);
    let mut line = String::new();
    while reader.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let event: Value = serde_json::from_str(&line)?;
            if event["type"] == "session" {
                return Ok(event);
            }
        }
        line.clear();
    }
    Err(WaylogError::Internal(
        "DSH session header is missing".into(),
    ))
}
fn read_events(path: &Path) -> Result<Vec<Value>> {
    let mut reader = artifact_reader(path)?;
    let mut bytes = Vec::new();
    let read = reader.read_to_end(&mut bytes);
    // Complete earlier frames survive a torn final compressed frame. Other decoder errors fail.
    if let Err(error) = read {
        if error.kind() != std::io::ErrorKind::UnexpectedEof || bytes.is_empty() {
            return Err(error.into());
        }
    }
    let text = String::from_utf8(bytes)
        .map_err(|e| WaylogError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    let mut events = Vec::new();
    for line in text.split_inclusive('\n') {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(event) => events.push(event),
            Err(e) if e.is_eof() && !line.ends_with('\n') => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(events)
}
#[async_trait]
impl Provider for DshProvider {
    fn name(&self) -> &str {
        "dsh"
    }
    fn has_history(&self) -> bool {
        crate::utils::path::home_dir().is_ok_and(|h| h.join(".dsh/sessions").is_dir())
    }
    async fn parse_session(&self, path: &Path) -> Result<ChatSession> {
        let events = read_events(path)?;
        let mut surface: Vec<(usize, &Value)> = Vec::new();
        let mut models = Vec::with_capacity(events.len());
        let mut request_model = None;
        for (index, event) in events.iter().enumerate() {
            if event["type"] == "request/header" {
                request_model = model(&event["data"]["header"]["config"]);
            }
            models.push(request_model.clone());
            if !matches!(
                event["type"].as_str(),
                Some(
                    "user/message"
                        | "assistant/message"
                        | "tool/result"
                        | "system/message"
                        | "developer/message"
                )
            ) {
                continue;
            }
            if event["surfaceOp"]["op"] == "replace" {
                let start = surface
                    .iter()
                    .position(|old| old.1["seq"] == event["surfaceOp"]["startSeq"]);
                let end = surface
                    .iter()
                    .position(|old| old.1["seq"] == event["surfaceOp"]["endSeq"]);
                match (start, end) {
                    (Some(start), Some(end)) if start <= end => {
                        surface.splice(start..=end, [(index, event)]);
                    }
                    _ => {
                        return Err(WaylogError::Internal(
                            "Invalid DSH surface replacement range".into(),
                        ))
                    }
                }
            } else {
                surface.push((index, event));
            }
        }
        let header = events
            .iter()
            .find(|v| v["type"] == "session")
            .ok_or_else(|| WaylogError::Internal("DSH session header is missing".into()))?;
        let id = header["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| WaylogError::Internal("DSH session ID is missing".into()))?;
        let mut messages = Vec::new();
        if header["origin"] != "subagent" {
            for (index, event) in surface {
                let data = &event["data"];
                let timestamp = time(&event["time"]);
                match event["type"].as_str() {
                    Some("system/message" | "developer/message") => messages.extend(blocks(
                        &data["message"]["content"],
                        MessageRole::System,
                        timestamp,
                        None,
                    )),
                    Some("user/message") => {
                        if data["source"].is_object() && data["source"]["kind"] != "user" {
                            continue;
                        }
                        messages.extend(blocks(
                            &data["content"],
                            MessageRole::User,
                            timestamp,
                            None,
                        ));
                    }
                    Some("assistant/message") => {
                        let native = &data["message"];
                        let mut output = blocks(
                            &native["content"],
                            MessageRole::Assistant(AssistantOutput::Message),
                            timestamp,
                            model(&native["source"]).or_else(|| models[index].clone()),
                        );
                        if let Some(first) = output.first_mut() {
                            if let (Some(input), Some(output)) = (
                                data["usage"]["inputTokens"].as_u64(),
                                data["usage"]["outputTokens"].as_u64(),
                            ) {
                                first.metadata.tokens = Some(TokenUsage {
                                    input: u32::try_from(input).map_err(|_| {
                                        WaylogError::Internal(
                                            "DSH input token count exceeds supported range".into(),
                                        )
                                    })?,
                                    output: u32::try_from(output).map_err(|_| {
                                        WaylogError::Internal(
                                            "DSH output token count exceeds supported range".into(),
                                        )
                                    })?,
                                    cached: u32::try_from(
                                        data["usage"]["cacheReadTokens"].as_u64().unwrap_or(0),
                                    )
                                    .map_err(|_| {
                                        WaylogError::Internal(
                                            "DSH cached token count exceeds supported range".into(),
                                        )
                                    })?,
                                });
                            }
                        }
                        messages.extend(output);
                    }
                    Some("tool/result") => {
                        let mut payload = data["message"].clone();
                        if let Some(object) = payload.as_object_mut() {
                            object.insert("type".into(), Value::String("tool_result".into()));
                            if let Some(id) = object.remove("toolCallId") {
                                object.insert("tool_use_id".into(), id);
                            }
                        }
                        messages.push(ChatMessage::tool(timestamp, payload));
                    }
                    _ => {}
                }
            }
        }
        let (first, last) = message_time_range(&messages);
        Ok(ChatSession {
            session_id: id.into(),
            provider: self.name().into(),
            project_path: header["cwd"]
                .as_str()
                .map(PathBuf::from)
                .unwrap_or_default(),
            started_at: time(&header["createdAt"]).or(first),
            updated_at: last,
            messages,
        })
    }
    async fn select_source_paths(&self, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
        selected_generations(paths)
    }
    async fn get_all_sessions(&self, project: &Path) -> Result<Vec<PathBuf>> {
        let root = crate::utils::path::home_dir()?.join(".dsh/sessions");
        discover(&root, project).await
    }
}
async fn discover(root: &Path, project: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut supplied = Vec::new();
    for entry in walkdir::WalkDir::new(root).min_depth(3).max_depth(3) {
        let entry = entry.map_err(|e| WaylogError::Io(std::io::Error::other(e)))?;
        if entry.file_type().is_file() {
            supplied.push(entry.into_path());
        }
    }
    let chosen = selected_generations(supplied)?;
    let mut paths = Vec::new();
    for path in chosen {
        let header = match session_header(&path) {
            Ok(header) => header,
            Err(error) => {
                tracing::warn!(path=%path.display(),%error,"Skipping unreadable DSH discovery candidate");
                continue;
            }
        };
        if header["origin"] == "subagent" || header["cwd"].as_str().map(Path::new) != Some(project)
        {
            continue;
        }
        paths.push((std::fs::metadata(&path)?.modified()?, path));
    }
    paths.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    Ok(paths.into_iter().map(|(_, p)| p).collect())
}
fn selected_generations(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut chosen: HashMap<PathBuf, (u64, std::time::SystemTime, PathBuf)> = HashMap::new();
    for path in paths {
        let Some(version) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(generation)
        else {
            continue;
        };
        let rank = (version, std::fs::metadata(&path)?.modified()?, path.clone());
        let parent = path.parent().unwrap_or(Path::new("")).to_path_buf();
        if chosen.get(&parent).is_none_or(|old| rank > *old) {
            chosen.insert(parent, rank);
        }
    }
    let mut selected: Vec<_> = chosen.into_values().map(|(_, _, path)| path).collect();
    selected.sort();
    Ok(selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[tokio::test]
    async fn preserves_assembled_blocks_and_compressed_generations() {
        let dir = tempfile::tempdir().unwrap();
        let sessiondir = dir.path().join("project/session-id");
        std::fs::create_dir_all(&sessiondir).unwrap();
        let events = vec![
            json!({"type":"session","id":"own","cwd":"/project","createdAt":1}),
            json!({"type":"user/message","time":2,"data":{"content":[{"type":"text","text":"question  "}]}}),
            json!({"type":"assistant/message","time":3,"data":{"usage":{"inputTokens":10,"outputTokens":3,"cacheReadTokens":5},"message":{"source":{"provider":"deepseek","model":"v4"},"content":[{"type":"reasoning","text":"think"},{"type":"text","text":"answer  "},{"type":"tool-call","id":"c","name":"read"}]}}}),
        ];
        let text = events
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(sessiondir.join("session.jsonl"), "obsolete").unwrap();
        let path = sessiondir.join("session.v4.jsonl.zstd");
        std::fs::write(&path, zstd::stream::encode_all(text.as_bytes(), 1).unwrap()).unwrap();
        let provider = DshProvider::new();
        let s = provider.parse_session(&path).await.unwrap();
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.messages[1].metadata.tokens.as_ref().unwrap().cached, 5);
        assert!(s.messages[2].metadata.tokens.is_none());
        assert_eq!(s.messages[0].content, "question  ");
        assert_eq!(s.messages[2].content, "answer  ");
        assert_eq!(s.messages[2].metadata.model.as_deref(), Some("deepseek/v4"));
        assert_eq!(
            discover(dir.path(), Path::new("/project")).await.unwrap(),
            vec![path]
        );
    }
    #[tokio::test]
    async fn replacement_replays_surface_order_and_pairs_native_result() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.jsonl");
        let events = vec![
            json!({"type":"session","id":"id"}),
            json!({"type":"user/message","seq":0,"data":{"content":[{"type":"text","text":"old"}]}}),
            json!({"type":"assistant/message","seq":1,"data":{"message":{"content":[{"type":"tool-call","id":"call","name":"read","arguments":"{}"}]}}}),
            json!({"type":"tool/result","seq":2,"data":{"message":{"id":"result-message","toolCallId":"call","content":[{"type":"text","text":"result"}]}}}),
            json!({"type":"user/message","seq":3,"surfaceOp":{"op":"replace","startSeq":0,"endSeq":0},"data":{"content":[{"type":"text","text":"replacement"}]}}),
        ];
        std::fs::write(
            &file,
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let session = DshProvider::new().parse_session(&file).await.unwrap();
        assert_eq!(session.messages[0].content, "replacement");
        assert_eq!(
            session.messages[1].metadata.tool_call_id.as_deref(),
            Some("call")
        );
        assert_eq!(
            session.messages[2].metadata.tool_call_id.as_deref(),
            Some("call")
        );
    }
    #[tokio::test]
    async fn discovery_ignores_children_and_unrelated_corruption() {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [
            (
                "valid",
                json!({"type":"session","id":"valid","cwd":"/project"}).to_string(),
            ),
            (
                "child",
                json!({"type":"session","id":"child","cwd":"/project","origin":"subagent"})
                    .to_string(),
            ),
            ("broken", "garbage".into()),
        ] {
            let path = dir.path().join("bucket").join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("session.jsonl"), text).unwrap();
        }
        let selected = discover(dir.path(), Path::new("/project")).await.unwrap();
        assert_eq!(
            selected,
            vec![dir.path().join("bucket/valid/session.jsonl")]
        );
    }
    #[test]
    fn canonical_generation_names() {
        assert_eq!(generation("session.v12.jsonl.zstd"), Some(12));
        for n in [
            "session.v0.jsonl",
            "session.v01.jsonl",
            "session.v9007199254740992.jsonl",
        ] {
            assert_eq!(generation(n), None);
        }
    }
}
