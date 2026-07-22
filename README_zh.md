# WayLog CLI

[![GitHub license](https://img.shields.io/github/license/soaringk/waylog-cli?style=flat-square)](https://github.com/soaringk/waylog-cli/blob/main/LICENSE)
![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg?style=flat-square)

**无缝同步、保留并本地化版本控制你的 AI 编程对话历史。**

WayLog CLI 是一个轻量级的工具，自动捕捉并存档受支持的 AI 编程会话，将其导出为整洁、可搜索的本地 Markdown 文件。不要再因为会话过期而丢失上下文——WayLog CLI 帮你实现 AI 历史的本地所有权。

[English](README.md) | [中文文档](README_zh.md)

---

## ✨ 特性

- **🔄 自动同步**：`run` 会定期同步最新会话，并在 agent 退出时执行最终同步。
- **📦 项目历史恢复**：`pull` 恢复各工具关联到当前项目的会话。
- **🗂️ 工作区历史恢复**：`pull --recursive` 纳入可见子项目，并把会话聚合到当前项目的 WayLog 历史中。
- **📝 Markdown 原生**：所有历史记录均保存为带 Frontmatter 元数据的高质量 Markdown 文件。

## 🚀 安装

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/soaringk/waylog-cli/main/scripts/install.sh | sh
```

### Windows PowerShell

```powershell
irm https://raw.githubusercontent.com/soaringk/waylog-cli/main/scripts/install.ps1 | iex
```

安装脚本会根据 macOS、Linux、Windows 以及 x64、ARM64 架构选择对应的 GitHub Release 预编译文件，并校验 SHA-256。运行前可设置 `WAYLOG_VERSION`（例如 `0.3.2`）固定版本；默认安装最新版本。

## 💡 使用方法

### 1. 同步运行 Agent (`run`)

使用 `waylog run` 代替直接调用 AI 工具。WayLog 会启动代理并持续同步它的对话历史。

```bash
# 可将 claude 替换为其他 CLI provider；Qoder 和 QoderWork 仅支持 pull
waylog run claude
```

![WayLog Run Demo](demo/run.gif)

### 2. 全量同步 / 恢复历史 (`pull`)

`pull` 读取当前项目并写入其 `.waylog/history/`，不受父目录影响。`--recursive` 会额外扫描可见子项目并汇总到同一输出。使用 `--hidden` 纳入隐藏子目录，或使用 `--output-dir` 替换输出目录。

```bash
# 拉取当前项目的所有历史记录
waylog pull

# 将当前工作区及其子项目聚合到同一份历史中
waylog pull --recursive

# 为任意 pull 模式指定输出目录
waylog pull --recursive --output-dir <目录>

# 只拉取一个本地 session，并输出到指定目录
waylog pull --provider opencode --session <session-id> --output-dir <目录>

# 直接解析单个原始记录或下载后的 provider 目录，不搜索本机历史目录
waylog pull --provider codex --source <conversation/codex> --output-dir <目录>
```

source 目录可以包含贡献者子目录。每个下载后的 provider 目录执行一次；传入的原始记录会重新生成对应 Markdown。多次写入同一个 `--output-dir` 时只更新本轮处理的 session，不删除其他文件。

![WayLog Pull Demo](demo/pull.gif)

## 📂 支持的供应商

| 供应商 | 状态 | 描述 |
|----------|--------|-------------|
| **Antigravity** | 🚧 Beta | 支持 Antigravity CLI。 |
| **Claude Code** | 🚧 Beta | 支持 Anthropic 的 `claude` 命令行工具。 |
| **Gemini CLI** | 🚧 Beta | 支持 Google 的 Gemini 命令行工具。 |
| **Codex** | 🚧 Beta | 支持 OpenAI Codex CLI。 |
| **OpenCode** | 🚧 Beta | 读取本地 SQLite 会话和官方 JSON 会话导出。 |
| **Qoder** | 🚧 Beta | 仅支持 pull；从 `~/.qoder/projects/` 读取当前项目的 Qoder IDE 会话。 |
| **QoderWork** | 🚧 Beta | 仅支持 pull；从 `~/.qoderwork/projects/` 读取应用内全部 QoderWork 任务。 |

Qoder 按当前项目查找会话。QoderWork 任务通常没有工作目录，因此 `waylog pull --provider qoderwork` 会把全部 QoderWork 会话汇总到当前 WayLog 历史目录。

### 开发构建

```bash
git clone https://github.com/soaringk/waylog-cli.git
cd waylog-cli
cargo build --release --locked
```

## 🤝 贡献

欢迎贡献！请随时提交 Pull Request。

## 📄 许可证

基于 Apache License 2.0 许可证分发。详见 `LICENSE` 文件。
