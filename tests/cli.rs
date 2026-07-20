use std::path::Path;
use std::process::{Command, Output};

fn run_waylog(current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_waylog"))
        .args(args)
        .current_dir(current_dir)
        .output()
        .expect("waylog should run")
}

#[test]
fn help_and_version_succeed() {
    let current_dir = tempfile::tempdir().unwrap();

    assert!(run_waylog(current_dir.path(), &["--help"]).status.success());
    assert!(run_waylog(current_dir.path(), &["--version"])
        .status
        .success());
}

#[test]
fn command_errors_return_usage_exit_code() {
    let current_dir = tempfile::tempdir().unwrap();

    for args in [
        &["run"][..],
        &["run", "nonexistent_agent_xyz"],
        &["pull", "--provider", "invalid_provider"],
    ] {
        assert_eq!(run_waylog(current_dir.path(), args).status.code(), Some(64));
    }
}

#[test]
fn missing_agent_error_is_not_duplicated() {
    let current_dir = tempfile::tempdir().unwrap();
    let output = run_waylog(current_dir.path(), &["run"]);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(stderr.matches("Missing required argument").count(), 1);
}

#[test]
fn json_error_output_is_valid() {
    let current_dir = tempfile::tempdir().unwrap();
    let output = run_waylog(current_dir.path(), &["--output", "json", "run"]);

    assert_eq!(output.status.code(), Some(64));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["level"], "error");
    assert_eq!(value["message"], "Missing required argument <AGENT>");
}
