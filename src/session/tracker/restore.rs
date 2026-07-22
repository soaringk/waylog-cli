use super::SessionState;
use crate::error::Result;
use std::collections::HashMap;
use tokio::fs;

/// Scan markdown files to restore session state
/// Returns a map of session_id -> SessionState
pub(crate) async fn restore_from_disk(
    history_dir: &std::path::Path,
    provider_name: &str,
) -> Result<HashMap<String, SessionState>> {
    if !history_dir.exists() {
        return Ok(HashMap::new());
    }

    let mut entries = fs::read_dir(history_dir).await?;
    let mut sessions_map = HashMap::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let Ok(frontmatter) = crate::exporter::parse_frontmatter(&path).await else {
            continue;
        };
        if frontmatter
            .provider
            .as_deref()
            .is_some_and(|name| name != provider_name)
        {
            continue;
        }
        let Some(session_id) = frontmatter.session_id else {
            continue;
        };
        sessions_map.insert(
            session_id,
            SessionState {
                markdown_path: path,
                synced_message_count: frontmatter.message_count.unwrap_or(0),
                include_tool_calls: frontmatter.include_tool_calls.unwrap_or(false),
            },
        );
    }

    Ok(sessions_map)
}
