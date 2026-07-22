# Decision Log

## Decisions

- **Provider isolation:** Each agent implements `Provider` and produces `ChatSession`, keeping synchronization and Markdown generation generic.
- **Provider scope:** Providers declare whether their native history is project-scoped or application-wide so recursive recovery does not repeat global scans.
- **Markdown as state:** Markdown frontmatter restores sync progress, avoiding a separate state file or database.
- **Recursive recovery:** `--recursive` aggregates visible descendant sessions into the current output; `--hidden` includes hidden descendants.
- **Explicit output:** Pull defaults to the invocation directory's `.waylog/history/`; `--output-dir` replaces it. Ancestor projects never affect pull, and repeated writes preserve unrelated files.
- **Explicit sources:** `--session` targets local provider history, while `--source` rebuilds supplied provider artifacts or directory trees. Both accept `--output-dir`.
- **Optional tool output:** Providers detect structural tool records without closed type allowlists. `--include-tool-calls` renders readable payloads after removing stable protocol wrappers and preserves the complete native value when normalization is unsafe.
