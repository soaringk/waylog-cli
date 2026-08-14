# Lessons Learned

## Parsing provider history

- Test observed provider formats; their storage evolves independently of WayLog.
- A role-keyed parser silently drops every type it does not name. Codex reasoning carries no role, so it vanished while parsing still reported success.
- Provider histories replay other sessions verbatim, so metadata found anywhere in a file may belong to a session being continued. Take identity from the first record.
- Put targeted lookup in the provider that understands the storage, not in a scan over every session.

## Verifying an export

- Count parsed records against the raw file. Per-session parity finds losses that reading a sample never surfaces.
- Compare native records and the export together; parse success alone hides missing context and wrong identity.
- Never measure completeness by grepping headings out of Markdown. Sessions quoting earlier exports put those headings inside message text: 18 of 506 local exports, one with 37. Compare structural counts against native records.
- Tests that read a developer's real history copy private data. Use synthetic fixtures.

## Sync state

- Title-derived filenames are not identities. Include the provider session ID.
- In a merge target, a lenient match is a data-loss hole: an absent `provider` counted as a match, letting a pull overwrite a file that merely shared a `session_id`. Never allow for a shape the tool never writes.
- Message count detects neither in-place source edits nor layout changes. Authoritative imports rebuild; layout changes need `--force`.
- `UpToDate` skips the write, never the parse. Measure what a shortcut saves before building machinery to protect it.

## Design

- Only content you export can define an order. A dropped block splitting a request in two looked like fidelity but showed the reader nothing, while a turn boundary that providers do not record cannot be inferred at all.
- Generated Markdown cannot describe its own structure. Programs need a structured format, not a stricter Markdown contract.
- Validate provider names before initializing, so invalid input has no side effects.
