use crate::error::Result;
use crate::providers::base::Provider;
use crate::session::SessionTracker;
use crate::synchronizer::Synchronizer;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, info};

/// Sync interval in seconds
const SYNC_INTERVAL_SECS: u64 = 30;

/// Periodic sync watcher (simplified - no file watching)
pub struct FileWatcher {
    provider: Arc<dyn Provider>,
    target_project_dir: PathBuf,
    synchronizer: Synchronizer,
}

impl FileWatcher {
    pub fn new(
        provider: Arc<dyn Provider>,
        tracking_root: PathBuf,
        target_project_dir: PathBuf,
        tracker: Arc<SessionTracker>,
    ) -> Self {
        let synchronizer = Synchronizer::new(
            provider.clone(),
            tracking_root,
            target_project_dir.clone(),
            tracker.clone(),
        );

        Self {
            provider,
            target_project_dir,
            synchronizer,
        }
    }

    /// Start periodic sync loop
    pub async fn watch(&self) -> Result<()> {
        info!(
            "Starting periodic sync (every {} seconds)",
            SYNC_INTERVAL_SECS
        );

        let mut interval = time::interval(Duration::from_secs(SYNC_INTERVAL_SECS));

        loop {
            interval.tick().await;

            if let Err(e) = self.sync_latest().await {
                tracing::error!("Periodic sync error: {}", e);
            }
        }
    }

    /// Sync only the latest session
    async fn sync_latest(&self) -> Result<()> {
        // Find the latest session file
        let session_file = match self
            .provider
            .find_latest_session(&self.target_project_dir)
            .await?
        {
            Some(file) => file,
            None => {
                debug!("No session file found");
                return Ok(());
            }
        };

        // Use shared synchronizer logic
        self.synchronizer.sync_session(&session_file, false).await?;

        Ok(())
    }
}
