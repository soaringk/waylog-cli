# Tech Debt and Risks

## Open Risks

- OpenCode parsing depends on its local SQLite schema and must track upstream changes.
- Providers using the default `find_session` parse every project session, which can make targeted export slow for large histories.
- Provider project-location conventions can change across versions or platforms, so discovery tests must reflect actual layouts.
- The bilingual README files can drift; user-visible behavior changes must update both.
