# Decision Log

## Decisions

- **Provider isolation:** Each agent implements `Provider` and produces `ChatSession`, keeping synchronization and export generation generic.
- **Provider scope:** Providers declare whether their native history is project-scoped or application-wide so recursive recovery does not repeat global scans.
- **Exports as state:** Each export records the sync progress needed to resume it, avoiding a separate state file or database.
- **Recursive recovery:** `--recursive` aggregates visible descendant sessions into the current output; `--hidden` includes hidden descendants.
- **Explicit output:** Pull defaults to the invocation directory's `.waylog/history/`; `--output-dir` replaces it. Ancestor projects never affect pull, and repeated writes preserve unrelated files.
- **Explicit sources:** `--session` targets local provider history, while `--source` rebuilds supplied provider artifacts or directory trees. Both accept `--output-dir`.
- **Optional tool output:** Providers detect structural tool records without closed type allowlists. `--include-tool-calls` renders readable payloads after removing stable protocol wrappers and preserves the complete native value when normalization is unsafe.
- **Model output is one role:** Reasoning, answers, and tool exchanges are all things the model produced, so they share `MessageRole::Assistant` and differ only by `AssistantOutput`. Making reasoning a sibling of the assistant would let a turn's steps be mistaken for separate speakers, and would force every consumer to rediscover the grouping.
- **Reasoning follows the native record:** Providers that store reasoning as ordered parts emit `AssistantOutput::Reasoning` records that keep their position in the stream. Providers whose format attaches thought summaries to a message keep using `MessageMetadata::thoughts`, because inventing an order for them would be fabrication.
- **Two audiences, two formats:** Markdown stays the default because histories are mostly read by people, and `--format json` serves programs. Markdown cannot describe its own structure, since message text may contain lines that look like headings, so a stricter Markdown contract was rejected in favour of a second format.
- **One hierarchy for both formats:** `exporter` splits a session into entries once — a user or system record, or one assistant turn holding every part the model produced. Markdown renders that as `##` and `###` headings and JSON as `turns` and `parts`, so neither format invents a shape the other lacks.
- **No export layout version:** Layout changes are migrated by a documented one-time `--force` rather than a version discriminator. `UpToDate` only skips the file write, never the parse, so the saving a discriminator would protect is negligible, and a bump that depends on human memory would turn a visible stale export into a silent one.
- **The latest parse is canonical:** An export is a rendering of the session as parsed now, not an accumulated document. A pull therefore replaces whatever occupies the path it derives, which keeps a truncated or hand-edited export repairable without `--force`. Sync state only decides whether that write is needed, never whether it is allowed.
- **Rollout self-identification:** A session's identity comes from the first metadata record in its own file, because resumed and forked histories replay the metadata of the sessions they continue.
