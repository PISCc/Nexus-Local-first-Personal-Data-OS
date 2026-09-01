# Nexus

Nexus 是一个本地优先的个人数据操作系统，目标是把文件、笔记、代码和其他
个人资料组织成可靠、可检索、可追溯的本地基础。

当前项目已完成 M0 工程基础、M1 本地文件扫描和 M2.0 最小 Document 模型。下一步进入
M2.1 纯文本、Markdown 与代码解析；全文搜索、增量索引和人工智能功能尚未开始。

## 产品原则

- 默认本地运行，核心功能不依赖网络或人工智能。
- 用户拥有自己的数据；未经明确选择，不上传文件、路径、内容或搜索历史。
- 先建立确定性的索引和搜索，再考虑语义能力。
- 失败必须可观察、可解释，单个文件或目录异常不应拖垮整个任务。
- 优先保持简单、明确、可测试的模块边界。

## 当前进度

| 阶段      | 状态     | 结果                                       |
| --------- | -------- | ------------------------------------------ |
| M0.0–M0.3 | 已完成   | 工具链、仓库、Tauri 壳层和 Rust crate 边界 |
| M0.4      | 已完成   | SQLite 初始化、迁移和版本控制              |
| M0.5      | 已完成   | 启动日志、核心错误和启动状态               |
| M0.6      | 已完成   | 数据库、核心和前端状态测试                 |
| M0.7      | 已完成   | README、GitHub Actions 和 M0 架构决策      |
| M1.0      | 已完成   | 文件元数据模型、schema 2 和数据库读写入口  |
| M1.1      | 已完成   | 流式递归遍历、忽略路径和符号链接策略       |
| M1.2      | 已完成   | 有界批量事务写入、upsert 和失败统计        |
| M1.3      | 已完成   | 手动重扫、新增/更新/移除和安全统计         |
| M1.4      | 已完成   | 后台重扫、取消、进度事件和全中文界面       |
| M1.5      | 已完成   | 本地文件扫描验收和结果记录                 |
| M2.0      | 已完成   | 最小 Document 模型、来源和位置元数据边界   |
| M2.1      | 下一步   | 纯文本、Markdown 和代码解析                |

## 系统要求

桌面应用当前按 Windows MSVC 环境验证，需要：

- Windows 10 或更高版本。
- Node.js `>=22.23.2`。
- pnpm `11.19.0`。
- Rust stable toolchain，以及 Rustfmt、Clippy 和
  `x86_64-pc-windows-msvc` 目标。
- Visual Studio 2022 Build Tools 的 C++ 工具链和 Windows SDK。
- 可用的 WebView2 运行时。

不需要全局安装 Tauri CLI 或 SQLite CLI；Tauri CLI 已作为前端开发依赖锁定在
仓库中，SQLite 由 Rust 的 `rusqlite + bundled` 依赖提供。

## 安装

```text
git clone https://github.com/PISCc/Nexus-Local-first-Personal-Data-OS.git
cd Nexus
pnpm install --frozen-lockfile
```

Rust 会读取根目录的 `rust-toolchain.toml`。在 Windows 上，如果 Rust 或 C++
工具链尚未安装，请先完成系统要求中的安装，再执行项目命令。

## 开发与检查命令

所有命令都从仓库根目录执行：

| 命令             | 用途                             |
| ---------------- | -------------------------------- |
| `pnpm dev`       | 启动 Tauri 桌面应用              |
| `pnpm dev:web`   | 启动浏览器前端预览               |
| `pnpm format`    | 检查前端和 Rust 格式             |
| `pnpm lint`      | 执行 ESLint 和 Rust Clippy       |
| `pnpm typecheck` | 执行 TypeScript 和 Rust 类型检查 |
| `pnpm test`      | 执行前端与 Rust 测试             |
| `pnpm build`     | 构建前端和 Rust 工作区           |

浏览器预览没有 Tauri 桌面核心，因此启动状态会显示“降级 / 需处理”；这是
预览环境的诚实提示。使用 `pnpm dev` 启动桌面应用后，Tauri 会在应用数据目录
中初始化 `nexus.sqlite3`，正常情况下显示“本地 / 就绪”。

## 当前架构

M0 的 Rust 依赖方向是：

```text
nexus-desktop → nexus-core → nexus-db
```

核心层不依赖 Tauri，数据库层不假设 Tauri 路径。当前数据库包含
`nexus_metadata` 和 `file_metadata` 表，不包含文件正文或全文搜索索引。

长期数据流按以下方向演进：

```text
Source → Parser → Document → SQLite + FTS → Query Layer → Desktop UI
```

M1.0–M1.3 负责文件元数据模型、流式递归遍历、批量持久化和手动重扫；M1.4 将其接入后台任务、取消、进度事件和全中文界面，不实现内容解析、全文搜索或人工智能层。

M2.0 在 `nexus-core` 中定义最小 `Document` 模型：包含不透明 ID、本地文件来源、标题、正文和可选的 1-based 行范围；本地文件路径统一为绝对路径并使用数据库层的跨平台规范化规则。M2.0 不实现具体格式解析，也不引入新的解析依赖；下一步由 M2.1 负责纯文本、Markdown 和代码文件。

核心层的 `rescan_directory_with_control` 接收调用方传入的数据库路径、扫描根目录、取消控制器和进度回调，完成一次可取消的手动重扫；桌面层通过 `start_rescan`、`cancel_rescan`、`rescan-progress` 和 `rescan-finished` 接入任务控制。当前界面要求用户输入完整目录路径，暂不增加原生目录选择器依赖。

## 数据与隐私

- 数据库默认位于 Tauri 应用数据目录。
- 启动日志只输出到 stderr，只记录固定消息和安全错误分类。
- 日志不记录文件内容、文件名、搜索词或完整用户路径。
- 数据库初始化失败会进入可理解的降级状态，不会静默忽略。
- 项目当前不上传数据、不要求云服务，也不要求 LLM。
- M1 重扫只读取文件元数据；目录路径仅作为本地命令参数使用，不进入日志或事件说明。

## 项目文档

- [项目简报](./00_PROJECT_BRIEF.md)
- [路线图](./01_ROADMAP.md)
- [Codex 协作流程](./02_CODEX_WORKFLOW.md)
- [架构说明](./03_ARCHITECTURE.md)
- [M0 规格](./04_M0_SPEC.md)
- [执行计划](./05_EXECUTION_PLAN.md)
- [M0 架构决策记录](./docs/decisions/0001-m0-engineering-foundation.md)
- [M1.0 文件元数据决策记录](./docs/decisions/0002-m1-file-metadata-model.md)
- [M1.1 文件扫描器决策记录](./docs/decisions/0003-m1-file-scanner.md)
- [M1.2 文件元数据批量持久化决策记录](./docs/decisions/0004-m1-file-metadata-batch-persistence.md)
- [M1.3 手动重扫决策记录](./docs/decisions/0005-m1-manual-rescan.md)
- [M1.4 可取消重扫、进度事件和桌面界面决策记录](./docs/decisions/0006-m1-rescan-control-and-ui.md)
- [M1 本地文件扫描验收记录](./docs/acceptance/0001-m1-file-scanner.md)
- [M2.0 Document 模型决策记录](./docs/decisions/0007-m2-document-model.md)

## 开发约束

每个交付单元都按“调查 → 计划 → 实现 → 测试 → Review → 文档”推进。当前只
处理 M2.1 的本地纯文本与代码解析基础；在 M2 完成前，不进入全文搜索、云端同步、
多用户、认证或 Agent 系统。
