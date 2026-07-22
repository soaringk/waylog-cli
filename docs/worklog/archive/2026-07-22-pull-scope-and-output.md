# Make pull scope and output explicit

- Status: completed
- Opened: 2026-07-22
- Repo: `waylog-cli`

## Goal

Make `pull` operate on the invocation directory without inheriting an ancestor `.waylog`: standard pulls restore only that project, recursive pulls include its visible descendants, and both write to its `.waylog/history/` unless `--output-dir` is supplied.

## Design

- Reuse `--output-dir` for standard, recursive, session, and source pulls.
- Treat every output directory as a merge target: touch only sessions processed by the current pull.
- Keep `--recursive` responsible only for expanding project discovery.
- Skip hidden descendant directories unless recursive discovery also receives `--hidden`; always skip `.waylog` output trees.
- Print the resolved history directory when a pull completes.
- Preserve `run` project-root discovery.

## Validation

- `cargo test --locked --all-features`: 65 unit tests and 6 CLI integration tests passed.
- `cargo fmt --all -- --check`, Clippy with warnings denied, release build, and `git diff --check` passed.
- CLI help reports version 0.3.2 and exposes `--output-dir` independently of session and source targeting.
- Repeated pulls into one `--output-dir` preserve Markdown files not processed by the later pull.
- A CLI integration test confirms that pull writes to the invocation directory's `.waylog/history/`, ignores an ancestor `.waylog`, and reports the resolved history directory.

## Promoted

- The explicit pull scope, discovery scope, and output contract are recorded in the architecture, constraints, and decision docs.
