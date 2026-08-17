# One request is one message, one exchange is one step

- Status: completed
- Opened: 2026-08-14
- Completed: 2026-08-17
- Repo: `waylog-cli`
- Releases: 0.4.1, 0.4.2

## Request

- Finish the export-fidelity work started in 0.4.0: a provider request must appear as one message in every provider, not only in Codex.
- Then align the two formats. Markdown paired a tool call with its result while JSON exposed both separately, so the formats disagreed on how many steps a turn contained.
- Keep chronological order primary, and keep pairing honest when the linking id is missing.

## Findings

- Claude, Qoder, QoderWork, and OpenCode split one request into several turns. Over half of local QoderWork requests were affected, each `<system-reminder>` becoming its own user turn. Fixed by filtering to the blocks WayLog exports and joining the remaining text, so media WayLog drops no longer splits a request.
- The 2:1 tool-part ratio between the formats was not a design decision. `group_tool_exchanges` lived in the Markdown formatter, so `exporter::entries` did not in fact derive the whole hierarchy for both formats, despite documentation claiming it did.
- Measured 52 sessions, 9545 tool records: every tool record carried an id and no id appeared more than twice, so pairing is well defined in practice.
- Pairing is not adjacency. All 3183 Codex pairs are adjacent, but 301 Claude pairs are not, up to 17 records apart, because Claude batches parallel calls. Of the 1096 records passed over, every one is another tool record — no reasoning or answer is ever crossed, which is what makes id matching safe for the narrative.
- Three Claude calls have no result at all, from truncated sessions. They exercise the unpaired path in real data.

## Outcome

- `exporter::entries` now derives turns and the steps within them, and `Entry::AssistantTurn` carries `Part` groups. Markdown renders one section per step and JSON one entry per step, so neither format can count a step the other merges.
- A JSON `tool` part carries the call in `content` and its result in `result`. An unpaired record keeps `content` alone: nothing records which end of an exchange a lone record holds, so no field claims it.
- Pairing joins only a part still holding one record, so a third record reusing an id starts a new step instead of joining a completed exchange. This replaced a `HashMap` that would have absorbed it.
- `message_count` still counts provider records, because sync staleness compares it against the current parse. Pairing is presentation only.

## Verification

- Markdown is byte-identical to 0.4.1 across 37 Claude and Codex sessions, including the parallel-batch ones, so the shared grouping reproduces the previous behaviour exactly.
- JSON loses nothing: 8789 tool payloads became 4393 paired exchanges plus 3 unpaired records, multiset-equal per session, with turn counts, `message_count`, and non-tool parts unchanged.
- The formats now agree on tool steps in all 37 sessions, 4396 each, and on all six record kinds in 36 of 37.
- The remaining session differs only because its message text quotes real timestamped export headings; 8 of its extra headings carry timestamps outside the session window. Counting Markdown structure by matching headings is approximate by nature, which is why `--format json` exists.

## Rejected

- Naming the paired fields `call` and `result`. An unpaired record would have to sit in `call` while it might be a result, asserting polarity the provider never recorded.
- A `records` array per tool part. Honest about order, but it drops the readable call/result naming and shapes tool parts unlike every other kind.
- Verifying format alignment by parsing Markdown with fence tracking. A ``` line inside a tool payload desynchronises it, which produced false mismatches in 7 sessions before the instrument itself was corrected.
