use std::path::Path;
use std::process::{Command, Output};

fn run_waylog(current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_waylog"))
        .args(args)
        .current_dir(current_dir)
        .output()
        .expect("waylog should run")
}

fn write_qoder_session(path: &Path, session_id: &str, project: &Path, response: &str) {
    let events = [
        serde_json::json!({
            "type": "user",
            "sessionId": session_id,
            "cwd": project,
            "timestamp": "2026-07-20T07:00:00.000Z",
            "uuid": format!("{session_id}-user"),
            "message": {"role": "user", "content": [{"type": "text", "text": "hello"}]}
        }),
        serde_json::json!({
            "type": "assistant",
            "sessionId": session_id,
            "cwd": project,
            "timestamp": "2026-07-20T07:00:01.000Z",
            "uuid": format!("{session_id}-assistant"),
            "message": {"role": "assistant", "content": [{"type": "text", "text": response}]}
        }),
    ];
    std::fs::write(
        path,
        events
            .into_iter()
            .map(|event| event.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
}

fn pull_qoder_source(current_dir: &Path, source: &Path, output_dir: &Path) {
    let output = run_waylog(
        current_dir,
        &[
            "pull",
            "--provider",
            "qoder",
            "--source",
            source.to_str().unwrap(),
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--quiet",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn source_parses_one_file_or_a_provider_tree_without_local_discovery() {
    let current_dir = tempfile::tempdir().unwrap();
    let source_dir = current_dir.path().join("raw");
    std::fs::create_dir(&source_dir).unwrap();
    let first = source_dir.join("first.jsonl");
    let contributor_dir = source_dir.join("contributor");
    std::fs::create_dir(&contributor_dir).unwrap();
    let second = contributor_dir.join("second.jsonl");
    write_qoder_session(
        &first,
        "session-first",
        current_dir.path(),
        "first response",
    );
    write_qoder_session(
        &second,
        "session-second",
        current_dir.path(),
        "second response",
    );

    let single_output = current_dir.path().join("single-output");
    pull_qoder_source(current_dir.path(), &first, &single_output);
    assert_eq!(std::fs::read_dir(&single_output).unwrap().count(), 1);

    write_qoder_session(
        &first,
        "session-first",
        current_dir.path(),
        "updated response",
    );
    pull_qoder_source(current_dir.path(), &first, &single_output);
    let markdown = std::fs::read_to_string(
        std::fs::read_dir(&single_output)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(markdown.contains("updated response"));
    assert!(!markdown.contains("first response"));

    let batch_output = current_dir.path().join("batch-output");
    pull_qoder_source(current_dir.path(), &source_dir, &batch_output);
    assert_eq!(std::fs::read_dir(batch_output).unwrap().count(), 2);

    let empty_source = current_dir.path().join("empty");
    let empty_output = current_dir.path().join("empty-output");
    std::fs::create_dir(&empty_source).unwrap();
    let output = run_waylog(
        current_dir.path(),
        &[
            "pull",
            "--provider",
            "qoder",
            "--source",
            empty_source.to_str().unwrap(),
            "--output-dir",
            empty_output.to_str().unwrap(),
            "--quiet",
        ],
    );
    assert!(
        !output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
