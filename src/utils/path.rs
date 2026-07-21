use crate::error::{Result, WaylogError};
use crate::init::{subdirs, WAYLOG_DIR};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Get the home directory in a cross-platform way
pub fn home_dir() -> Result<PathBuf> {
    home::home_dir()
        .ok_or_else(|| WaylogError::PathError("Could not find home directory".to_string()))
}

/// Get the data directory for AI tools
/// On Unix: ~/.{tool}
/// On Windows: %USERPROFILE%\.{tool} (future extension point)
pub fn get_ai_data_dir(tool_name: &str) -> Result<PathBuf> {
    let home = home_dir()?;

    #[cfg(target_os = "windows")]
    {
        // Windows: Use AppData\Local for application data (future extension)
        // For now, keep it simple and use home directory
        Ok(home.join(format!(".{}", tool_name)))
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Unix-like systems (macOS, Linux)
        Ok(home.join(format!(".{}", tool_name)))
    }
}

/// Encode a path for Claude Code (replace all non-alphanumeric chars with -)
/// Unix: /Users/name/project -> -Users-name-project
/// Windows: C:\Users\name\project -> C--Users-name-project
/// Non-ASCII: /Users/名字/project -> -Users----project
pub fn encode_path_claude(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    let normalized = path_str.replace('\\', "/");

    normalized
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Encode a path for Gemini (SHA-256 hash)
/// This is platform-independent as it hashes the string representation
/// Example: /home/user/project -> 9dad1e4e08b0b11cbcd860257e8bdfa6b8e5f01790e10a6a0b1f4870c13e686b
pub fn encode_path_gemini(path: &Path) -> String {
    // Use the canonical string representation for consistent hashing
    let path_str = path.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Get the .waylog/history directory for the current project
pub fn get_waylog_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(WAYLOG_DIR).join(subdirs::HISTORY)
}

/// Find the project root by looking for .waylog folder or .git folder
/// moving upwards from the current directory.
/// If we reach the home directory or the system root without finding a marker,
/// returns the current directory to avoid treat the whole home as a project.
pub fn find_project_root() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = home_dir().ok();

    find_project_root_from(&current_dir, home.as_deref())
}

fn find_project_root_from(current_dir: &Path, home: Option<&Path>) -> Option<PathBuf> {
    for path in current_dir.ancestors() {
        if path.join(WAYLOG_DIR).is_dir() {
            return Some(path.to_path_buf());
        }

        if home == Some(path) {
            break;
        }
    }

    None
}

/// Ensure a directory exists, creating it if necessary
pub fn ensure_dir_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_encode_path_claude_matches_storage_key() {
        assert_eq!(
            encode_path_claude(Path::new("/Users/名字/my project@")),
            "-Users----my-project-"
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_encode_path_claude_windows_absolute() {
        let path = Path::new("C:\\Users\\user\\project");
        assert_eq!(encode_path_claude(path), "C--Users-user-project");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_encode_path_claude_windows_relative() {
        let path = Path::new("project\\subdir");
        assert_eq!(encode_path_claude(path), "project-subdir");
    }

    #[test]
    fn test_encode_path_gemini_matches_storage_key() {
        assert_eq!(
            encode_path_gemini(Path::new("/home/user/project")),
            "9dad1e4e08b0b11cbcd860257e8bdfa6b8e5f01790e10a6a0b1f4870c13e686b"
        );
    }

    #[test]
    fn test_get_ai_data_dir_format() {
        assert_eq!(
            get_ai_data_dir("claude").unwrap(),
            home_dir().unwrap().join(".claude")
        );
    }

    #[test]
    fn test_get_waylog_dir() {
        let project_dir = std::env::temp_dir().join("test-project");
        let waylog_dir = get_waylog_dir(&project_dir);

        let expected = project_dir.join(".waylog").join("history");
        assert_eq!(waylog_dir, expected);
    }

    #[test]
    fn test_ensure_dir_exists() {
        let temp_dir = TempDir::new().unwrap();
        let new_dir = temp_dir.path().join("new-dir");

        // Should create directory if it doesn't exist
        assert!(!new_dir.exists());
        ensure_dir_exists(&new_dir).unwrap();
        assert!(new_dir.exists());
        assert!(new_dir.is_dir());

        // Should not error if directory already exists
        ensure_dir_exists(&new_dir).unwrap();
        assert!(new_dir.exists());

        // Test nested directory creation
        let nested_dir = temp_dir.path().join("a").join("b").join("c");
        ensure_dir_exists(&nested_dir).unwrap();
        assert!(nested_dir.exists());
        assert!(nested_dir.is_dir());
    }

    #[test]
    fn test_find_project_root_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let project_root = temp_dir.path().join("project");
        let subdir = project_root.join("subdir").join("deep");
        fs::create_dir_all(&subdir).unwrap();
        fs::create_dir_all(project_root.join(".waylog")).unwrap();

        assert_eq!(find_project_root_from(&subdir, None), Some(project_root));
        assert_eq!(find_project_root_from(temp_dir.path(), None), None);
    }
}
