use crate::error::Result;
use crate::synchronizer::resolve_session_markdown_path;
use crate::{exporter, providers, session};
use std::sync::Arc;
use tokio::process::Child;
use tokio::task::JoinHandle;
use tracing;

/// Perform cleanup and final sync
///
/// This function handles:
/// - Stopping the file watcher
/// - Performing final sync of chat messages
/// - Saving session state
///
/// Errors during cleanup are logged but don't prevent the function from completing.
pub(crate) async fn cleanup_and_sync(
    watcher_handle: &JoinHandle<()>,
    _child: &mut Child,
    tracker: &Arc<session::SessionTracker>,
    provider: &Arc<dyn providers::base::Provider>,
    project_path: &std::path::Path,
    waylog_dir: &std::path::Path,
    _exit_status: Option<std::process::ExitStatus>,
) -> Result<()> {
    // Stop the file watcher
    watcher_handle.abort();
    // Wait a bit for the watcher to stop (non-blocking, ignore result)
    // Note: JoinHandle is not Copy, so we can't await the reference directly
    // Just abort is sufficient, the task will be cleaned up

    // Do a final sync
    tracing::info!("Session ended, performing final sync...");

    if let Ok(Some(session_file)) = provider.find_latest_session(project_path).await {
        if let Ok((session, new_messages)) = tracker.get_new_messages(&session_file).await {
            if !new_messages.is_empty() {
                tracing::info!("Syncing {} final messages", new_messages.len());

                let markdown_path =
                    if let Some(existing) = tracker.get_markdown_path(&session.session_id).await {
                        existing
                    } else {
                        resolve_session_markdown_path(waylog_dir, &session, provider.name()).await?
                    };

                // Rewrite from provider source so frontmatter remains authoritative.
                if let Err(e) = exporter::create_markdown_file(&markdown_path, &session).await {
                    tracing::error!("Failed to write markdown file: {}", e);
                }

                if let Err(e) = tracker
                    .update_session(
                        session.session_id.clone(),
                        session_file,
                        markdown_path.clone(),
                        session.messages.len(),
                    )
                    .await
                {
                    tracing::error!("Failed to update session: {}", e);
                } else {
                    tracing::info!("✓ Final sync complete: {}", markdown_path.display());
                }
            }
        }
    }

    // Save final state - errors are logged but don't stop cleanup
    if let Err(e) = tracker.save_state().await {
        tracing::warn!("Failed to save state: {}", e);
    }

    Ok(())
}
