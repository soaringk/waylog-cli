# Lessons Learned

## Parsing provider history

- Test observed provider formats; their storage evolves independently of WayLog.
- A role-keyed parser silently drops every type it does not name. Readable reasoning and tool records need coverage independent of role-tagged messages.
- Provider histories replay other sessions verbatim, so metadata found anywhere in a file may belong to a session being continued. Take identity from the first record.
- Put targeted lookup in the provider that understands the storage, not in a scan over every session.

## Verifying an export

- Count parsed records against the raw file. Per-session parity finds losses that reading a sample never surfaces.
- Compare native records and the export together; parse success alone hides missing context and wrong identity.
- Never measure completeness by grepping headings out of Markdown. Quoted exports can contain the same headings as the enclosing document. Compare structural counts against native records.
- Tests that read a developer's real history copy private data. Use synthetic fixtures.

## Sync state

- Title-derived filenames are not identities. Include the provider session ID.
- Export ownership requires an exact provider, session ID and format match.
- Message count detects neither in-place source edits nor layout changes. Authoritative imports rebuild; normal pulls compare current rendered content.
- `UpToDate` skips the write, never the parse. Measure what a shortcut saves before building machinery to protect it.

## Design

- Only content you export can define an order. Only exported blocks split a request; turn boundaries must come from recorded input.
- Generated Markdown cannot describe its own structure. Programs need a structured format, not a stricter Markdown contract.
- Validate provider names before initializing, so invalid input has no side effects.
