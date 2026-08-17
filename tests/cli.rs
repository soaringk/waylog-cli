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
            "message": {"role": "assistant", "content": [
                {"type": "text", "text": response},
                {"type": "tool_use", "id": format!("{session_id}-tool"), "name": "Read", "input": {"file_path": "src/main.rs"}}
            ]}
        }),
        serde_json::json!({
            "type": "user",
            "sessionId": session_id,
            "cwd": project,
            "timestamp": "2026-07-20T07:00:02.000Z",
            "uuid": format!("{session_id}-tool-result"),
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": format!("{session_id}-tool"), "content": "file contents"}
            ]}
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

fn pull_qoder_source(current_dir: &Path, source: &Path, output_dir: &Path) -> Output {
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
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
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
    let first_markdown = std::fs::read_dir(&single_output)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let first_content = std::fs::read_to_string(&first_markdown).unwrap();
    assert!(first_content.contains("updated response"));
    assert!(!first_content.contains("first response"));

    pull_qoder_source(current_dir.path(), &second, &single_output);
    assert_eq!(std::fs::read_dir(&single_output).unwrap().count(), 2);
    assert_eq!(
        std::fs::read_to_string(&first_markdown).unwrap(),
        first_content
    );
    assert!(std::fs::read_dir(&single_output).unwrap().any(|entry| {
        std::fs::read_to_string(entry.unwrap().path())
            .unwrap()
            .contains("second response")
    }));

    let output = run_waylog(
        current_dir.path(),
        &[
            "pull",
            "--provider",
            "qoder",
            "--source",
            first.to_str().unwrap(),
            "--output-dir",
            single_output.to_str().unwrap(),
            "--include-tool-calls",
        ],
    );
    assert!(output.status.success());
    let markdown_with_tools = std::fs::read_to_string(&first_markdown).unwrap();
    assert_eq!(markdown_with_tools.matches("### 🛠️ Tool").count(), 1);
    assert!(!markdown_with_tools.contains("```json"));
    assert!(markdown_with_tools.contains("src/main.rs"));
    assert!(markdown_with_tools.contains("file contents"));

    pull_qoder_source(current_dir.path(), &first, &single_output);
    assert!(!std::fs::read_to_string(&first_markdown)
        .unwrap()
        .contains("🛠️ Tool"));

    let batch_output = current_dir.path().join("batch-output");
    let output = pull_qoder_source(current_dir.path(), &source_dir, &batch_output);
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains(&format!("History: {}", batch_output.display())));
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

#[test]
fn pull_uses_the_invocation_waylog_and_reports_its_history_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let project = workspace.path().join("project");
    let source = workspace.path().join("session.jsonl");
    std::fs::create_dir_all(workspace.path().join(".waylog")).unwrap();
    std::fs::create_dir_all(project.join(".waylog")).unwrap();
    write_qoder_session(&source, "session-current", &project, "response");

    let output = run_waylog(
        &project,
        &[
            "pull",
            "--provider",
            "qoder",
            "--source",
            source.to_str().unwrap(),
        ],
    );
    let history_dir = project.canonicalize().unwrap().join(".waylog/history");

    assert!(output.status.success());
    assert_eq!(std::fs::read_dir(&history_dir).unwrap().count(), 1);
    assert!(!workspace.path().join(".waylog/history").exists());
    let reported_history = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("History: "))
        .unwrap()
        .to_owned();
    assert_eq!(
        Path::new(&reported_history).canonicalize().unwrap(),
        history_dir.canonicalize().unwrap()
    );
}

#[test]
fn json_format_exports_structured_records_beside_markdown() {
    let current_dir = tempfile::tempdir().unwrap();
    let source_dir = current_dir.path().join("raw");
    std::fs::create_dir(&source_dir).unwrap();
    let session = source_dir.join("session.jsonl");
    write_qoder_session(&session, "session-json", current_dir.path(), "the answer");
    let output_dir = current_dir.path().join("out");

    let output = run_waylog(
        current_dir.path(),
        &[
            "pull",
            "--provider",
            "qoder",
            "--source",
            session.to_str().unwrap(),
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--format",
            "json",
            "--include-tool-calls",
        ],
    );
    assert!(output.status.success());

    let exported = std::fs::read_dir(&output_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].extension().unwrap(), "json");

    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&exported[0]).unwrap()).unwrap();
    assert_eq!(document["provider"], "qoder");
    assert_eq!(document["session_id"], "session-json");
    assert_eq!(document["include_tool_calls"], true);
    assert_eq!(document["message_count"], 4);

    let turns = document["turns"].as_array().unwrap();
    let roles = turns
        .iter()
        .map(|turn| turn["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(roles, ["user", "assistant"]);
    assert_eq!(turns[0]["content"], "hello");

    // Every kind of model output is nested inside the one assistant turn.
    let kinds = turns[1]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["message", "tool"]);
    assert_eq!(turns[1]["parts"][0]["content"], "the answer");
    // A call and the result carrying its id are one step, while message_count above still
    // counts both records, because that is what the provider recorded.
    assert_eq!(turns[1]["parts"][1]["tool_call_id"], "session-json-tool");
    assert_eq!(turns[1]["parts"][1]["result"], "file contents");

    // Markdown into the same directory leaves the structured export in place.
    pull_qoder_source(current_dir.path(), &session, &output_dir);
    assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 2);
    assert!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&exported[0]).unwrap()).is_ok()
    );
}

#[test]
fn a_pull_never_adopts_or_overwrites_a_file_it_did_not_write() {
    for (format, unrelated_name, unrelated_body) in [
        (
            "markdown",
            "my-notes.md",
            "---\nsession_id: session-shared\n---\n\nMy own notes.\n",
        ),
        (
            "json",
            "my-notes.json",
            "{\n  \"session_id\": \"session-shared\",\n  \"mine\": true\n}\n",
        ),
    ] {
        let current_dir = tempfile::tempdir().unwrap();
        let source_dir = current_dir.path().join("raw");
        std::fs::create_dir(&source_dir).unwrap();
        let session = source_dir.join("session.jsonl");
        write_qoder_session(&session, "session-shared", current_dir.path(), "answer");
        let output_dir = current_dir.path().join("out");
        std::fs::create_dir(&output_dir).unwrap();
        let unrelated = output_dir.join(unrelated_name);
        std::fs::write(&unrelated, unrelated_body).unwrap();

        let output = run_waylog(
            current_dir.path(),
            &[
                "pull",
                "--provider",
                "qoder",
                "--source",
                session.to_str().unwrap(),
                "--output-dir",
                output_dir.to_str().unwrap(),
                "--format",
                format,
            ],
        );
        assert!(output.status.success());

        // The unrelated file survives untouched and WayLog wrote its own export beside it.
        assert_eq!(std::fs::read_to_string(&unrelated).unwrap(), unrelated_body);
        assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 2);
    }
}
