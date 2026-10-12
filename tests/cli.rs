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

    for args in [&["pull", "--provider", "invalid_provider"][..]] {
        assert_eq!(run_waylog(current_dir.path(), args).status.code(), Some(64));
    }
}

#[test]
fn json_error_output_is_valid() {
    let current_dir = tempfile::tempdir().unwrap();
    let output = run_waylog(
        current_dir.path(),
        &["--output", "json", "pull", "--provider", "invalid_provider"],
    );

    assert_eq!(output.status.code(), Some(64));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["level"], "error");
    assert_eq!(
        value["message"],
        "'invalid_provider' is not a recognized provider."
    );
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

#[test]
fn codex_source_exports_a_live_tail_and_models_but_rejects_committed_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("rollout.jsonl");
    let output_dir = dir.path().join("out");
    let records = [
        serde_json::json!({"type":"session_meta","payload":{"id":"session-live","cwd":dir.path()}}),
        serde_json::json!({"type":"turn_context","payload":{"model":"recorded-model"}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"request"}]}}),
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]}}),
    ];
    let complete = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let live = format!("{complete}\n{{\"type\":");
    std::fs::write(&source, &live).unwrap();
    let args = [
        "pull",
        "--provider",
        "codex",
        "--source",
        source.to_str().unwrap(),
        "--output-dir",
        output_dir.to_str().unwrap(),
        "--format",
        "json",
    ];
    let output = run_waylog(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(&source).unwrap(), live);
    let exported = std::fs::read_dir(&output_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read(&exported).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["turns"][1]["parts"][0]["model"], "recorded-model");
    assert_eq!(document["message_count"], 2);
    std::fs::write(&source, format!("{live}\n")).unwrap();
    assert!(!run_waylog(dir.path(), &args).status.success());
    assert_eq!(std::fs::read(exported).unwrap(), bytes);
}

#[test]
fn gemini_source_replays_edits_and_clears_an_existing_export_on_rewind() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("session.jsonl");
    let output_dir = dir.path().join("out");
    let records = [
        serde_json::json!({"sessionId":"gemini-live","projectHash":"opaque","startTime":"invalid"}),
        serde_json::json!({"id":"u","type":"user","content":"ask"}),
        serde_json::json!({"id":"a","type":"gemini","content":"old answer"}),
        serde_json::json!({"$patch":{"id":"a","content":"revised answer"}}),
    ];
    let complete = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&source, &complete).unwrap();
    let args = [
        "pull",
        "--provider",
        "gemini",
        "--source",
        source.to_str().unwrap(),
        "--output-dir",
        output_dir.to_str().unwrap(),
        "--format",
        "json",
    ];
    assert!(run_waylog(dir.path(), &args).status.success());
    let exported = std::fs::read_dir(&output_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&exported).unwrap()).unwrap();
    assert_eq!(document["project"], serde_json::Value::Null);
    assert_eq!(document["started_at"], serde_json::Value::Null);
    assert_eq!(
        document["turns"][1]["parts"][0]["content"],
        "revised answer"
    );
    std::fs::write(
        &source,
        format!("{complete}\n{}", serde_json::json!({"$rewindTo":"u"})),
    )
    .unwrap();
    assert!(run_waylog(dir.path(), &args).status.success());
    let cleared: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&exported).unwrap()).unwrap();
    assert_eq!(cleared["message_count"], 0);
    assert!(cleared["turns"].as_array().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(output_dir).unwrap().count(), 1);
}

