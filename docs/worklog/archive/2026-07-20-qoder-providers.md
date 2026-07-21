# Add Qoder providers

- Status: complete
- Opened: 2026-07-20T07:41:11Z
- Completed: 2026-07-20T10:14:11Z
- Repo: `waylog-cli`

## Request

- Add conversation parsing for Qoder under `~/.qoder/projects/` and QoderWork under `~/.qoderwork/projects/`.
- Release the feature as version 0.3.1.
- Reproduce real Codex, OpenCode, Qoder, and QoderWork conversations and verify the generated Markdown.
- Keep Qoder and QoderWork separate where their storage scope differs.

## Initial Context

- Provider implementations convert native histories into `ChatSession`; shared synchronization and Markdown export must remain provider-independent.
- Local provider data may be inspected read-only for format validation, while automated tests must use synthetic fixtures.
- The two products share a Claude-family event format but organize session discovery differently.

## Plan

1. Inspect both native layouts and identify stable project, session, and message fields.
2. Implement separate provider discovery with shared event decoding.
3. Reproduce real provider conversations and compare their Markdown with deterministic markers.
4. Update version and user-facing documentation, then run full verification.

## Event Log

- Both products use Claude-family JSONL, so they reuse only the event decoder.
- Qoder is project-scoped. Its project key replaces path separators with `-`, preserves punctuation, and may store sessions directly or below `transcript/`.
- QoderWork is application-wide. Modern tasks run in internal workspaces, optional selected directories are additional directories rather than the session `cwd`, and sessions are direct JSONL files below first-level project-key directories.
- QoderWork's SQLite index omits legacy or detached transcripts, so discovery scans the transcript tree without depending on the database schema.
- Codex now reads the canonical session UUID from `session_meta` and treats canonicalized paths as equivalent.
- Application-wide providers are scanned once even during recursive pulls.
- Deterministic two-turn sessions exported correctly for Codex, OpenCode, Qoder, and QoderWork. Injected context remains part of the exported history by product decision.
- Cleanup review: Reopened to assess the provider seam and remove avoidable duplication without changing discovery or Markdown behavior.
- Cleanup: Claude, Qoder, and QoderWork now share internal main-session enumeration and JSONL parsing functions instead of reusing an entire provider adapter or duplicating directory scans.
- Cleanup: The provider interface supplies the common latest-session behavior and distinguishes history availability from optional CLI launchability; Qoder and QoderWork are pull-only.
- Validation: All automated checks and four real deterministic Markdown exports passed after the cleanup.
- Architecture: Added `--source` so a centralized parser can consume one uploaded provider artifact or a directory without local session discovery.
- OpenCode: Added official JSON export parsing while retaining read-only SQLite discovery for local use.
- Validation: File and two-file directory sources for Codex, OpenCode, Qoder, and QoderWork produced the expected Markdown; direct source and local discovery outputs were identical.
- Chore review: Reopened to validate the raw-artifact and centralized WayLog parsing seam for simplicity, locality, and consistency.
- Chore: Renamed provider history availability separately from CLI installation, removed duplicate provider validation, and kept direct source collection private to `pull`.
- Chore: Kept provider storage paths private, advertised only launchable agents, and changed batch synchronization to read one tracker entry instead of cloning all state per session.
- Source contract: A provider directory recursively includes contributor subdirectories, fails when it contains no artifacts, and always rebuilds supplied sessions so changed raw content cannot leave stale Markdown at the same message count.
- Test maintenance: Removed duplicate, no-op, no-assertion, exact-order, and process-global environment cases while retaining provider-format, source-ingestion, Markdown-integrity, and failure-semantics coverage.

## Outcome / Handoff

- Result: Version 0.3.1 has separate Qoder and QoderWork providers, OpenCode JSON export parsing, and one authoritative `--source` interface for a file or downloaded provider tree.
- Validation: 69 focused tests passed. Rustfmt, Clippy with warnings denied, release build, `git diff --check`, prior real four-provider exports, nested source parsing, and same-count source replacement passed.
- Follow-up: None.

## Promotion

- Promoted provider scope, shared Claude-family parsing, and the separation between pull availability and CLI launchability to durable documentation.
