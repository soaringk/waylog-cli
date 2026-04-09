use crate::error::{Result, WaylogError};
use crate::output::Output;
use crate::providers::base::Provider;
use crate::synchronizer::SyncStatus;
use crate::{providers, session, synchronizer};
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;
use walkdir::{DirEntry, WalkDir};

pub async fn handle_pull(
    provider_name: Option<String>,
    force: bool,
    recursive: bool,
    include_hidden: bool,
    verbose: bool,
    tracking_root: PathBuf,
    target_project_path: PathBuf,
    output: &mut Output,
) -> Result<()> {
    // 1. Validate provider first (before any other operations)
    // This ensures we catch invalid providers even if project is not initialized
    if let Some(ref name) = provider_name {
        match providers::get_provider(name) {
            Ok(_) => {} // Provider is valid, continue
            Err(WaylogError::ProviderNotFound(ref invalid_name)) => {
                output.unknown_provider(invalid_name)?;
                return Err(WaylogError::ProviderNotFound(name.clone()));
            }
            Err(e) => return Err(e),
        }
    }

    output.pull_start(&target_project_path, recursive, include_hidden)?;

    let project_paths = if recursive {
        collect_project_paths(&target_project_path, include_hidden)
    } else {
        vec![target_project_path.clone()]
    };

    // Filter providers
    let providers_to_sync = if let Some(name) = provider_name {
        vec![providers::get_provider(&name)?]
    } else {
        // Sync all known providers
        vec![
            providers::get_provider("claude")?,
            providers::get_provider("gemini")?,
            providers::get_provider("codex")?,
        ]
    };

    let mut total_synced = 0;
    let mut total_uptodate = 0;

    for provider in providers_to_sync {
        if !provider.is_installed() {
            debug!("Skipping {} (not installed)", provider.name());
            continue;
        }

        // Recursive mode intentionally aggregates all descendant sessions into the
        // resolved tracking root for this invocation. Nested `.waylog` projects are
        // not treated as separate sync targets unless the user runs `pull` there.
        let tracker =
            Arc::new(session::SessionTracker::new(tracking_root.clone(), provider.clone()).await?);
        let synchronizer = synchronizer::Synchronizer::new(
            provider.clone(),
            tracking_root.clone(),
            target_project_path.clone(),
            tracker.clone(),
        );

        let session_paths = collect_session_paths(provider.clone(), &project_paths).await?;

        match synchronizer.sync_paths(session_paths, force).await {
            Ok(results) => {
                // Print section header
                output.provider_header(provider.name(), results.len())?;

                let mut provider_uptodate = 0;
                let mut provider_synced = 0;
                let mut provider_skipped = 0;
                let mut _provider_failed = 0;

                for (path, status) in results {
                    let filename = path.file_name().unwrap_or_default().to_string_lossy();
                    match status {
                        SyncStatus::Synced { new_messages } => {
                            output.synced(&filename, new_messages, verbose)?;
                            provider_synced += 1;
                        }
                        SyncStatus::UpToDate => {
                            output.up_to_date(&filename, verbose)?;
                            provider_uptodate += 1;
                        }
                        SyncStatus::Failed(e) => {
                            output.failed(&filename, &e.to_string())?;
                            _provider_failed += 1;
                        }
                        SyncStatus::Skipped => {
                            output.skipped(&filename, verbose)?;
                            provider_skipped += 1;
                        }
                    }
                }

                if !verbose {
                    output.summary_compact(provider_synced, provider_uptodate)?;
                }
                if verbose && provider_skipped > 0 {
                    output.skipped(&format!("{} sessions", provider_skipped), verbose)?;
                }

                total_synced += provider_synced;
                total_uptodate += provider_uptodate;
            }
            Err(e) => {
                tracing::error!("Failed to scan {}: {}", provider.name(), e);
            }
        }

        // Save state after each provider
        tracker.save_state().await?;
    }

    output.summary(total_synced, total_uptodate)?;

    Ok(())
}

fn collect_project_paths(root: &Path, include_hidden: bool) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| should_visit_directory(entry, include_hidden));

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                debug!("Skipping unreadable path during recursive pull: {}", err);
                continue;
            }
        };

        if entry.file_type().is_dir() {
            paths.push(entry.into_path());
        }
    }

    paths
}

fn should_visit_directory(entry: &DirEntry, include_hidden: bool) -> bool {
    if entry.depth() == 0 {
        return true;
    }

    if !entry.file_type().is_dir() {
        return true;
    }

    let name = entry.file_name().to_string_lossy();
    if name == crate::init::WAYLOG_DIR {
        return false;
    }

    include_hidden || !name.starts_with('.')
}

