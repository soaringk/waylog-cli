# Gemini and Antigravity source imports

- Status: completed under the accepted transcript-export scope
- Opened: 2026-07-21
- Closed: 2026-10-12

## Accepted scope

Export supported conversations from a supplied path, or select one by native ID with `--session`. Preserve readable user and assistant text. Project attribution is optional. Real-application sample acceptance is deferred and does not block completion.

## Delivered

- Gemini snapshot/JSONL parsing and recorded replacements, patches, checkpoints and rewinds.
- Antigravity CLI, 2.0 and IDE imports with native directory identity; ambiguous detached files are rejected.
- Missing project metadata leaves project unknown. Antigravity metadata errors do not prevent transcript parsing.
- CLI regression coverage imports two conversations per provider, selects one by ID, checks transcript text and rejects a missing ID. Antigravity fixtures include a malformed optional project cache.

Current contract: [Source imports](../../architecture/source-imports.md). Parser integration: [0.5.0 source update](2026-10-12-session-export-hardening.md).
