# Engineering Constraints

## Stable Constraints

- Never mutate provider-owned history; database-backed providers must use read-only connections.
- Project-scoped providers must limit discovery to the requested project; application-wide providers must declare global scope and be scanned once per pull.
- Output directories are merge targets: a pull may create or replace Markdown for sessions it processes but must not remove unrelated files.
- `--recursive` broadens lookup to visible descendant projects without changing the output root; `--hidden` is required to traverse hidden descendants, and `.waylog` output trees are always skipped.
- Markdown frontmatter is the persisted sync state, and filenames include the provider session ID to preserve identity outside the local history tree.
- Provider-tagged sync state restoration must match both provider and session ID because one output directory may contain multiple providers.
- Session parsing failures make a batch pull fail only when every attempted session fails; a one-session pull reflects that session's result.
- `--source` bypasses local session discovery, recursively scans regular files below the supplied provider directory, fails when it finds none, and rebuilds supplied sessions even when their message count is unchanged.
- Tool messages are opt-in output; changing `--include-tool-calls` must rewrite the affected Markdown instead of reusing incompatible sync state.
- Provider parsing must not fabricate source content: missing or invalid values remain absent or `null` in Markdown, and timestamps never fall back to wall-clock time.
- Tool detection uses structural protocol markers instead of closed type allowlists. Markdown may remove only confirmed stable wrappers; unsafe normalization falls back to the complete native value.
- Latest-session discovery must search the provider's complete history scope; storage date partitions describe session creation, not recent activity.

## Compatibility

- Support macOS, Linux, and Windows on x64 and ARM64, with platform-specific code behind `cfg` boundaries.
- Treat provider formats as external contracts and support coexisting real formats when necessary.
- Keep `README.md` and `README_zh.md` behaviorally aligned.

## Working Rules

- Tests use synthetic fixtures and temporary directories, never a contributor's actual agent histories, unless otherwise instructed.
- Process-level CLI assertions belong in Rust integration tests so the normal platform matrix covers them.
- CI and release build jobs are read-only; only release publishing may receive `contents: write`.
- Keep documentation concise, intention-led, and unwrapped in source; keep chronology in worklogs.
- Keep documentation self-contained: describe WayLog behavior and boundaries without naming downstream integration repositories.
- Before handoff, run `cargo fmt --all -- --check`, `cargo test --all-features`, `cargo clippy --all-features -- -D warnings`, and `git diff --check`.
