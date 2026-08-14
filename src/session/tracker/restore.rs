use super::SessionState;
use crate::error::Result;
use crate::exporter::ExportFormat;
use std::collections::HashMap;
use tokio::fs;

/// Recover sync state for one provider from the exports already in a history directory.
/// Only exports written in the requested format are considered, so switching formats
/// leaves the other format's files untouched.
pub(crate) async fn restore_from_disk(
    history_dir: &std::path::Path,
    provider_name: &str,
    format: ExportFormat,
) -> Result<HashMap<String, SessionState>> {
    if !history_dir.exists() {
        return Ok(HashMap::new());
    }

    let mut entries = fs::read_dir(history_dir).await?;
    let mut sessions_map = HashMap::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(format.extension()) {
            continue;
        }
        let Ok(header) = crate::exporter::read_header(&path, format).await else {
            continue;
        };
        // Every export WayLog writes names its provider, so anything without a matching
        // one belongs to someone else and must never be adopted as an export path.
        if header.provider.as_deref() != Some(provider_name) {
            continue;
        }
        let Some(session_id) = header.session_id else {
            continue;
        };
        sessions_map.insert(
            session_id,
            SessionState {
                export_path: path,
                synced_message_count: header.message_count.unwrap_or(0),
                include_tool_calls: header.include_tool_calls.unwrap_or(false),
            },
        );
    }

    Ok(sessions_map)
}