async fn collect_session_paths(
    provider: Arc<dyn Provider>,
    project_paths: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let mut session_paths = Vec::new();
    let mut seen = HashSet::new();

    for project_path in project_paths {
        for session_path in provider.get_all_sessions(project_path).await? {
            if seen.insert(session_path.clone()) {
                session_paths.push(session_path);
            }
        }
    }

    Ok(session_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{
        ChatMessage, ChatSession, MessageMetadata, MessageRole, Provider,
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    struct MockProvider {
        sessions_by_project: HashMap<PathBuf, Vec<PathBuf>>,
        installed: bool,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn data_dir(&self) -> Result<PathBuf> {
            Ok(std::env::temp_dir())
        }

        fn session_dir(&self, _project_path: &Path) -> Result<PathBuf> {
            Ok(std::env::temp_dir())
        }

        async fn find_latest_session(&self, _project_path: &Path) -> Result<Option<PathBuf>> {
            Ok(None)
        }

        async fn parse_session(&self, file_path: &Path) -> Result<ChatSession> {
            Ok(ChatSession {
                session_id: file_path.display().to_string(),
                provider: self.name().to_string(),
                project_path: PathBuf::from("/project"),
                started_at: Utc::now(),
                updated_at: Utc::now(),
                messages: vec![ChatMessage {
                    id: "msg-1".to_string(),
                    timestamp: Utc::now(),
                    role: MessageRole::User,
                    content: "hello".to_string(),
                    metadata: MessageMetadata::default(),
                }],
            })
        }

        async fn get_all_sessions(&self, project_path: &Path) -> Result<Vec<PathBuf>> {
            Ok(self
                .sessions_by_project
                .get(project_path)
                .cloned()
                .unwrap_or_default())
        }

        fn is_installed(&self) -> bool {
            self.installed
        }

        fn command(&self) -> &str {
            "mock"
        }
    }

    #[test]
    fn collect_project_paths_skips_hidden_directories_by_default() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("visible").join("nested")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::create_dir_all(root.join(".waylog").join("history")).unwrap();

        let paths = collect_project_paths(root, false);

        assert!(paths.contains(&root.to_path_buf()));
        assert!(paths.contains(&root.join("visible")));
        assert!(paths.contains(&root.join("visible").join("nested")));
        assert!(!paths.contains(&root.join(".hidden")));
        assert!(!paths.contains(&root.join(".waylog")));
    }

    #[test]
    fn collect_project_paths_includes_hidden_directories_except_waylog() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::create_dir_all(root.join("visible")).unwrap();
        fs::create_dir_all(root.join(".hidden").join("nested")).unwrap();
        fs::create_dir_all(root.join(".waylog").join("history")).unwrap();

        let paths = collect_project_paths(root, true);

        assert!(paths.contains(&root.to_path_buf()));
        assert!(paths.contains(&root.join("visible")));
        assert!(paths.contains(&root.join(".hidden")));
        assert!(paths.contains(&root.join(".hidden").join("nested")));
        assert!(!paths.contains(&root.join(".waylog")));
        assert!(!paths.contains(&root.join(".waylog").join("history")));
    }

    #[test]
    fn collect_project_paths_keeps_nested_projects_but_skips_their_waylog_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let nested_project = root.join("nested-project");
        let nested_waylog = nested_project.join(".waylog");

        fs::create_dir_all(nested_waylog.join("history")).unwrap();
        fs::create_dir_all(nested_project.join("src")).unwrap();

        let paths = collect_project_paths(root, false);

        assert!(paths.contains(&root.to_path_buf()));
        assert!(paths.contains(&nested_project));
        assert!(paths.contains(&nested_project.join("src")));
        assert!(!paths.contains(&nested_waylog));
        assert!(!paths.contains(&nested_waylog.join("history")));
    }

    #[tokio::test]
    async fn collect_session_paths_deduplicates_across_project_paths() {
        let temp_dir = TempDir::new().unwrap();
        let project_a = temp_dir.path().join("a");
        let project_b = temp_dir.path().join("b");
        let shared = temp_dir.path().join("shared.jsonl");
        let unique = temp_dir.path().join("unique.jsonl");

        let provider = Arc::new(MockProvider {
            sessions_by_project: HashMap::from([
                (project_a.clone(), vec![shared.clone(), unique.clone()]),
                (project_b.clone(), vec![shared.clone()]),
            ]),
            installed: true,
        });

        let sessions = collect_session_paths(provider, &[project_a, project_b])
            .await
            .unwrap();

        assert_eq!(sessions, vec![shared, unique]);
    }
}
