use crate::synchronizer::{SyncStatus, Synchronizer};
use crate::{providers, session};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing;

/// Perform cleanup and final sync
///
/// This function handles:
/// - Stopping the file watcher
/// - Performing final sync of chat messages
///
/// Errors during cleanup are logged but don't prevent the function from completing.
pub(crate) async fn cleanup_and_sync(
    watcher_handle: &JoinHandle<()>,
    tracker: &Arc<session::SessionTracker>,
    provider: &Arc<dyn providers::base::Provider>,
    project_path: &std::path::Path,
    waylog_dir: &std::path::Path,
) {
    watcher_handle.abort();
    tracing::info!("Session ended, performing final sync...");

    let session_file = match provider.find_latest_session(project_path).await {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            tracing::error!("Failed to find final session: {}", error);
            return;
        }
    };
    let synchronizer = Synchronizer::new(
        provider.clone(),
        waylog_dir.to_path_buf(),
        tracker.clone(),
        false,
    );

    match synchronizer.sync_session(&session_file, false).await {
        Ok(SyncStatus::Synced { new_messages }) => {
            tracing::info!("✓ Final sync complete: {} messages", new_messages);
        }
        Ok(SyncStatus::Failed(error)) => tracing::error!("Final sync failed: {}", error),
        Err(error) => tracing::error!("Final sync failed: {}", error),
        Ok(SyncStatus::UpToDate | SyncStatus::Skipped) => {}
    }
}
