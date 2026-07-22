# Optional tool-call output

- Status: completed
- Opened: 2026-07-22
- Completed: 2026-07-22
- Repo: `waylog-cli`

## Request

- Add opt-in, provider-aware tool request/result output under grouped Tool headings with plain code fences.
- Keep the implementation deterministic, compatible, cohesive, and simple.
- Detect tool records through structural protocol markers instead of a closed type allowlist.
- Remove only stable machine wrappers for readability, with full-value fallback when normalization is unsafe.
- Represent missing or invalid source timestamps as `null`; never replace source data with the current time.
- Consolidate the finished implementation without changing its behavior or adding speculative seams.

## Outcome

- Added `waylog pull --include-tool-calls`; providers retain recognized calls and results as Tool messages.
- Grouped matching request/result records inside the Markdown formatter while preserving raw message counts for synchronization.
- Centralized structural tool detection, readable payload normalization, and full-value fallback.
- Persisted output mode so changing the flag rewrites existing Markdown; default output remains compatible.
- Removed closed tool type lists across stable providers; exact protocol tokens and linkage fields identify tool records.
- Preserved Claude and Codex message text without provider-markup filtering and kept repeated records rather than deduplicating them.
- Preserved unknown and unfamiliar wrapper fields with deterministic serialization and safe Markdown fences.
- Represented missing or invalid stable-provider timestamps as `null` instead of fabricating the current time.
- Reused `Synchronizer` for final run cleanup and removed duplicate tracker interfaces and tests.
- Reduced sync state to the three frontmatter facts used at runtime, removed the dead state module and provider dependency, and kept one explicit history-directory constructor.
- Removed the unused discovery path from `Synchronizer`; discovery remains owned by pull and watcher flows.
- Restored state by provider and session ID so merged output directories cannot cross-wire providers.

## Validation

- Formatting, 69 focused unit tests, 6 CLI integration tests, Clippy with warnings denied, release build, and `git diff --check` passed.
- Semantic equality tests cover structural detection, unknown fields, stable wrapper removal, unfamiliar wrapper preservation, and full-value fallback.
- Real Codex, Qoder, QoderWork, and OpenCode histories reproduced 50→25, 4→2, 94→47, and 9→9 tool records→sections respectively.

## Follow-up

- Gemini-specific source discrepancies remain in their existing validation worklog.
