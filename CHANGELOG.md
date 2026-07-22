# Changelog

User-facing features and critical fixes are documented here from version 0.3.0 onward.

## Next

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

[0.3.2]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.2
[0.3.1]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.1
[0.3.0]: https://github.com/soaringk/waylog-cli/releases/tag/v0.3.0
