# Source imports

`waylog pull --provider <provider> --source <path>` exports supported native conversations below the supplied path. Add `--session <id>` to export one conversation; a missing ID reports an error. Markdown is the default readable transcript format.

Session identity and recorded user/assistant text are required. Project attribution is optional: missing or unreadable project metadata must not block transcript export.

- Gemini imports JSON snapshots or JSONL histories. `.project_root` supplies the project path when available.
- Antigravity imports retain `brain/<session-id>/.system_generated/logs/transcript[_full].jsonl`. The directory carries the native identity; detached transcripts without it are rejected.
- Antigravity reads optional project mappings from `history.jsonl` or `cache/last_conversations.json` beside the supplied `brain/` directory. Unavailable mappings leave the project unknown.

Validation uses reference format behavior and synthetic parser/CLI fixtures for directory imports, ID selection, recorded edits and transcript content. Real-application sample acceptance is not a completion requirement.
