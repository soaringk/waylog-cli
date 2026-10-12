# WayLog CLI

[![GitHub license](https://img.shields.io/github/license/soaringk/waylog-cli?style=flat-square)](https://github.com/soaringk/waylog-cli/blob/main/LICENSE)
![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg?style=flat-square)

**无缝同步、保留并本地化版本控制你的 AI 编程对话历史。**

WayLog CLI 是一个轻量级的工具，导出受支持的 AI 编程会话历史，将其导出为整洁、可搜索的本地 Markdown 文件。不要再因为会话过期而丢失上下文——WayLog CLI 帮你实现 AI 历史的本地所有权。

[English](README.md) | [中文文档](README_zh.md)

---

## ✨ 特性

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

### 全量同步 / 恢复历史 (`pull`)

`pull` 读取当前项目并写入其 `.waylog/history/`，不受父目录影响。`--recursive` 会额外扫描可见子项目并汇总到同一输出。使用 `--hidden` 纳入隐藏子目录，或使用 `--output-dir` 替换输出目录。

```bash
# 拉取当前项目的所有历史记录
waylog pull

# 将当前工作区及其子项目聚合到同一份历史中
waylog pull --recursive

# 将工具调用及结果输出为 Tool 段落
waylog pull --include-tool-calls

# 面向程序输出结构化 JSON，而不是 Markdown
waylog pull --format json

# 为任意 pull 模式指定输出目录
waylog pull --recursive --output-dir <目录>

# 只拉取一个本地 session，并输出到指定目录
waylog pull --provider opencode --session <session-id> --output-dir <目录>

# 直接解析单个原始记录或下载后的 provider 目录，不搜索本机历史目录
waylog pull --provider codex --source <conversation/codex> --output-dir <目录>
```

source 目录可以包含贡献者子目录。每个下载后的 provider 目录执行一次；传入的原始记录会重新生成对应 Markdown。多次写入同一个 `--output-dir` 时只更新本轮处理的 session，不删除其他文件。

默认省略工具调用。`--include-tool-calls` 会把可识别的调用和结果合并到同一个 `Tool` 段落，删除稳定的协议包装以提高可读性；无法安全标准化时回退到完整原生 payload。模式变化时会重写已有 Markdown。

### 输出格式

历史记录主要给人读，因此默认输出 Markdown。程序应使用 `--format json`：每个 session 输出一份结构化文档，层级与 Markdown 完全一致——`turns` 数组中，user 与 system 轮次带 `content`，assistant 轮次带模型产出的 `parts`，每个 part 的 `kind` 为 `reasoning`、`message` 或 `tool`，按记录顺序排列。

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

记录保持 provider 写入的顺序，也就是因果顺序：助手工作期间发来的输入会出现在两个 assistant 轮次之间，而不会被并入前一条。要读助手说过的话，取所有 `kind` 为 `message` 的 `parts`。

一个 `tool` part 就是一次完整交互：`content` 是调用，`result` 是它的返回，两者通过 `tool_call_id` 匹配。provider 会把并行调用成批写入，因此结果按 id 匹配，而不按位置匹配。没有匹配 id 的 tool 记录单独成为一个 part，按记录顺序排列，因为没有任何信息能把它关联到某次调用。`message_count` 统计 provider 写下的记录数，因此一次配对的交互计为两条。

不要靠匹配 Markdown 标题来还原结构：消息正文里可能出现与标题完全相同的行，因此从 Markdown 抽取本质上只能是近似的，这正是 `--format json` 存在的原因。缺失的值保持 `null`；`result`、`model`、`tokens`、`tool_call_id`、`tool_calls`、`thoughts` 只在 provider 确实记录时出现。

两种格式内容一致，文件名相同、仅扩展名不同，因此同一目录可以同时存放两者。

### 会话结构

每一轮用户输入是一个 `## 👤 User` 段落。助手在这一轮产出的全部内容归入同一个 `## 🤖 Assistant` 段落，其中各步骤按记录顺序嵌套：

| 子段落 | 内容 |
|--------|------|
| `### 🧠 Reasoning` | 一步可读的推理过程，仅在 provider 保存了其文本时出现。 |
| `### 💬 Message` | 助手写给你的一条回答。 |
| `### 🛠️ Tool` | 一次调用及其结果，仅在 `--include-tool-calls` 下输出。 |

多数 provider 会加密或直接丢弃逐字思维链。WayLog 只导出 provider 真正记录下来的推理文本，不重建其余部分，因此一轮助手输出中的推理步骤可能少于模型实际经历的步数。

![WayLog Pull Demo](demo/pull.gif)

## 📂 支持的供应商

| 供应商 | 状态 | 描述 |
|----------|--------|-------------|
| **Antigravity** | 🚧 Beta | CLI（`antigravity`）、2.0（`antigravity-2`）与 IDE（`antigravity-ide`）；读取各自 brain 目录中的全部会话。 |
| **Claude Code** | 🚧 Beta | 支持 Anthropic 的 `claude` 命令行工具。 |
| **Gemini CLI** | 🚧 Beta | 支持 Google 的 Gemini 命令行工具。 |
| **Codex** | 🚧 Beta | 支持 OpenAI Codex CLI。 |
| **OpenCode** | 🚧 Beta | 读取本地 SQLite 会话、指定的 SQLite 数据库及官方 JSON 会话导出。 |
| **Cursor** | 🚧 Beta | 只读访问应用级 `state.vscdb`，展开独立会话，保留原生顺序、思考和工具记录。 |
| **DeepSeek Harness** | 🚧 Beta | `dsh`；读取 `~/.dsh/sessions/` 中当前项目的最新代际 JSONL，支持 Zstandard 压缩及原生压缩整理。 |
| **Grok Build** | 🚧 Beta | `grok`；读取 `~/.grok/sessions/` 中当前项目的 `updates.jsonl` 与 `summary.json`，重放流式消息和 rewind。 |
| **Qoder** | 🚧 Beta | 从 `~/.qoder/projects/` 读取当前项目的 Qoder IDE 会话。 |
| **QoderWork** | 🚧 Beta | 从 `~/.qoderwork/projects/` 读取应用内全部 QoderWork 任务。 |

Claude 系列的源目录扫描会排除子会话记录。显式导入子会话或 sidechain 也会报错，因为它们可能复用父会话身份。

Qoder 按当前项目查找会话。QoderWork 任务通常没有工作目录，因此 `waylog pull --provider qoderwork` 会把全部 QoderWork 会话汇总到当前 WayLog 历史目录。

Codex 同时读取 `~/.codex/sessions/` 和 `~/.codex/archived_sessions/`，只查找当前项目的会话。结构化导出保留每轮上下文中记录的模型。仅包含事件消息的会话也可导出；如果会话提供 response items，则使用该记录流，不重复加入镜像事件消息。

Gemini 支持 JSON 快照和 JSONL 日志，按原记录顺序重放消息替换、完整检查点、局部修改及 rewind。如果旧 JSON 快照与对应的新 JSONL 日志共存，发现阶段会在迁移尚未完成时保留更完整的旧快照；新日志中明确的修改或 rewind 则以日志为准，即使消息变少。`--source` 指定的文件始终按提供内容解析。优先使用非空的 `displayContent`，保留已记录的思考摘要，并在 `--include-tool-calls` 模式下导出工具调用和结果。缺失或无效的时间保持为空；项目路径从 `.project_root` 读取，不把哈希值当作工作目录。子 Agent 会话不会混入主会话。

JSONL 解析器只忽略没有换行结尾、尚未写完整的最后一条 JSON 记录。其他损坏会使该会话导出失败，并保留原导出文件。普通 pull 会比较消息数量相同的导出内容，因此正文和元数据变化也会更新。已有会话经 rewind 清空后，导出会更新为空，不保留过期回答；其他文件不受影响。

Antigravity 的三个产品目录分别是 `~/.gemini/antigravity-cli/`、`~/.gemini/antigravity/` 和 `~/.gemini/antigravity-ide/`。它们按应用范围发现会话，未记录项目路径时保持为空。CLI 的 `history.jsonl` 或最近会话缓存可补充项目路径，项目映射缺失或不可读均不会阻止正文导出。

收集 Antigravity 历史时，需保留产品目录下的 `brain/<session-id>/.system_generated/logs/transcript[_full].jsonl` 结构。需要项目路径时，同时保留与 `brain/` 同级的 `history.jsonl` 或缓存。无法确定身份的散落 transcript 会报错；元数据只从所提供的目录树读取。

`--source` 指定单个文件时按原样解析；指定目录时由 provider 筛选原生历史文件，忽略辅助文件并选择有效代际。一个数据库可以导出多个独立会话；`--session` 可进一步按 ID 选取其中一个，未找到会报错。数据库中的损坏会话单独报错，不妨碍有效会话导出。

```bash
waylog pull --provider cursor --format json
waylog pull --provider dsh --source /path/to/dsh/sessions --include-tool-calls
waylog pull --provider cursor --source /path/to/state.vscdb --session CONVERSATION_ID
```

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
