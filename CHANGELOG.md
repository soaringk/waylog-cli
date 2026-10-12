# Changelog

User-facing features and critical fixes are documented here from version 0.3.0 onward.

## [0.5.0] - 2026-10-12

### Removed

- Removed `waylog run`, agent launching, process/signal management and background synchronization. Periodic exports use repeated `waylog pull` invocations from an external scheduler.
- Removed run-only provider interfaces, dependencies, demos and tests.

### Added

- Cursor read-only SQLite import with multiple conversations, native ordering, reasoning and tool exchanges.
- DeepSeek Harness current-generation JSONL and Zstandard import, including compaction; Grok Build streamed histories and rewinds.
- Antigravity 2.0 and IDE histories alongside CLI. All three surfaces discover their complete application-wide brain histories, even without workspace mappings.
- OpenCode can import all conversations from a supplied SQLite database without accessing local history.
- `--session` selects a conversation within a supplied database or history artifact.
- Provider-specific source-directory selection avoids importing auxiliary files or obsolete native generations.
- Codex discovery includes archived sessions as well as active history, with the same project scope and targeted session lookup.
- Codex assistant records carry the model recorded in each turn context. Event-only histories are supported without duplicating their mirrors in modern response-item histories.
- Gemini exports native tool calls and patched results with `--include-tool-calls`, and retains display text and recorded thoughts.

### Fixed

- Optional Antigravity project metadata errors do not block transcript exports.
- Cursor and OpenCode isolate malformed conversations during database imports. Targeted imports select the native ID before parsing unrelated conversations.
- Gemini tool-only messages preserve token usage once per native message; Grok text separated only by omitted media stays in one submission.
- Claude-family source directories exclude subagent recordings, and explicit sidechain imports are rejected, preventing child sessions from overwriting their parent. Native session IDs remain authoritative when metadata follows an initial snapshot record.
- Antigravity imports validate native identity-bearing paths and resolve project metadata from the supplied tree.
- JSONL providers tolerate only an unfinished, unterminated final JSON record. Malformed records and newline-terminated incomplete records still fail, rather than silently dropping evidence.
- Gemini JSONL replay applies message replacements, checkpoints, content/tool patches, removals, ordering and rewinds before export. Timestamps do not reorder messages.
- Gemini discovery selects one recording during JSON-to-JSONL migration, retaining a complete snapshot until the append log catches up while honoring explicit edits and rewinds.
- Missing or invalid Gemini timestamps remain absent, and an opaque project hash is never presented as a working directory. `.project_root` supplies the path when available. Subagent recordings are not attributed to the main conversation.
- Normal pulls refresh changed content or metadata even when record counts stay equal. A previously exported conversation rewound to empty is rewritten as an empty export instead of leaving stale answers behind.

### Upgrading

Run a normal pull in each project you want to refresh, using `--format json` for structured exports. Equal-count exports are now checked against the current rendered content, so these changes do not require a one-time `--force`.

## [0.4.2] - 2026-08-17

### Changed

- A tool call and its result are now one `tool` part in `--format json`, matching what the Markdown has always shown. The call stays in `content` and the result it returned appears in `result`. A result is matched by `tool_call_id`, because providers batch parallel calls; a record with no matching id keeps its own part, in recorded order. `message_count` still counts the records a provider wrote, so a paired exchange counts as two.

### Upgrading

This release changes the structured export only. Markdown output is byte-identical, so Markdown histories need nothing. Because pairing does not change `message_count`, existing JSON exports written with `--include-tool-calls` are not stale and an ordinary pull leaves them on the old shape: run `waylog pull --format json --force` once to convert them.

## [0.4.1] - 2026-08-14

### Fixed

- Every provider now treats one request as one message. Claude, Qoder, QoderWork and OpenCode were splitting a request into several turns; over half of local QoderWork requests were split, each `<system-reminder>` becoming its own user turn.
- Text is only split by a block WayLog also exports, such as reasoning or a tool record. Media that WayLog drops no longer breaks a request in two.

## [0.4.0] - 2026-08-14

### Added

- `--format json` writes each session as one structured document with the same hierarchy the Markdown shows: a `turns` array where an assistant turn carries the `parts` the model produced. Programs no longer have to match Markdown headings, which message text can imitate. Markdown remains the default, and both formats can share an output directory.
- Assistant reasoning is now exported. Readable reasoning steps appear as `Reasoning` records, nested with answers and tool exchanges under one `Assistant` section in the order the provider recorded them.

### Fixed

- Recovered assistant content that was silently dropped: Codex `reasoning` items, Claude, Qoder, and QoderWork `thinking` blocks, and OpenCode reasoning that vanished whenever a message had no text part. Encrypted or unsaved reasoning stays absent rather than being reconstructed.
- Codex session identity now comes from a rollout's own first `session_meta`. A resumed or forked rollout replays the sessions it continues, so WayLog misread it as the replayed session and let it overwrite that session's export.
- Codex requests stay one message instead of splitting every content item into its own message, which previously turned a single injected transcript into hundreds of `User` sections.
- Codex `developer` messages are recorded as `System` instead of being discarded.
- An export is rewritten whenever its recorded message count no longer matches the current parse, not only when the count grew.
- A pull no longer adopts, and therefore no longer overwrites, a file it did not write. Restoring sync state previously treated a missing `provider` as a match, so any file in an `--output-dir` carrying a matching `session_id` could be replaced by an export.
- A session whose provider recorded no project location is reported as `null` instead of an empty value.

### Upgrading

This release changes the export layout. Sessions whose message count changes are rewritten by an ordinary pull; run `waylog pull --force` once to convert histories whose count is unchanged, such as Gemini and Antigravity sessions or Claude sessions without reasoning.

## [0.3.3] - 2026-07-23

- Added `--include-tool-calls` to render tool requests and results in readable grouped Tool sections, with complete native payload fallback when normalization is unsafe.
- Preserved provider-recorded conversation content, represented missing timestamps as `null`, and isolated merged-output sync state by provider and output mode.
- Fixed Codex latest-session discovery so long-running sessions are found regardless of their creation-date directory.

## [0.3.2] - 2026-07-22

- Anchored pull to the current project: output defaults to `.waylog/history/`, recursive recovery adds visible descendants, and repeated writes preserve unrelated files.

## [0.3.1] - 2026-07-21

- Added project-scoped Qoder and application-wide QoderWork history parsing.
- Added direct parsing of provider-native files or downloaded provider directory trees, including OpenCode JSON exports.

## [0.3.0] - 2026-07-20

- Added OpenCode history parsing from its local SQLite database.
- Added targeted session export with `--session` and `--output-dir`.
- Added checksum-verified, pre-built binaries for macOS, Linux, and Windows on x64 and ARM64.
- Added current Gemini JSONL history support and fixed cross-platform builds.

[0.3.3]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.3
[0.3.2]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.2
[0.3.1]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.1
[0.3.0]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.0

[0.5.0]: https://github.com/soaringk/waylog-cli/releases/tag/v0.5.0
