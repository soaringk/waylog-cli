# Assistant trajectory completeness and a structured export format

- Status: completed
- Opened: 2026-08-14
- Completed: 2026-08-14
- Repo: `waylog-cli`
- Release: 0.4.0

## Request

- Exported sessions read as user input only; assistant output was missing or reduced to final conclusions.
- Record complete User and Assistant content for Codex, including intermediate trajectory, without inventing anything absent from the source.
- Keep tool calls and results behind the existing opt-in flag.
- Check Claude Code, OpenCode, and Qoder for the same class of defect and fix what is found.
- Add a JSON output mode so programs stop matching Markdown headings.

## Findings

Measured against 445 existing Codex exports and the raw provider files:

- `codex.rs` accepted only payloads carrying `role: user|assistant`, so `reasoning` items fell through and returned nothing. In one month of local history: 8276 reasoning items dropped, 2147 holding readable summary text. `developer` messages (1070) and `agent_message` items (33) were dropped the same way.
- `claude.rs` handled only `text` and tool payloads, so `thinking` blocks hit its catch-all arm: 1128 dropped against 2119 text blocks. `qoder` and `qoderwork` reuse this parser and inherited it. In teammate QoderWork artifacts the ratio inverts — 374 thinking against 238 text.
- `opencode.rs` collected reasoning into `MessageMetadata::thoughts` but attached it only to the first text part, losing it whenever a message had no text part: 12 of 42 reasoning-bearing messages.
- Codex emitted one message per content item. Because Codex injects transcripts as one request with up to 90 items, 5901 requests became 79223 sections. Assistant messages always carry exactly one item, so only the user side inflated.
- `codex.rs` reassigned `session_id` on every `session_meta` line, but 125 local rollouts replay the metadata of sessions they continue. Using the last occurrence collapsed 506 distinct sessions onto 445 identities, so 61 sessions silently overwrote another session's export.

## Outcome

- Modelled model output as one role: `MessageRole::Assistant(AssistantOutput)` with `Reasoning`, `Message`, and `Tool`. Reasoning is a distinct ordered record that cannot be mistaken for an answer, and cannot be mistaken for a separate speaker either.
- Codex records reasoning from `summary`, maps `developer` to `System`, joins a request's content items into one message, and takes identity from its own first `session_meta`. Reasoning holding only `encrypted_content` produces nothing.
- Claude-family parsing records `thinking` blocks in place; `redacted_thinking` stays absent.
- OpenCode emits reasoning parts in stream order. `MessageMetadata::thoughts` now serves only Gemini and Antigravity, whose native formats attach summaries to a message.
- Markdown groups consecutive assistant steps into one `## 🤖 Assistant` turn with `### 🧠 Reasoning`, `### 💬 Message`, and `### 🛠️ Tool` subsections in recorded order. Tool grouping by call ID is unchanged and still opt-in.
- Added `--format json`. `exporter` now owns a format-neutral seam — which records an export contains, how the turn structure is derived, how one is written, how its sync state is read back — with `exporter::markdown` and `exporter::json` behind it. `SessionTracker` restores state only from the format being written, so both formats coexist in one directory.
- `exporter::entries` derives the hierarchy once, so Markdown headings and JSON `turns`/`parts` cannot drift apart. This replaced a first attempt in which the formatter re-inferred turns from a flat list while JSON exposed the un-inferred form; the giveaway was needing `MessageRole::is_assistant_step` and a comment to explain the nesting.
- Removed `ChatMessage::id`. Nothing read it once the exporter owned its own wire structs, and its `uuid` fallbacks made exports non-deterministic for records without a provider id. The `uuid` dependency went with it, along with the `ClaudeEvent::uuid` and `AntigravityEvent::step_index` fields that only fed it.
- `Synchronizer` rewrites when the message count differs in either direction, so the smaller counts produced by joined requests no longer leave stale exports behind.

## Rejected

- A `layout_version` frontmatter discriminator was implemented in response to review and then removed. `UpToDate` skips only the write and never the parse, so it protected almost no work, and a bump depending on human memory would make a future stale export silent rather than visible. Equal-count histories are converted by one documented `--force`, recorded in `CHANGELOG.md` and `docs/risks/tech-debt-and-risks.md`.
- A speculative reasoning `content[]` branch was removed: no `reasoning` item in 69180 local records carries that field, so it was an untested fallback with no demonstrated failure mode.
- Codex `agent_message` sub-agent traffic stays unexported; attributing it to the main assistant would misstate who spoke.
- Trimming block whitespace was reverted: it silently stripped trailing spaces from provider text, which can carry meaning in Markdown. Spacing is now only ever added around content.

## Review findings

- Restoring sync state treated an absent `provider` as a match, so a pull could adopt and overwrite an unrelated file in an `--output-dir` that happened to carry the same `session_id`. Reproduced as real data loss in both formats, not only JSON as first reported. `provider` has been in the frontmatter since v0.1.0 and none of 450 local exports lack it, so the allowance protected a shape WayLog never writes; the match is now exact for every format and is covered by a CLI regression test.
- A session whose provider recorded no project location serialized as an empty value rather than `null`, in Markdown as well as JSON. Both now render an unrecorded project as `null` through one shared `exporter::project`, matching how absent timestamps already behave.

## Validation

- `cargo fmt --all -- --check`, 79 unit tests, 7 CLI integration tests, and Clippy with warnings denied passed.
- Independent ground truth over all 506 local Codex sessions: every session exported, and message counts matched a raw-JSONL recount for every session that was not being appended to during the run.
- Record-by-record parity on three real sessions: one Codex (13 user, 18 assistant, 4 developer, 43 readable reasoning, 56 call/result pairs, 35 encrypted-only reasoning items correctly absent), one Claude (204 text, 92 thinking, 119 user), and four teammate QoderWork sessions (238 text, 374 reasoning).
- Before and after runs over the same corpus with the previous binary: 445 exports became 506, and reasoning went from absent to recorded.
- JSON export cross-checked against the same session's Markdown counts, verified to keep tool records opt-in, and confirmed to survive a Markdown pull into the same directory. One session's structure checked end to end: 190 records equal 173 assistant parts plus 17 user and system turns, across the same 7 turns the Markdown shows.
- Restructuring the model was checked for Markdown neutrality by regenerating the whole corpus and comparing: 505 of 506 static sessions structurally identical, the exception being a session that grew during the run. Repeated after the review fixes: 507 of 509 identical, the exceptions being two sessions that grew.
