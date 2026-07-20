# Agent Instructions

## Documentation Workflow

This repository uses a two-layer documentation system managed by the `doc-flow` skill.

1. Read `docs/index.yaml` at session start to discover durable docs.
2. Load the `doc-flow` skill for the documentation workflow.
3. Keep durable docs under `docs/` and task chronology under `docs/worklog/`.
4. For substantial work, create or reuse one file under `docs/worklog/active/`.
5. Before handoff, push, or PR, promote only high-signal reusable conclusions and archive or delete the active worklog.

**Layers** — Durable docs: `docs/architecture/`, `docs/constraints/`, `docs/decisions/`, `docs/lessons/`, `docs/risks/`. Ephemeral worklogs: `docs/worklog/active/`. Do not create a third layer.
