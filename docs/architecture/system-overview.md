# System Overview

## Purpose

WayLog is a local-first Rust CLI that turns coding-agent histories into readable Markdown without changing provider-owned data.

## Modes

- `waylog run` launches one agent and keeps its latest session synchronized while the process is active.
- `waylog pull` recovers sessions associated with the invocation directory and writes to its `.waylog/history/`.
- `waylog pull --recursive` adds visible descendant projects to that recovery scope, `--hidden` includes hidden descendants, and `--output-dir` explicitly replaces the destination.
- `waylog pull --provider <provider> --session <id>` targets one session in local provider history and may write to a caller-managed directory.
- `--source` parses supplied provider artifacts or a downloaded provider directory tree without local session discovery, allowing collectors and a centralized parser to remain separate while preserving upload grouping.
- `--include-tool-calls` adds calls and results as `Tool` messages, removes stable protocol wrappers for readability, and falls back to the complete native payload when normalization is unsafe.

## Core Boundaries

- `providers::base::Provider` owns native storage discovery, project matching, session lookup, and conversion into `ChatSession`.
- Direct source parsing bypasses discovery, treats supplied artifacts as authoritative, and still crosses the same `Provider::parse_session` seam.
- Provider history availability is independent of the optional CLI launch command, so application-only providers participate in `pull` without pretending to support `run`.
- Claude-family providers share JSONL parsing and main-session enumeration while keeping product-specific storage discovery behind `Provider`.
- Qoder is project-scoped, while QoderWork is application-wide and scans its task workspaces once per pull.
- `Synchronizer` owns provider-independent sync behavior and status reporting.
- `SessionTracker` reconstructs provider-specific sync state from Markdown frontmatter, avoiding a second state store.
- `exporter::markdown` owns the Markdown and frontmatter representation.
- `output` owns human and JSON terminal output.

## Distribution

- GitHub Releases provide checksum-verified binaries for macOS, Linux, and Windows on x64 and ARM64 so users do not need a Rust toolchain.

## Evolution Notes

- Add new agent formats behind `Provider`; do not branch shared synchronization logic for provider-specific behavior.
