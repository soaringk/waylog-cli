# Adopt and refine doc-flow

- Status: complete
- Opened: 2026-07-20T07:16:20Z
- Repo: `waylog-cli`

## Request

- Adopt the `doc-flow` two-layer model, preserve existing canonical guides, and migrate stable engineering context into concise durable docs.
- Keep prose unwrapped in source and explain intention and boundaries instead of implementation inventories or change history.
- Explain that `--recursive` is workspace-level aggregation, not independent output for every nested project.

## Initial Context

- `README.md`, `README_zh.md`, and `CHANGELOG.md` were the existing canonical user and release docs.
- The repository had no local agent routing, documentation index, or durable engineering context.
- The worktree already contained approved workflow hardening, Rust CLI tests, and removal of shell integration scripts.

## Event Log

- Indexed the existing canonical docs and added architecture, constraints, decisions, lessons, and risks.
- Added repository-local routing for consuming durable docs and recording substantial work in one active worklog.
- Removed the redundant worklog template because this worklog is sufficient as a context-free example.
- Rewrote durable docs around intent and boundaries, removed manual line wrapping, and documented the distinct `--recursive` recovery scope in both user guides.
- Distilled the recursive-mode intent into system architecture and the documentation style into engineering constraints.
- Reopened this worklog to audit every durable documentation claim against the amended code and workflow behavior.
- Corrected provider coverage, periodic sync semantics, recursive hidden-directory scope, targeted lookup performance claims, batch failure boundaries, and the required `--provider` argument.

## Outcome / Handoff

- Result: Doc-flow is adopted with one index, five concise durable categories, and this single archived worklog; durable claims now match the CLI, provider list, synchronization behavior, and workflows.
- Validation: All indexed paths and YAML are valid; release CLI help and version are correct; 97 tests, Rustfmt, Clippy with warnings denied, release build, and `git diff --check` pass.
- Follow-up: Future substantial tasks should create one active worklog and distill it before handoff, push, or PR.
