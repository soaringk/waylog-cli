# Session parsing and export hardening

Scope: prepare a 0.5.0 minor release by comparing existing providers with AI Session Export and checking provider-owned formats. No provider history is read or modified; validation uses synthetic fixtures.

Reference: AI Session Export commit `869cddb6537a0eba781f66019d197d4c3b273a27`. Gemini replay behavior checked against Google's `chatRecordingService.ts` and `chatRecordingTypes.ts` on 2026-10-12. The reference tool's clean-transcript filtering and blanket malformed-line skipping are not adopted.

Confirmed gaps: Codex archive discovery and per-turn models; append-only JSONL tails; Gemini checkpoint/replacement/patch/rewind replay, native tool results and display text; fabricated Gemini timestamps and hash-directory cwd; equal-count updates left stale by synchronization.

Implemented the complete reference Supported Sources inventory, retaining Qoder and QoderWork. Cursor, OpenCode and Second Mind expand batch artifacts through the shared provider interface. Directory import chooses native artifacts and current generations; explicit files remain authoritative.

Independent adversarial reviews found and resolved directory generation clobbering, subagent latest selection, Grok rewind model contamination and Antigravity launch cross-project selection. Native DSH schemas were checked against `deepseek-ai/deepseek-harness`; Cursor format evidence includes actual capture documentation from `skillsynchq/txcript`.

Validation on macOS: 114 unit tests and 15 CLI integration tests passed; formatting, locked all-feature Clippy with warnings denied, release build and diff checks passed. The release binary reports 0.5.0. Fixtures verify batch import, selected IDs, causal order, tool pairing, SQLite WAL reads, current compressed generations, rewinds and unchanged source bytes. Real contributor histories and other operating systems were not exercised. No commit, tag or release was published.
