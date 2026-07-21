# Lessons Learned

## Lessons

- Title-derived filenames are not stable identities; include the provider session ID to prevent collisions.
- Tests that pull from a developer environment can copy private histories; use synthetic fixtures and temporary directories.
- Validate provider names before project initialization so invalid input has no prompt or filesystem side effects.
- Test observed provider formats because their storage evolves independently of WayLog.
- Compare UI-visible messages, native records, and Markdown; parse success alone does not reveal hidden context or incorrect identity.
- Put targeted lookup in the provider that understands the native storage instead of parsing every session.
- Message count does not detect in-place source changes; authoritative imports must rebuild instead of relying on incremental state.
