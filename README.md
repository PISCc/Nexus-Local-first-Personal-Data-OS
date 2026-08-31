# Nexus

Nexus 是一个本地优先的个人数据操作系统，目标是把文件、笔记、代码和其他
个人资料组织成可靠、可检索、可追溯的本地基础。

当前项目处于 M0 工程基础阶段。M0.0–M0.6 已完成，M0.7 的 README、CI 和架构
决策已在本地完成；首次 GitHub Actions 运行是 M0 的最后远程验收项。文件扫描、
正文解析、全文搜索和人工智能功能尚未开始。

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
| M0.7      | 本地完成 | README、GitHub Actions 和 M0 架构决策      |
| M1        | 待开始   | 本地文件扫描                               |

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

核心层不依赖 Tauri，数据库层不假设平台路径。当前数据库只包含
`nexus_metadata` 元数据表，不包含文件索引或正文。

长期数据流按以下方向演进：

```text
Source → Parser → Document → SQLite + FTS → Query Layer → Desktop UI
```

M0 不实现文件扫描、内容解析、全文搜索或人工智能层。

## 数据与隐私

- 数据库默认位于 Tauri 应用数据目录。
- 启动日志只输出到 stderr，只记录固定消息和安全错误分类。
- 日志不记录文件内容、文件名、搜索词或完整用户路径。
- 数据库初始化失败会进入可理解的降级状态，不会静默忽略。
- 项目当前不上传数据、不要求云服务，也不要求 LLM。

## 项目文档

- [项目简报](./00_PROJECT_BRIEF.md)
- [路线图](./01_ROADMAP.md)
- [Codex 协作流程](./02_CODEX_WORKFLOW.md)
- [架构说明](./03_ARCHITECTURE.md)
- [M0 规格](./04_M0_SPEC.md)
- [执行计划](./05_EXECUTION_PLAN.md)
- [M0 架构决策记录](./docs/decisions/0001-m0-engineering-foundation.md)

## 开发约束

每个交付单元都按“调查 → 计划 → 实现 → 测试 → Review → 文档”推进。当前只
处理 M0 的工程基础；在 M0 验收完成前，不进入 M1，也不提前建立复杂插件、云端
同步、多用户、认证或 Agent 系统。
