# Tech Debt and Risks

## Provider coupling

- OpenCode and Cursor parsing depend on native SQLite schemas and must track upstream changes.
- Provider project-location conventions change across versions and platforms; discovery tests must match real layouts.
- Providers using the default `find_session` parse every project session, so targeted export is slow on large histories.

## Content WayLog does not represent

- Codex records sub-agent traffic as `agent_message` items with `author` and `recipient`. They stay unexported because attributing them to the main assistant misstates who spoke. Representing sub-agent turns is open.
- Non-text content such as images is dropped by every provider; neither format represents it. Adjacent text around an omitted block is joined within the original message.

## Sync state

- `exporter::json::parse_header` reads a whole export for four fields in its first lines, where Markdown reads 2 KB. One scan of a 513-file history costs 183 MB instead of 1.1 MB, once per provider. Fix: a `Deserialize` that stops consuming the map once it has those fields, over `Deserializer::from_reader` without `end()`.
- Sync state comes from scanning the whole output directory, so a pull reads every header once per provider even for one session. Deriving the path from the parsed session would make that I/O proportional to sessions synced and remove any way to write a path WayLog did not compute. Cost: a filename shift orphans the old export instead of reusing it, measured at 1 of 445 sessions.

## Documentation

- The bilingual READMEs drift; behaviour changes must update both.
