# Validate Gemini and Antigravity source imports

- Status: active
- Opened: 2026-07-21
- Repo: `waylog-cli`

## Goal

Verify current Gemini CLI and Antigravity histories can be uploaded as standalone raw artifacts and deterministically reproduced through `waylog pull --provider <provider> --source <path>`.

## Known discrepancies

- No validated raw-artifact collection workflow exists for either provider, so their parsers are not yet exercised through centralized source imports.
- Gemini source files lose the native `.project_root` sidecar after upload, leaving JSONL project metadata empty; legacy JSON derives a path from its local storage location instead of the original project.
- Antigravity source files lose the native `brain/<conversation-id>/` path and `history.jsonl`, so session ID can fall back to `transcript[_full]` and project metadata becomes empty. Uploading those generic filenames would also collide across conversations.
- Invalid timestamps fall back to the current time, and Antigravity events without `step_index` receive random message IDs, so malformed records are not deterministic.
- Existing tests use synthetic fixtures; current real application histories have not been reproduced and compared.

## Completion criteria

- Define collision-free raw artifact names and preserve the metadata required by standalone parsing.
- Reproduce deterministic user and assistant markers in current Gemini CLI and Antigravity sessions.
- Confirm local discovery and `--source` produce equivalent Markdown for the same histories.
- Add only stable format and source-ingestion tests, then update provider status documentation.

## Promotion candidates

- None until current real histories are validated.
