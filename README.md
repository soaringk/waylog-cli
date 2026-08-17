# WayLog CLI

[![GitHub license](https://img.shields.io/github/license/soaringk/waylog-cli?style=flat-square)](https://github.com/soaringk/waylog-cli/blob/main/LICENSE)
![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg?style=flat-square)

**Seamlessly sync, preserve, and version-control your AI coding conversations locally.**

WayLog CLI is a lightweight tool written in Rust that automatically saves supported AI coding sessions into clean, searchable local Markdown files. Stop losing your context to session timeouts—WayLog CLI helps you own your AI history locally.

[中文文档](README_zh.md) | [English](README.md)

---

## ✨ Features

- **🔄 Auto-Sync**: `run` periodically synchronizes the latest session and performs a final sync when the agent exits.
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

### 1. Run an Agent with Sync (`run`)

Use `waylog run` instead of calling your AI tool directly. WayLog launches the agent and keeps its conversation history synchronized.

```bash
# Replace claude with another CLI-backed provider; Qoder and QoderWork are pull-only
waylog run claude
```

![WayLog Run Demo](demo/run.gif)

### 2. Full Sync / Recover History (`pull`)

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

Both formats hold the same content and use the same filename with a different extension, so one directory can hold both. `waylog run` always writes Markdown.

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
| **Antigravity** | 🚧 Beta | Supports the Antigravity CLI. |
| **Claude Code** | 🚧 Beta | Supports `claude` CLI tool from Anthropic. |
| **Gemini CLI** | 🚧 Beta | Supports Google's Gemini CLI tools. |
| **Codex** | 🚧 Beta | Supports OpenAI Codex CLI. |
| **OpenCode** | 🚧 Beta | Reads local SQLite sessions and official JSON session exports. |
| **Qoder** | 🚧 Beta | Pull-only; reads project-scoped Qoder IDE sessions from `~/.qoder/projects/`. |
| **QoderWork** | 🚧 Beta | Pull-only; reads application-wide QoderWork tasks from `~/.qoderwork/projects/`. |

Qoder follows the current project. QoderWork tasks usually have no working directory, so `waylog pull --provider qoderwork` collects all QoderWork sessions into the current WayLog history.

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
