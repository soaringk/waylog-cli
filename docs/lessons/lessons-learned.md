# Lessons Learned

## Parsing provider history

- Test observed provider formats, because their storage evolves independently of WayLog.
- A role-keyed parser silently drops every record type it does not name. Codex reasoning items carry no role, so they vanished from exports while parsing still reported success.
- Provider histories replay other sessions verbatim. Metadata found anywhere in a file may belong to a session being continued, so identity must come from the file's own first record.
- Put targeted lookup in the provider that understands the native storage instead of parsing every session.

## Verifying an export

- Count parsed records against the raw provider file. Per-session message-count parity finds losses that reading a sample never surfaces.
- Compare UI-visible messages, native records, and the export together; parse success alone reveals neither hidden context nor incorrect identity.
- Never measure completeness by grepping headings out of generated Markdown. Sessions that quote earlier exports place those same headings inside message text: 18 of 506 local exports contain such lines, one of them 37. Compare structural counts against the native records instead.
- Tests that pull from a developer environment can copy private histories; use synthetic fixtures and temporary directories.

## Sync state

- Title-derived filenames are not stable identities; include the provider session ID to prevent collisions.
- In a directory documented as a merge target, a lenient match is a data-loss hole: treating an absent `provider` as a match let a pull overwrite an unrelated file that merely shared a `session_id`. Prefer an exact match over an allowance for a shape the tool never writes.
- Message count detects neither in-place source edits nor representation changes. Authoritative imports must rebuild, and a layout change needs an explicit `--force` pull to reach existing exports.
- `UpToDate` skips only the export write, never the parse. Measure what a shortcut actually saves before building machinery to protect it.

## Design

- Generated Markdown cannot describe its own structure, so programs need a separate structured format rather than a stricter Markdown contract.
- Validate provider names before project initialization so invalid input has no prompt or filesystem side effects.
