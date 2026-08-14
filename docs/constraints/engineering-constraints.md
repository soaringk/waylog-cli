# Engineering Constraints

## Discovery

- Never mutate provider-owned history; database-backed providers must use read-only connections.
- Project-scoped providers must limit discovery to the requested project; application-wide providers must declare global scope and be scanned once per pull.
- `--recursive` broadens lookup to visible descendant projects without changing the output root; `--hidden` is required to traverse hidden descendants, and `.waylog` output trees are always skipped.
- `--source` bypasses local session discovery, recursively scans regular files below the supplied provider directory, fails when it finds none, and rebuilds supplied sessions even when their message count is unchanged.
- Latest-session discovery must search the provider's complete history scope; storage date partitions describe session creation, not recent activity.
- Session identity comes from the parsed file's own first metadata record, so history replayed inside a resumed or forked session cannot redirect output to another session's export.

## Parsing

- Never fabricate content. Absent values stay `null`, timestamps never fall back to wall-clock time, and unreadable reasoning stays missing.
- Reproduce provider text exactly. Add spacing around a record, never inside it; trailing whitespace can carry meaning.
- Keep records in recorded order for every provider. Order is causal order: input arriving mid-turn must stay where it landed, or nothing explains why the assistant changed course.
- One request is one message: join its text blocks. Only a block WayLog also exports, such as reasoning or a tool record, splits them.
- Never infer a turn boundary. Providers record later input as an ordinary message, so only a run of model output is an observable turn.
- Preserve every readable step of a turn, not just the answer. `AssistantOutput` distinguishes reasoning, answers, and tool records so none hides another.
- Detect tool records by structural markers, not a type allowlist. Strip only confirmed wrappers; fall back to the native value.
- A batch pull fails only if every session fails. A one-session pull reports that session.

## Export

- Markdown is for people, JSON for programs. Both render `exporter::entries`, so a format changes presentation only.
- Everything the model produced belongs to one assistant turn. No format may show reasoning, answers, or tool records as peers of user input.
- Message text can imitate Markdown headings, so never recover structure by matching them. Machine consumers use `--format json`.
- Tool records are opt-in. Changing `--include-tool-calls` rewrites the affected export.
- A pull writes only the path it derives for a session it processes, and replaces whatever occupies that path. It touches nothing else, including the other format's exports.
- Each export carries its own sync state: Markdown in frontmatter, JSON in top-level fields. There is no second store.
- Restoring that state requires an exact match on provider, session ID, and format. A file that does not match is another tool's, and adopting it would overwrite it.
- A message count that differs from the current parse in either direction means the export is stale.
- Exports carry no layout version, so a structural change reaches old files only when their count also changed. Such releases tell users to run `--force` once. Do not add a discriminator that depends on remembering to bump it.
- Sync-state decisions live in `Synchronizer`, so all modes behave alike. Modes differ only in which sessions they select and whether they force.

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
