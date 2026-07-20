# System Overview

## Purpose

WayLog is a local-first Rust CLI that turns coding-agent histories into readable Markdown without changing provider-owned data.

## Modes

- `waylog run` launches one agent and keeps its latest session synchronized while the process is active.
- `waylog pull` recovers sessions associated with the current project.
- `waylog pull --recursive` treats the visible current directory tree as one workspace recovery scope and aggregates descendant sessions into the resolved tracking root, which may be an ancestor of the invocation directory; `--hidden` expands that scope, and neither mode creates independent outputs for nested projects.
- `waylog pull --provider <provider> --session <id>` targets one provider session for hook integration and may write to a caller-managed directory.

## Core Boundaries

- `providers::base::Provider` owns native storage discovery, project matching, session lookup, and conversion into `ChatSession`.
- `Synchronizer` owns provider-independent sync behavior and status reporting.
- `SessionTracker` reconstructs sync state from Markdown frontmatter, avoiding a second state store.
- `exporter::markdown` owns the Markdown and frontmatter representation.
- `output` owns human and JSON terminal output.

## Distribution

- GitHub Releases provide checksum-verified binaries for macOS, Linux, and Windows on x64 and ARM64 so users do not need a Rust toolchain.

## Evolution Notes

- Add new agent formats behind `Provider`; do not branch shared synchronization logic for provider-specific behavior.
