# Changelog

User-facing features and critical fixes are documented here from version 0.3.0 onward.

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
