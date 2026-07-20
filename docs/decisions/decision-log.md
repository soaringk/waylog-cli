# Decision Log

## Decisions

- **Provider isolation:** Each agent implements `Provider` and produces `ChatSession`, keeping synchronization and Markdown generation generic.
- **Markdown as state:** Markdown frontmatter restores sync progress, avoiding a separate state file or database.
- **Recursive recovery:** `--recursive` is for workspace or monorepo recovery and deliberately aggregates descendant sessions into one tracking root.
- **Targeted export:** `--session` is provider-scoped for hook integration; `--output-dir` is therefore valid only with one session.
