# WayLog CLI

[![GitHub license](https://img.shields.io/github/license/soaringk/waylog-cli?style=flat-square)](https://github.com/soaringk/waylog-cli/blob/main/LICENSE)
![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg?style=flat-square)

**Seamlessly sync, preserve, and version-control your AI coding conversations locally.**

WayLog CLI is a lightweight tool written in Rust that exports supported AI coding histories into clean, searchable local Markdown files. Stop losing your context to session timeouts—WayLog CLI helps you own your AI history locally.

[中文文档](README_zh.md) | [English](README.md)

---

## ✨ Features

- **📦 Project Recovery**: `pull` restores provider sessions associated with the current project.
- **🗂️ Workspace Recovery**: `pull --recursive` includes visible descendant projects and aggregates their sessions into the current project's WayLog history.
- **📝 Markdown Native**: All history is saved as high-quality Markdown files with frontmatter metadata.

## 🚀 Installation

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/soaringk/waylog-cli/main/scripts/install.sh | sh
```

### Windows PowerShell

```powershell
irm https://raw.githubusercontent.com/soaringk/waylog-cli/main/scripts/install.ps1 | iex
```

The installers select the matching GitHub Release binary for macOS, Linux, or Windows on x64 or ARM64 and verify its SHA-256 checksum. Set `WAYLOG_VERSION` (for example, `0.3.2`) before running the installer to pin a release instead of using the latest version.

## 💡 Usage

### Full Sync / Recover History (`pull`)

`pull` reads the current project and writes to its `.waylog/history/`; ancestors never affect it. `--recursive` also scans visible descendant projects and aggregates them into the same output. Use `--hidden` to include hidden descendants or `--output-dir` to replace the destination.

```bash
# Pull all history for the current project
waylog pull

# Recover one workspace, including descendant projects, into one history
waylog pull --recursive

# Include tool calls and results as Tool sections
waylog pull --include-tool-calls

# Write structured JSON for programs instead of Markdown
waylog pull --format json

# Choose the output directory for any pull mode
waylog pull --recursive --output-dir <directory>

# Pull one local session into a caller-managed directory
waylog pull --provider opencode --session <session-id> --output-dir <directory>

# Parse one uploaded artifact or a downloaded provider directory without local history discovery
waylog pull --provider codex --source <conversation/codex> --output-dir <directory>
```

A source directory may contain contributor subdirectories. Run the command once per downloaded provider directory; supplied artifacts regenerate their Markdown. Repeated pulls to one `--output-dir` update processed sessions without deleting other files.

Tool calls are omitted by default. `--include-tool-calls` groups each recognized request and response into one `Tool` section, removes stable protocol wrappers for readability, and falls back to the complete native payload when normalization is unsafe. Existing Markdown is rewritten when this mode changes.

### Output formats

Markdown is the default because histories are mostly read by people. Programs should use `--format json`, which writes one structured document per session with the same hierarchy the Markdown shows: a `turns` array where a user or system turn carries `content`, and an assistant turn carries the `parts` the model produced — each part a `reasoning`, `message`, or `tool` kind, in recorded order.

```json
{
  "provider": "codex",
  "session_id": "019ffb27-9f2f-7822-a84a-c3f244e5d57f",
  "project": "/path/to/project",
  "started_at": "2026-08-13T12:47:14.819+00:00",
  "updated_at": "2026-08-13T13:10:00.000+00:00",
  "message_count": 78,
  "include_tool_calls": false,
  "turns": [
    { "role": "user", "timestamp": "2026-08-13T12:47:14.819+00:00", "content": "..." },
    {
      "role": "assistant",
      "timestamp": "2026-08-13T12:48:08.449+00:00",
      "parts": [
        { "kind": "reasoning", "timestamp": "2026-08-13T12:48:08.449+00:00", "content": "..." },
        { "kind": "message", "timestamp": "2026-08-13T12:48:11.002+00:00", "content": "...", "model": "..." }
      ]
    }
  ]
}
```

Records stay in the order the provider wrote them, which is causal order: input sent while the assistant is working appears between assistant turns rather than folded into an earlier one. To read what the assistant said, take every `parts` entry whose `kind` is `message`.

A `tool` part is one exchange: the call in `content` and what it returned in `result`, matched by `tool_call_id`. Providers batch parallel calls, so a result is matched by its id rather than by position. A tool record with no matching id keeps its own part, in recorded order, because nothing links it to a call. `message_count` counts the records a provider wrote, so a paired exchange counts as two.

Do not recover structure by matching Markdown headings: message text can contain lines that look exactly like them, so extraction from Markdown is approximate by nature. `--format json` exists for that reason. Absent values stay `null`, and `result`, `model`, `tokens`, `tool_call_id`, `tool_calls`, and `thoughts` appear only when the provider recorded them.

Both formats hold the same content and use the same filename with a different extension, so one directory can hold both.

### Conversation layout

Each user turn is a `## 👤 User` section. Everything the assistant produced in reply belongs to one `## 🤖 Assistant` section, whose steps are nested in recorded order:

| Subsection | Content |
|------------|---------|
| `### 🧠 Reasoning` | One readable reasoning step, when the provider stored its text. |
| `### 💬 Message` | One answer the assistant addressed to you. |
| `### 🛠️ Tool` | One request and its result, only with `--include-tool-calls`. |

Providers encrypt or omit most verbatim chains of thought. WayLog exports only the reasoning text a provider actually recorded and never reconstructs the rest, so an assistant turn can hold fewer reasoning steps than the model took.

![WayLog Pull Demo](demo/pull.gif)

## 📂 Supported Providers

| Provider | Status | Description |
|----------|--------|-------------|
| **Antigravity** | 🚧 Beta | CLI (`antigravity`), 2.0 (`antigravity-2`) and IDE (`antigravity-ide`); reads all conversations in each surface’s brain directory. |
| **Claude Code** | 🚧 Beta | Supports `claude` CLI tool from Anthropic. |
| **Gemini CLI** | 🚧 Beta | Supports Google's Gemini CLI tools. |
| **Codex** | 🚧 Beta | Supports OpenAI Codex CLI. |
| **OpenCode** | 🚧 Beta | Reads local SQLite sessions, supplied SQLite databases and official JSON session exports. |
| **Cursor** | 🚧 Beta | Opens application-wide `state.vscdb` read-only and expands conversations with native ordering, reasoning and tools. |
| **DeepSeek Harness** | 🚧 Beta | `dsh`; reads the newest project-scoped JSONL generation under `~/.dsh/sessions/`, including Zstandard compression and native compaction. |
| **Grok Build** | 🚧 Beta | `grok`; reads project-scoped `updates.jsonl` and `summary.json` under `~/.grok/sessions/`, replaying streamed chunks and rewinds. |
| **Qoder** | 🚧 Beta | Reads project-scoped Qoder IDE sessions from `~/.qoder/projects/`. |
| **QoderWork** | 🚧 Beta | Reads application-wide QoderWork tasks from `~/.qoderwork/projects/`. |

Codex scans both `~/.codex/sessions/` and `~/.codex/archived_sessions/`, keeping discovery limited to the requested project. Structured exports attribute assistant parts to the model recorded in their turn context. Event-only rollouts are also supported; when response items are available, their stream is used without adding duplicate event messages.

Gemini reads JSON snapshots and replays JSONL replacements, checkpoints, patches and rewinds in provider order. When a legacy JSON snapshot and its matching JSONL successor coexist, discovery keeps the more complete snapshot until migration catches up; recorded edits and rewinds make the log authoritative even with fewer messages. Explicit `--source` files are always parsed as supplied. It uses nonempty `displayContent` when recorded, retains readable thoughts, and includes tool calls/results with `--include-tool-calls`. A missing or invalid time stays absent. The project path comes from `.project_root` when available; a hash is not a working directory. Subagent recordings are skipped.

JSONL parsers ignore an unfinished final JSON record only when it has no terminating newline. Other corruption fails that session and leaves its previous export untouched. Normal pulls compare equal-count exports with freshly rendered content, so message edits and metadata changes also refresh them. If an already exported conversation becomes empty after a rewind, its export is rewritten with no turns; unrelated files are untouched.

Claude-family source directories exclude subagent recordings. Explicit subagent/sidechain imports are rejected because they can reuse the parent session identity.

Qoder follows the current project. QoderWork tasks usually have no working directory, so `waylog pull --provider qoderwork` collects all QoderWork sessions into the current WayLog history.

Antigravity’s surface roots are `~/.gemini/antigravity-cli/`, `~/.gemini/antigravity/` and `~/.gemini/antigravity-ide/`. Discovery is application-wide; an unrecorded project path stays absent. CLI `history.jsonl` or its last-conversation cache can supply the project path, but missing or unreadable project metadata does not prevent transcript export.

For collected Antigravity histories, retain `brain/<session-id>/.system_generated/logs/transcript[_full].jsonl` beneath the copied surface root. Keep its `history.jsonl` or cache alongside `brain/` when project metadata is needed. Detached transcripts with an ambiguous identity are rejected. Metadata is read from the supplied tree.

An explicit `--source` file is parsed as supplied. For a directory, the provider selects native history artifacts, excluding auxiliary files and obsolete generations. A database can produce several independent sessions; combine `--session` with `--source` to select one by ID. A missing ID fails. Database imports report malformed conversations separately while exporting the valid ones.

```bash
waylog pull --provider cursor --format json
waylog pull --provider dsh --source /path/to/dsh/sessions --include-tool-calls
waylog pull --provider cursor --source /path/to/state.vscdb --session CONVERSATION_ID
```

### Development build

```bash
git clone https://github.com/soaringk/waylog-cli.git
cd waylog-cli
cargo build --release --locked
```

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

Distributed under the Apache License 2.0. See `LICENSE` for more information.
