# Engineering Constraints

## Discovery

- Never mutate provider-owned history; database-backed providers must use read-only connections.
- Project-scoped providers must limit discovery to the requested project; application-wide providers must declare global scope and be scanned once per pull.
- `--recursive` broadens lookup to visible descendant projects without changing the output root; `--hidden` is required to traverse hidden descendants, and `.waylog` output trees are always skipped.
- `--source` bypasses local session discovery, recursively scans regular files below the supplied provider directory, fails when it finds none, and rebuilds supplied sessions even when their message count is unchanged.
- Latest-session discovery must search the provider's complete history scope; storage date partitions describe session creation, not recent activity.
- Session identity comes from the parsed file's own first metadata record, so history replayed inside a resumed or forked session cannot redirect output to another session's export.

## Parsing

- Provider parsing must not fabricate source content. Missing or invalid values stay absent or `null`, timestamps never fall back to wall-clock time, and encrypted or omitted reasoning stays missing instead of being reconstructed or replaced by a placeholder.
- Parsing must preserve every readable step of an assistant turn, not only its final answer. Reasoning, answers, and tool records are separate records distinguished by `AssistantOutput`, so none can hide another.
- Provider text is reproduced exactly. Formatting may add spacing around a record but must never trim or rewrite its content, because trailing whitespace can carry meaning.
- One provider request maps to one message; its content items are joined rather than split, because splitting a single input into many messages buries the assistant's replies.
- Tool detection uses structural protocol markers instead of closed type allowlists. Normalization may remove only confirmed stable wrappers and falls back to the complete native value when unsafe.
- Session parsing failures make a batch pull fail only when every attempted session fails; a one-session pull reflects that session's result.

## Export

- Markdown is written for people and JSON for programs. Both render the same entries from `exporter::entries`, so a format may change presentation but never structure or content.
- Everything the model produced belongs to one assistant turn. No format may present reasoning, answers, or tool exchanges as peers of the user's input.
- Message text can contain lines that look like Markdown headings, so structure must never be recovered by matching heading text. Machine consumers use `--format json`.
- Tool records are opt-in. Changing `--include-tool-calls` must rewrite the affected export instead of reusing incompatible sync state.
- Output directories are merge targets: a pull creates or replaces only the path it derives for a session it processes, and never touches any other file, including exports written in the other format. A file already occupying a derived path is replaced whatever its contents, which is how a truncated or hand-edited export repairs itself.
- Each export records its own sync state, Markdown in frontmatter and JSON in its top-level fields, so no separate state store exists. Filenames include the provider session ID to preserve identity outside the local history tree.
- Every export names its provider, so sync-state restoration requires an exact match on provider, session ID, and export format. A file that does not match belongs to someone else and must never be adopted as an export path, because adopting it would overwrite it.
- A recorded message count that differs from the current parse in either direction means the export is stale and must be rewritten.
- Exports carry no layout version, so a release that changes export structure reaches existing files only when their message count also changed. Such releases must tell users to run one `--force` pull; do not add a version discriminator whose correctness depends on remembering to bump it.
- Sync-state decisions belong to `Synchronizer`, so `run`, `pull`, `--recursive`, and `--source` behave identically. Modes differ only in which sessions they select and whether they force.

## Compatibility

- Support macOS, Linux, and Windows on x64 and ARM64, with platform-specific code behind `cfg` boundaries.
- Treat provider formats as external contracts and support coexisting real formats when necessary.
- Keep `README.md` and `README_zh.md` behaviorally aligned.
- `src/cli.rs` must not reference the rest of the crate; `build.rs` compiles it standalone to render the man page.

## Working Rules

- Tests use synthetic fixtures and temporary directories, never a contributor's actual agent histories, unless otherwise instructed.
- Process-level CLI assertions belong in Rust integration tests so the normal platform matrix covers them.
- CI and release build jobs are read-only; only release publishing may receive `contents: write`.
- Keep documentation concise, intention-led, and unwrapped in source; keep chronology in worklogs.
- Keep documentation self-contained: describe WayLog behavior and boundaries without naming downstream integration repositories.
- Before handoff, run `cargo fmt --all -- --check`, `cargo test --all-features`, `cargo clippy --all-features -- -D warnings`, and `git diff --check`.
