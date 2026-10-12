# Engineering Constraints

## Discovery

- Never mutate provider-owned history; database-backed providers must use read-only connections.
- Project-scoped providers must limit discovery to the requested project; application-wide providers must declare global scope and be scanned once per pull.
- `--recursive` broadens lookup to visible descendant projects without changing the output root; `--hidden` is required to traverse hidden descendants, and `.waylog` output trees are always skipped.
- `--source` bypasses local session discovery, lets the provider select current native artifacts below the supplied directory, fails when it finds none, and rebuilds supplied sessions even when their message count is unchanged.
- A native artifact may contain multiple independent sessions. Batch import expands all of them; explicit session selection exports only the matching ID and fails when absent.
- Discovery searches the provider's complete history scope; storage date partitions describe session creation, not recent activity.
- Session identity comes from native metadata or an identity-bearing native storage path. For replayed histories, use the recording's own first session metadata; embedded ancestor history must not redirect an export.

## Parsing

- Never fabricate content. Absent values stay `null`, timestamps never fall back to wall-clock time, and unreadable reasoning stays missing.
- Ignore only an unterminated final JSONL record whose parse error is unexpected EOF; committed incomplete records and other corruption fail the session.
- Replay provider-defined replacements, checkpoints, patches and rewinds before export. Do not sort surviving records by timestamp.
- Reproduce provider text exactly. Add spacing around a record, never inside it; trailing whitespace can carry meaning.
- Keep records in recorded order for every provider. Order is causal order: input arriving mid-turn must stay where it landed, or nothing explains why the assistant changed course.
- One request is one message: join its text blocks. Only a block WayLog also exports, such as reasoning or a tool record, splits them.
- Never infer a turn boundary. Providers record later input as an ordinary message, so only a run of model output is an observable turn.
- Preserve every readable step of a turn, not just the answer. `AssistantOutput` distinguishes reasoning, answers, and tool records so none hides another.
- Detect tool records by structural markers, not a type allowlist. Strip only confirmed wrappers; fall back to the native value.
- A batch pull fails only if every session fails. A one-session pull reports that session.

## Export

- Markdown is for people, JSON for programs. Both render `exporter::entries`, so a format changes presentation only and neither may show a step the other merges.
- Everything the model produced belongs to one assistant turn. No format may show reasoning, answers, or tool records as peers of user input.
- A tool call and the result carrying its id are one step. Match a result by id, never by adjacency, because providers batch parallel calls. A record with no matching id stands alone in recorded order; which end of an exchange it holds is not recorded.
- Message text can imitate Markdown headings, so never recover structure by matching them. Machine consumers use `--format json`.
- Tool records are opt-in. Changing `--include-tool-calls` rewrites the affected export.
- A pull writes only the path it derives for a session it processes, and replaces whatever occupies that path. It touches nothing else, including the other format's exports.
- Each export carries its own sync state: Markdown in frontmatter, JSON in top-level fields. There is no second store.
- Restoring that state requires an exact match on provider, session ID, and format. A file that does not match is another tool's, and adopting it would overwrite it.
- A different message count means the export is stale; equal-count exports must also match the current rendered content. This catches edits, metadata changes and layout updates without a second state store or layout version.
- An empty parse does not create a new export. When a previously exported session becomes empty, rewrite its tracked export to remove stale content.
- Sync-state decisions live in `Synchronizer`; source selection and forced rebuilding are independent of export representation.

## Compatibility

- Support macOS, Linux, and Windows on x64 and ARM64, with platform-specific code behind `cfg` boundaries.
- Target the latest provider implementation and verify its native format. Backward compatibility is optional; retain it only when it adds little complexity and does not weaken current-format correctness. Old-version coverage is not a release requirement.
- Keep `README.md` and `README_zh.md` behaviorally aligned.
- `src/cli.rs` must not reference the rest of the crate; `build.rs` compiles it standalone to render the man page.

## Working Rules

- Tests use synthetic fixtures and temporary directories, never a contributor's actual agent histories, unless otherwise instructed.
- Process-level CLI assertions belong in Rust integration tests so the normal platform matrix covers them.
- CI and release build jobs are read-only; only release publishing may receive `contents: write`.
- Keep documentation concise, intention-led, and unwrapped in source; keep chronology in worklogs. Code and its comments describe current behavior and intention, with code evolution recorded in commit messages and `CHANGELOG.md`.
- Keep documentation self-contained: describe WayLog behavior and boundaries without naming downstream integration repositories.
- Before handoff, run `cargo fmt --all -- --check`, `cargo test --all-features`, `cargo clippy --all-features -- -D warnings`, and `git diff --check`.
