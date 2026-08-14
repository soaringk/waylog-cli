# Tech Debt and Risks

## Provider coupling

- OpenCode parsing depends on its local SQLite schema and must track upstream changes.
- Provider project-location conventions can change across versions or platforms, so discovery tests must reflect actual layouts.
- Providers using the default `find_session` parse every project session, which can make targeted export slow for large histories.

## Content WayLog does not represent

- Codex multi-agent rollouts record sub-agent traffic as `agent_message` items with `author` and `recipient` fields. They stay unexported because attributing them to the main assistant would misstate who spoke; representing sub-agent turns is an open question.
- Non-text content, such as image inputs, is dropped by every provider. Neither export format has an agreed representation for it, and joining a request's text items loses the position an image held between them.
- Gemini parsing still substitutes wall-clock time for unparsable session timestamps, which conflicts with the no-fabrication constraint.

## Sync state

- Exports carry no layout version, so a release that changes export structure silently leaves equal-count files on the previous layout unless users are told to run `--force`.
- `--source` over a provider directory tree rebuilds sidechain and sub-agent transcripts that share their parent's session ID, so they collapse onto one export. Distinguishing them needs a session identity that includes the transcript, not only the ID.
- `exporter::json::parse_header` reads and parses a whole export to recover four fields that sit in its first lines, while the Markdown header costs a bounded 2 KB read. One scan over a 513-file history therefore reads 183 MB instead of 1.1 MB, and a pull builds one scan per provider. The fix is a `Deserialize` implementation that stops consuming the map once it has those fields, driven by `serde_json::Deserializer::from_reader` without calling `end()`.
- Sync state is recovered by scanning the whole output directory, so a pull reads every export's header once per provider even when it syncs one session. Deriving the export path from the parsed session instead would make that I/O proportional to the sessions synced and would remove any way to write to a path WayLog did not compute. The cost is that a release which shifts a filename leaves an orphaned export rather than reusing the old name, measured at 1 of 445 local sessions.

## Documentation

- The bilingual README files can drift; user-visible behavior changes must update both.