#[test]
fn grok_source_directory_ignores_summary_and_replays_updates() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(
        source.join("summary.json"),
        r#"{"info":{"id":"grok-one","cwd":"/recorded/project"}}"#,
    )
    .unwrap();
    let event = serde_json::json!({"method":"session/update","params":{"sessionId":"grok-one","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"keep this answer  "},"_meta":{"promptIndex":0}}}});
    std::fs::write(source.join("updates.jsonl"), format!("{event}\n")).unwrap();
    let output = dir.path().join("out");
    let result = run_waylog(
        dir.path(),
        &[
            "pull",
            "--provider",
            "grok",
            "--source",
            source.to_str().unwrap(),
            "--format",
            "json",
            "--output-dir",
            output.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let files: Vec<_> = std::fs::read_dir(output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(json["turns"][0]["content"], "keep this answer  ");
}

#[test]
fn dsh_source_directory_selects_latest_compressed_generation() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let header = serde_json::json!({"type":"session","id":"dsh-one","cwd":"/recorded/project"});
    for (name, text) in [
        ("session.jsonl", "obsolete"),
        ("session.v2.jsonl.zstd", "current"),
    ] {
        let event = serde_json::json!({"type":"user/message","data":{"content":[{"type":"text","text":text}]}});
        let bytes = format!("{header}\n{event}\n").into_bytes();
        let bytes = if name.ends_with("zstd") {
            zstd::stream::encode_all(bytes.as_slice(), 1).unwrap()
        } else {
            bytes
        };
        std::fs::write(source.join(name), bytes).unwrap();
    }
    let output = dir.path().join("out");
    let result = run_waylog(
        dir.path(),
        &[
            "pull",
            "--provider",
            "dsh",
            "--source",
            source.to_str().unwrap(),
            "--format",
            "json",
            "--output-dir",
            output.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let files: Vec<_> = std::fs::read_dir(output)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(json["turns"][0]["content"], "current");
}

#[test]
fn cursor_database_source_exports_multiple_conversations_and_native_tool_turns() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("state.vscdb");
    let connection = rusqlite::Connection::open(&source).unwrap();
    connection
        .execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value TEXT);")
        .unwrap();
    let records = [
        (
            "composerData:first",
            serde_json::json!({"fullConversationHeadersOnly":[{"bubbleId":"user"},{"bubbleId":"tool"},{"bubbleId":"answer"}]}),
        ),
        (
            "bubbleId:first:answer",
            serde_json::json!({"type":2,"text":"final answer  "}),
        ),
        (
            "bubbleId:first:tool",
            serde_json::json!({"type":2,"thinking":{"text":"recorded reasoning"},"toolFormerData":{"name":"read_file","toolCallId":"call-one","params":"{}","status":"completed","result":"returned content"}}),
        ),
        (
            "bubbleId:first:user",
            serde_json::json!({"type":1,"text":"question","modelInfo":{"modelName":"native-model"}}),
        ),
        (
            "bubbleId:second:user",
            serde_json::json!({"type":1,"text":"other question"}),
        ),
    ];
    for (key, value) in records {
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES (?1, ?2)",
                rusqlite::params![key, value.to_string()],
            )
            .unwrap();
    }
    drop(connection);
    let original = std::fs::read(&source).unwrap();
    let out = dir.path().join("all");
    let output = run_waylog(
        dir.path(),
        &[
            "pull",
            "--provider",
            "cursor",
            "--source",
            source.to_str().unwrap(),
            "--format",
            "json",
            "--include-tool-calls",
            "--output-dir",
            out.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let documents: Vec<serde_json::Value> = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| {
            serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap()
        })
        .collect();
    assert_eq!(documents.len(), 2);
    let first = documents
        .iter()
        .find(|value| value["session_id"] == "first")
        .unwrap();
    assert_eq!(first["turns"][0]["role"], "user");
    let parts = first["turns"][1]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0]["kind"], "reasoning");
    assert_eq!(parts[1]["kind"], "tool");
    assert_eq!(parts[1]["tool_call_id"], "call-one");
    assert!(parts[1]["result"].to_string().contains("returned content"));
    assert_eq!(parts[2]["content"], "final answer  ");
    assert_eq!(parts[2]["model"], "native-model");
    let selected = dir.path().join("selected");
    let output = run_waylog(
        dir.path(),
        &[
            "pull",
            "--provider",
            "cursor",
            "--source",
            source.to_str().unwrap(),
            "--session",
            "second",
            "--output-dir",
            selected.to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    assert_eq!(std::fs::read_dir(selected).unwrap().count(), 1);
    let missing = run_waylog(
        dir.path(),
        &[
            "pull",
            "--provider",
            "cursor",
            "--source",
            source.to_str().unwrap(),
            "--session",
            "absent",
            "--output-dir",
            out.to_str().unwrap(),
        ],
    );
    assert!(!missing.status.success());
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 2);
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn claude_source_directory_excludes_native_subagent_tree() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::create_dir_all(source.join("main/subagents")).unwrap();
    write_qoder_session(
        &source.join("main.jsonl"),
        "same-native-id",
        dir.path(),
        "main answer",
    );
    write_qoder_session(
        &source.join("main/subagents/agent.jsonl"),
        "same-native-id",
        dir.path(),
        "child answer",
    );
    std::fs::write(source.join("readme.txt"), "auxiliary").unwrap();
    let out = dir.path().join("out");
    let output = run_waylog(
        dir.path(),
        &[
            "pull",
            "--provider",
            "claude",
            "--source",
            source.to_str().unwrap(),
            "--output-dir",
            out.to_str().unwrap(),
        ],
    );
    assert!(output.status.success());
    let files: Vec<_> = std::fs::read_dir(out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("main answer"));
    assert!(!text.contains("child answer"));
}

#[test]
fn database_imports_isolate_corrupt_sessions_and_select_before_parsing() {
    for case in [
        "opencode",
        "cursor-header",
        "cursor-state",
        "cursor-bubble",
        "cursor-rich-text",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let provider = if case == "opencode" {
            "opencode"
        } else {
            "cursor"
        };
        let source = dir.path().join("history.db");
        let connection = rusqlite::Connection::open(&source).unwrap();
        if provider == "opencode" {
            connection.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,data TEXT);").unwrap();
            for (id, data) in [("good", r#"{"role":"user"}"#), ("bad", "not json")] {
                connection
                    .execute(
                        "INSERT INTO session VALUES (?1,'/project',1700000000000,1700000000001)",
                        [id],
                    )
                    .unwrap();
                connection
                    .execute(
                        "INSERT INTO message VALUES (?1,?1,1700000000000,?2)",
                        [id, data],
                    )
                    .unwrap();
                connection
                    .execute(
                        "INSERT INTO part VALUES (?1,?1,?1,?2)",
                        [id, r#"{"type":"text","text":"valid question"}"#],
                    )
                    .unwrap();
            }
        } else {
            connection.execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE composerHeaders(composerId TEXT,value TEXT,isSubagent INTEGER);").unwrap();
            for id in ["good", "bad"] {
                let bubble = if id == "bad" && case == "cursor-bubble" {
                    "not json"
                } else if id == "bad" && case == "cursor-rich-text" {
                    r#"{"type":1,"richText":"not json"}"#
                } else {
                    r#"{"type":1,"text":"valid question"}"#
                };
                connection
                    .execute(
                        "INSERT INTO cursorDiskKV VALUES (?1,?2)",
                        [format!("bubbleId:{id}:user"), bubble.to_string()],
                    )
                    .unwrap();
                if case == "cursor-state" {
                    connection
                        .execute(
                            "INSERT INTO cursorDiskKV VALUES (?1,?2)",
                            [
                                format!("composerData:{id}"),
                                if id == "bad" {
                                    "not json".to_string()
                                } else {
                                    "{}".to_string()
                                },
                            ],
                        )
                        .unwrap();
                }
                if case == "cursor-header" {
                    connection
                        .execute(
                            "INSERT INTO composerHeaders VALUES (?1,?2,0)",
                            [id, if id == "bad" { "not json" } else { "{}" }],
                        )
                        .unwrap();
                }
            }
        }
        drop(connection);
        let original = std::fs::read(&source).unwrap();
        let all = dir.path().join("all");
        let output = run_waylog(
            dir.path(),
            &[
                "pull",
                "--provider",
                provider,
                "--source",
                source.to_str().unwrap(),
                "--format",
                "json",
                "--output-dir",
                all.to_str().unwrap(),
            ],
        );
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("session bad"),
            "{case} must report failed conversation"
        );
        let files = std::fs::read_dir(&all)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1, "{case}");
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(document["session_id"], "good", "{case}");
        assert_eq!(document["turns"][0]["content"], "valid question", "{case}");
        for id in ["good", "bad", "missing"] {
            let selected = dir.path().join(id);
            let output = run_waylog(
                dir.path(),
                &[
                    "pull",
                    "--provider",
                    provider,
                    "--source",
                    source.to_str().unwrap(),
                    "--session",
                    id,
                    "--format",
                    "json",
                    "--output-dir",
                    selected.to_str().unwrap(),
                ],
            );
            assert_eq!(
                output.status.success(),
                id == "good",
                "{case}/{id}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if id == "good" {
                assert_eq!(std::fs::read_dir(&selected).unwrap().count(), 1);
                assert!(!String::from_utf8_lossy(&output.stderr).contains("session bad"));
            } else {
                assert_eq!(
                    std::fs::read_dir(&selected)
                        .map(|entries| entries.count())
                        .unwrap_or(0),
                    0
                );
                if id == "missing" {
                    assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
                }
            }
        }
        assert_eq!(std::fs::read(&source).unwrap(), original, "{case}");
    }
}

#[test]
fn explicit_sidechain_import_cannot_overwrite_parent_export() {
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("parent.jsonl");
    let child = dir.path().join("subagents/child.jsonl");
    let relocated = dir.path().join("relocated-child.jsonl");
    std::fs::create_dir_all(child.parent().unwrap()).unwrap();
    write_qoder_session(&parent, "parent-id", dir.path(), "parent answer");
    write_qoder_session(&child, "parent-id", dir.path(), "child answer");
    std::fs::write(&relocated, serde_json::json!({"type":"user","sessionId":"parent-id","isSidechain":true,"message":{"content":"relocated child"}}).to_string()).unwrap();
    for provider in ["claude", "qoder", "qoderwork"] {
        let out = dir.path().join(provider);
        let output = run_waylog(
            dir.path(),
            &[
                "pull",
                "--provider",
                provider,
                "--source",
                parent.to_str().unwrap(),
                "--output-dir",
                out.to_str().unwrap(),
            ],
        );
        assert!(output.status.success());
        let files = std::fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        let original = std::fs::read(&files[0]).unwrap();
        for source in [&child, &relocated] {
            let output = run_waylog(
                dir.path(),
                &[
                    "pull",
                    "--provider",
                    provider,
                    "--source",
                    source.to_str().unwrap(),
                    "--output-dir",
                    out.to_str().unwrap(),
                ],
            );
            assert!(!output.status.success(), "{provider}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("independent session identity")
            );
            assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
            assert_eq!(std::fs::read(&files[0]).unwrap(), original);
        }
    }
}

#[test]
fn gemini_and_antigravity_export_source_trees_and_selected_transcripts_without_project_metadata() {
    for provider in ["gemini", "antigravity", "antigravity-2", "antigravity-ide"] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        for id in ["one", "two"] {
            let file = if provider == "gemini" {
                source.join(format!("project/chats/session-{id}.jsonl"))
            } else {
                source.join(format!(
                    "brain/{id}/.system_generated/logs/transcript_full.jsonl"
                ))
            };
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            let records = if provider == "gemini" {
                vec![
                    serde_json::json!({"sessionId":id,"projectHash":"opaque"}),
                    serde_json::json!({"id":"u","type":"user","content":format!("question-{id}")}),
                    serde_json::json!({"id":"a","type":"gemini","content":format!("answer-{id}")}),
                ]
            } else {
                vec![
                    serde_json::json!({"type":"USER_INPUT","source":"USER_EXPLICIT","content":format!("question-{id}")}),
                    serde_json::json!({"type":"PLANNER_RESPONSE","source":"MODEL","content":format!("answer-{id}")}),
                ]
            };
            std::fs::write(
                file,
                records
                    .iter()
                    .map(serde_json::Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
        }
        if provider != "gemini" {
            std::fs::create_dir_all(source.join("cache")).unwrap();
            std::fs::write(source.join("cache/last_conversations.json"), "{").unwrap();
        }
        for (selection, expected) in [(None, 2), (Some("two"), 1)] {
            let out = dir.path().join(if selection.is_some() {
                "selected"
            } else {
                "all"
            });
            let mut args = vec![
                "pull",
                "--provider",
                provider,
                "--source",
                source.to_str().unwrap(),
                "--output-dir",
                out.to_str().unwrap(),
            ];
            if let Some(id) = selection {
                args.extend(["--session", id]);
            }
            let result = run_waylog(dir.path(), &args);
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let files: Vec<_> = std::fs::read_dir(&out)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert_eq!(files.len(), expected);
            let text = files
                .iter()
                .map(|f| std::fs::read_to_string(f).unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("question-two") && text.contains("answer-two"));
            if selection.is_some() {
                assert!(!text.contains("question-one") && !text.contains("answer-one"));
            } else {
                assert!(text.contains("question-one") && text.contains("answer-one"));
            }
        }
        let result = run_waylog(
            dir.path(),
            &[
                "pull",
                "--provider",
                provider,
                "--source",
                source.to_str().unwrap(),
                "--session",
                "missing",
                "--output-dir",
                dir.path().join("missing").to_str().unwrap(),
            ],
        );
        assert!(!result.status.success());
    }
}
