# Decision Log

## Decisions

- **Provider isolation:** Each agent implements `Provider` and produces `ChatSession`, keeping synchronization and Markdown generation generic.
- **Provider scope:** Providers declare whether their native history is project-scoped or application-wide so recursive recovery does not repeat global scans.
- **Markdown as state:** Markdown frontmatter restores sync progress, avoiding a separate state file or database.
- **Recursive recovery:** `--recursive` is for workspace or monorepo recovery and deliberately aggregates descendant sessions into one tracking root.
- **Explicit sources:** `--session` targets local provider history, while `--source` authoritatively rebuilds supplied provider artifacts or a downloaded provider directory tree; either mode may use `--output-dir`.
