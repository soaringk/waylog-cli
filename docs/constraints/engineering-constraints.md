# Engineering Constraints

## Stable Constraints

- Never mutate provider-owned history; database-backed providers must use read-only connections.
- Project-scoped providers must limit discovery to the requested project; application-wide providers must declare global scope and be scanned once per pull.
- `--recursive` broadens lookup to descendant projects but keeps one resolved tracking root so workspace recovery produces one coherent history.
- Markdown frontmatter is the persisted sync state, and filenames include the provider session ID to preserve identity outside the local history tree.
- Session parsing failures make a batch pull fail only when every attempted session fails; a one-session pull reflects that session's result.
- `--source` bypasses local session discovery, recursively scans regular files below the supplied provider directory, fails when it finds none, and rebuilds supplied sessions even when their message count is unchanged.

## Compatibility

- Support macOS, Linux, and Windows on x64 and ARM64, with platform-specific code behind `cfg` boundaries.
- Treat provider formats as external contracts and support coexisting real formats when necessary.
- Keep `README.md` and `README_zh.md` behaviorally aligned.

## Working Rules

- Tests use synthetic fixtures and temporary directories, never a contributor's actual agent histories, unless otherwise instructed.
- Process-level CLI assertions belong in Rust integration tests so the normal platform matrix covers them.
- CI and release build jobs are read-only; only release publishing may receive `contents: write`.
- Keep documentation concise, intention-led, and unwrapped in source; keep chronology in worklogs.
- Before handoff, run `cargo fmt --all -- --check`, `cargo test --all-features`, `cargo clippy --all-features -- -D warnings`, and `git diff --check`.
