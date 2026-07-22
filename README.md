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

# Choose the output directory for any pull mode
waylog pull --recursive --output-dir <directory>

# Pull one local session into a caller-managed directory
waylog pull --provider opencode --session <session-id> --output-dir <directory>

# Parse one uploaded artifact or a downloaded provider directory without local history discovery
waylog pull --provider codex --source <conversation/codex> --output-dir <directory>
```

A source directory may contain contributor subdirectories. Run the command once per downloaded provider directory; supplied artifacts regenerate their Markdown. Repeated pulls to one `--output-dir` update processed sessions without deleting other files.

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
