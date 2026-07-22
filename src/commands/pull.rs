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

pub struct PullOptions {
    pub provider_name: Option<String>,
    pub force: bool,
    pub recursive: bool,
    pub include_hidden: bool,
    pub session_id: Option<String>,
    pub source: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub verbose: bool,
}

pub async fn handle_pull(
    options: PullOptions,
    project_path: PathBuf,
    output: &mut Output,
) -> Result<()> {
    let PullOptions {
        provider_name,
        force,
        recursive,
        include_hidden,
        session_id,
        source,
        output_dir,
        verbose,
    } = options;
    output.pull_start(&project_path, recursive, include_hidden)?;

    let project_paths = if recursive {
        collect_project_paths(&project_path, include_hidden)
    } else {
        vec![project_path.clone()]
    };
    let history_dir =
        output_dir.unwrap_or_else(|| crate::utils::path::get_waylog_dir(&project_path));

    let provider_was_selected = provider_name.is_some();
    let providers_to_sync = if let Some(name) = provider_name {
        vec![providers::get_provider(&name)?]
    } else {
        providers::list_providers()
            .into_iter()
            .map(providers::get_provider)
            .collect::<Result<Vec<_>>>()?
    };

    let mut total_synced = 0;
    let mut total_uptodate = 0;
    let mut total_tasks = 0;
    let mut total_failed = 0;

    for provider in providers_to_sync {
        if !provider_was_selected && !provider.has_history() {
            debug!("Skipping {} (no local history)", provider.name());
            continue;
        }

        let tracker = Arc::new(
            session::SessionTracker::new_with_history_dir(history_dir.clone(), provider.clone())
                .await?,
        );
        let synchronizer = synchronizer::Synchronizer::new_with_history_dir(
            provider.clone(),
            history_dir.clone(),
            project_path.clone(),
            tracker.clone(),
        );

        let session_paths = if let Some(source) = &source {
            collect_source_paths(source)?
        } else if let Some(session_id) = &session_id {
            let session_path = provider
                .find_session(&project_path, session_id)
                .await?
                .ok_or_else(|| WaylogError::SessionNotFound {
                    provider: provider.name().to_string(),
                    session_id: session_id.clone(),
                })?;
            vec![session_path]
        } else {
            collect_session_paths(provider.clone(), &project_paths).await?
        };

        let results = synchronizer
            .sync_paths(session_paths, force || source.is_some())
            .await?;
        total_tasks += results.len();

        output.provider_header(provider.name(), results.len())?;

        let mut provider_uptodate = 0;
        let mut provider_synced = 0;
        let mut provider_skipped = 0;

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
                    output.failed(&filename, &e)?;
                    total_failed += 1;
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

    if all_tasks_failed(total_tasks, total_failed) {
        return Err(WaylogError::AllSessionsFailed(total_failed));
    }

    output.summary(total_synced, total_uptodate, &history_dir)?;

    Ok(())
}

fn all_tasks_failed(total: usize, failed: usize) -> bool {
    total > 0 && failed == total
}

fn collect_source_paths(source: &Path) -> Result<Vec<PathBuf>> {
    if source.is_file() {
        return Ok(vec![source.to_path_buf()]);
    }
    if !source.is_dir() {
        return Err(WaylogError::PathError(format!(
            "Session source not found: {}",
            source.display()
        )));
    }

    let mut paths = Vec::new();
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(|error| WaylogError::PathError(error.to_string()))?;
        if entry.file_type().is_file() {
            paths.push(entry.into_path());
        }
    }
    paths.sort();

    if paths.is_empty() {
        return Err(WaylogError::PathError(format!(
            "Session source contains no files: {}",
            source.display()
        )));
    }
    Ok(paths)
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
    if !provider.is_project_scoped() {
        return match project_paths.first() {
            Some(project_path) => provider.get_all_sessions(project_path).await,
            None => Ok(Vec::new()),
        };
    }

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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    struct MockProvider {
        sessions_by_project: HashMap<PathBuf, Vec<PathBuf>>,
        history_available: bool,
        project_scoped: bool,
        get_all_calls: AtomicUsize,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn is_project_scoped(&self) -> bool {
            self.project_scoped
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
            self.get_all_calls.fetch_add(1, Ordering::Relaxed);
            Ok(self
                .sessions_by_project
                .get(project_path)
                .cloned()
                .unwrap_or_default())
        }

        fn has_history(&self) -> bool {
            self.history_available
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
            history_available: true,
            project_scoped: true,
            get_all_calls: AtomicUsize::new(0),
        });

        let sessions = collect_session_paths(provider.clone(), &[project_a, project_b])
            .await
            .unwrap();

        assert_eq!(sessions, vec![shared, unique]);
        assert_eq!(provider.get_all_calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn collect_session_paths_scans_global_provider_once() {
        let temp_dir = TempDir::new().unwrap();
        let project_a = temp_dir.path().join("a");
        let project_b = temp_dir.path().join("b");
        let session = temp_dir.path().join("global.jsonl");
        let provider = Arc::new(MockProvider {
            sessions_by_project: HashMap::from([(project_a.clone(), vec![session.clone()])]),
            history_available: true,
            project_scoped: false,
            get_all_calls: AtomicUsize::new(0),
        });

        let sessions = collect_session_paths(provider.clone(), &[project_a, project_b])
            .await
            .unwrap();

        assert_eq!(sessions, vec![session]);
        assert_eq!(provider.get_all_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pull_fails_only_when_every_attempted_session_failed() {
        assert!(all_tasks_failed(1, 1));
        assert!(all_tasks_failed(3, 3));
        assert!(!all_tasks_failed(3, 2));
        assert!(!all_tasks_failed(0, 0));
    }
}
