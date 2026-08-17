# Decision Log

## Decisions

- **Provider isolation:** Each agent implements `Provider` and produces `ChatSession`, so sync and export stay generic.
- **Provider scope:** Providers declare whether their native history is project-scoped or application-wide so recursive recovery does not repeat global scans.
- **Exports as state:** Each export records the sync progress needed to resume it, avoiding a separate state file or database.
- **Recursive recovery:** `--recursive` aggregates visible descendant sessions into the current output; `--hidden` includes hidden descendants.
- **Explicit output:** Pull defaults to the invocation directory's `.waylog/history/`; `--output-dir` replaces it. Ancestor projects never affect pull, and repeated writes preserve unrelated files.
- **Explicit sources:** `--session` targets local provider history, while `--source` rebuilds supplied provider artifacts or directory trees. Both accept `--output-dir`.
- **Optional tool output:** `--include-tool-calls` renders tool records with stable wrappers stripped, falling back to the native value when stripping is unsafe.
- **Model output is one role:** Reasoning, answers, and tool records share `MessageRole::Assistant` and differ by `AssistantOutput`. Making reasoning a sibling would read as a separate speaker and force consumers to rediscover the grouping.
- **Reasoning follows the native record:** Providers that order reasoning emit `AssistantOutput::Reasoning` records in place. Providers that attach thought summaries to a message keep `MessageMetadata::thoughts`; inventing an order would be fabrication.
- **Two audiences, two formats:** Markdown is the default because people read histories; `--format json` serves programs. Markdown cannot describe its own structure, since message text can imitate headings, so a stricter Markdown contract was rejected.
- **One hierarchy for both formats:** `exporter::entries` splits a session once into standalone records, assistant turns, and the steps within a turn. Markdown renders `##`/`###`, JSON renders `turns`/`parts`. Neither invents a shape the other lacks, and neither counts a step the other merges.
- **A call and its result are one step:** A tool result joins the call carrying the same id, because a call is only readable next to what it returned. Providers batch parallel calls, so a result is matched by id and not by adjacency; only other tool records are ever passed over. A record with no matching id stands alone in recorded order, since nothing links it to a call and which end of an exchange it holds is not recorded. `message_count` still counts both records, because it reports what the provider recorded.
- **No export layout version:** Layout changes migrate by a documented one-time `--force`. `UpToDate` skips only the write, never the parse, so a discriminator would protect almost nothing, and a bump depending on memory turns a visible stale export into a silent one.
- **Recorded order is causal order:** Records keep the order the provider wrote them, in every provider. Providers record mid-turn input as an ordinary message, so a turn boundary is not observable and WayLog does not guess one. Grouping applies only to a run of model output, where the boundary is real.
- **The latest parse is canonical:** An export renders the session as parsed now, not an accumulated document. A pull replaces whatever occupies its derived path, so a damaged export repairs itself. Sync state decides whether to write, never whether writing is allowed.
- **Rollout self-identification:** Identity comes from the first metadata record in the file itself, because resumed and forked histories replay the metadata of sessions they continue.
