# Nexus 执行方案

> 文档状态：规划稿
>
> 当前主目标：先完成 M0–M4，交付一个可靠的本地文件全文搜索工具。
>
> 约束：M0 未验收前，不进入 M1；M4 未形成稳定产品前，不主动实现 AI、Agent 或复杂扩展架构。

> 产品判断：M0–M4 就是第一个可实用版本；M5–M8 是在真实使用需求和评估结果支持下再决定的后续版本，不是必须完成的交付条件。

## 0. 当前项目状态

### 已完成

- 已完成项目目标、路线图、架构和 M0 规格的阅读与初步分析。
- 已确认 Nexus 的长期方向是 Local-first Personal Data OS。
- 已确认第一阶段最有价值的产品终点应是 M0–M4：本地文件索引、正文解析、全文搜索和增量更新。

### 当前仓库现状

- 已有 React/Vite 前端和 Tauri 2 桌面壳层，Rust workspace 已包含 desktop、
  `nexus-core` 和 `nexus-db` 三个当前 crate。
- README、CI 和架构决策已在 M0.7 加入；SQLite 层已在 M0.4 建立最小初始化和
  迁移能力，M0.5 已接入启动检查和降级状态。
- Node.js 和 pnpm 已存在。
- stable MSVC Rust toolchain、Rustfmt 和 Clippy 已安装并通过验证。
- Visual Studio 2022 Build Tools 的 MSVC x64 编译器和 Windows SDK 已安装并通过验证。
- 文档已统一放置在实际 Git 根目录；后续项目文件也应直接放在该根目录下。
- 初始同步前 Git 仓库没有 commit 和远程配置；当前已创建 `main` 初始提交并配置 GitHub `origin`。

### 当前阶段

当前处于：`M0.7` 本地实现完成，等待 GitHub Actions 首次运行后完成 M0 远程验收。

### M0.0 验证记录

- Node.js：`v22.23.2`
- pnpm：`11.19.0`
- Rust：`rustc 1.98.0`，active toolchain 为 `stable-x86_64-pc-windows-msvc`
- Cargo：`1.98.0`
- Rustfmt：`1.9.0-stable`
- Clippy：`0.1.98`
- MSVC：Visual Studio 2022 Build Tools，x64 编译器可用
- Git 根目录和工作区状态已验证；环境验证开始时没有未提交变更

### M0.1 配置记录

- 已创建根 `package.json`、`pnpm-workspace.yaml` 和 `pnpm-lock.yaml`。
- 已创建根 `Cargo.toml`、`Cargo.lock` 和 `rust-toolchain.toml`。
- 已创建 `.gitignore`，忽略依赖目录、构建产物、本地数据库和日志。
- 已提供前端与 Rust 的 format、lint、typecheck、test、build 根级命令入口。
- 当前 workspace 已包含首批 Rust crate；M0.3 不再创建未来阶段的占位 crate。

### M0.2 实现记录

- 已创建 `apps/desktop` React + TypeScript + Vite 前端。
- 已创建 Tauri 2 desktop crate、最小 capability 和 Vite 开发服务器连接配置。
- v0 界面、页面标题和窗口标题使用简体中文；核心逻辑仍未进入 UI。
- 前端 format、lint、typecheck、test、build 和 Tauri `cargo check --workspace`
  均已通过。

### M0.3 实现记录

- 已创建 `crates/nexus-core` 和 `crates/nexus-db` library crate。
- 已固定依赖方向：`nexus-desktop → nexus-core → nexus-db`。
- `nexus-core` 和 `nexus-db` 不依赖 Tauri。
- 本单元不引入 SQLite 驱动、数据库 schema、迁移或未来阶段的空 crate。

### M0.4 实现记录

- `nexus-db` 已采用 `rusqlite 0.40.2` 的 `bundled` 功能，SQLite 随 Rust
  依赖构建，不依赖系统 SQLite 安装。
- 已提供 `initialize_database(path)`，路径由调用方传入，不依赖 Tauri。
- 已添加 `0001_foundation.sql` 和 `PRAGMA user_version` 版本控制。
- 首个迁移只创建 `nexus_metadata` 元数据表，不创建文件索引或正文表。
- 已覆盖新建数据库、重复初始化、未知版本和不可访问路径错误。

### M0.5 实现记录

- Tauri 启动时将 `tracing` 输出到 stderr；日志只记录固定消息和安全的
  `error_kind`，不记录文件内容、文件名、搜索词或完整用户路径。
- `nexus-core` 已提供 `initialize(path)` 和 `CoreError`，负责传播数据库初始化
  结果，并提供非敏感的错误分类和中文用户说明。
- 桌面壳层使用 Tauri 应用数据目录，创建本地数据目录后初始化
  `nexus.sqlite3`；失败时进入 `degraded`，不会静默继续。
- 已加入 `get_startup_status` 命令，返回 `ready` 或 `degraded` 及非敏感中文
  说明；前端先显示 `loading`，再切换到对应状态。
- 已覆盖核心初始化成功、数据库错误安全映射、前端就绪状态和前端降级状态。
- 本单元新增 `serde`、`tracing` 和 `tracing-subscriber` 直接依赖；未引入
  异步运行时、持久化日志或云端服务。

### M0.6 实现记录

- 保留并验证 `nexus-db` 的隔离临时目录测试：新建数据库、重复初始化、未知
  schema 版本和不可访问路径均有覆盖。
- 保留并验证 `nexus-core` 的初始化成功与数据库错误传播测试，并检查错误展示
  不包含完整测试路径。
- React 页面通过 mock 的 `invoke("get_startup_status")` 覆盖 loading、ready、
  command 主动返回 degraded、command 拒绝和非法响应五类状态。
- 前端测试只使用 Vitest、jsdom 和内存 mock，不触碰真实用户目录或桌面数据库。
- M0.6 没有新增生产依赖、数据库表或运行时功能。

### M0.7 实现记录

- 已加入全中文 `README.md`，记录系统要求、安装、开发、检查、构建、架构、
  隐私边界和当前限制。
- 已将 Tauri CLI 加入 `apps/desktop` 的开发依赖，锁定在 pnpm lockfile 中，
  新终端不需要全局安装 Tauri CLI。
- 已加入 `.github/workflows/ci.yml`：Ubuntu 执行前端和 Rust 核心检查，Windows
  执行前端构建并编译 Tauri 桌面 crate。
- 已加入 M0 架构决策记录，并修正旧文档中对实际编号文件名的引用。
- 本地已模拟 README 与 CI 的全部命令并通过；GitHub Actions 尚未远程运行，需
  在提交并推送后完成最后验收。

### 初始同步状态

- 当前规划文档已经同步到 GitHub 远程仓库的 `main` 分支。
- M0.0–M0.4 的源码、锁文件和工程文档已提交并同步到 GitHub `main`；M0.5–M0.7
  当前在本地工作区，待 review 后再决定是否提交和同步。
- 当前已确认并实现 pnpm workspace、Tauri 2、`rusqlite + bundled`、启动日志、
  降级状态、CI 配置和 `nexus-core` / `nexus-db` 的最小结构；远程 CI 首跑仍待
  提交后确认。
- 不提交依赖目录、构建产物、日志文件或用户数据。

## 1. 执行原则

每个最小交付单元必须满足：

1. 只有一个清晰目的。
2. 有明确输入、输出和失败行为。
3. 可以单独测试。
4. 可以单独 review。
5. 不偷偷引入后续 milestone 的功能。
6. 完成后可以安全暂停，而不是必须继续下一单元才能验证。

标准循环：

```text
调查 → 方案确认 → 实现一个最小单元 → 测试 → Review → 更新文档
```

Codex 每次只接收一个最小交付单元。操作者负责确认边界、理解 diff、验收结果和决定是否继续。

## 2. 大板块总览

| 大板块 | 对应阶段 | 主要结果 | 是否属于第一版 |
|---|---|---|---|
| B0 工程决策与基线 | M0 前置 | 根目录、工具链、版本和边界确定 | 是 |
| B1 Engineering Foundation | M0 | 可启动的桌面应用和可测试的 Rust/SQLite 基础 | 是 |
| B2 File Scanner | M1 | 本地文件元数据索引 | 是 |
| B3 Content Parsing | M2 | 统一的文本 Document | 是 |
| B4 Full-text Search | M3 | 可用的正文搜索 | 是 |
| B5 Incremental Indexing | M4 | 文件变化后自动更新索引 | 是 |
| B6 Semantic Search | M5 | Embedding 与混合检索 | 否，属于 V2 |
| B7 Ask Nexus | M6 | 可追溯的资料问答 | 否，属于 V2 |
| B8 Personal Timeline | M7 | 资料活动时间线 | 否，待真实需求 |
| B9 Agent Layer | M8 | 受控的资料操作 Agent | 否，待真实需求 |
| B10 Quality and Hardening | 跨阶段 | 评估、诊断、恢复、隐私和发布质量 | 持续进行 |

## 3. 编码前必须确认的决定

以下决定会影响目录和 lockfile，不能由 Codex 静默决定：

| 决定 | 推荐默认值 | 备选方案 |
|---|---|---|
| 正式仓库根目录 | 使用当前 Git 根目录；本次同步已完成文档归位 | 在内部目录重新建立 Git 仓库 |
| 前端包管理器 | pnpm workspace | npm workspaces |
| Desktop 框架 | Tauri 2 | 其他 Tauri 版本 |
| SQLite Rust 驱动 | `rusqlite` + `bundled` | `sqlx` + Tokio |
| M0 测试 | Rust tests + Vitest + React Testing Library | M0 同时加入 Playwright |
| M0 日志 | `tracing` 输出 stderr，不记录敏感内容 | M0 加入持久化日志文件 |
| CI 平台 | Ubuntu 做前端/Core 检查，Windows 做 Tauri 编译 | 仅 Windows 或全平台矩阵 |
| Rust 结构 | 只创建 `nexus-core` 和 `nexus-db` | M0 同时创建未来所有 crate |
| 数据库失败行为 | 应用进入 degraded 状态并向 UI 报告 | 启动直接失败 |

没有确认前，不开始 M0 的实际代码实现。

## 4. M0 — Engineering Foundation

### M0.0：环境和仓库基线

**所属大板块：** B0 工程决策与基线

**目的：** 确保开发者和 CI 使用同一套仓库根目录、Node、pnpm、Rust 和 Windows 构建环境。

**所需知识：** Git 根目录、Node 包管理器、Rustup、Cargo、Windows MSVC 构建链。

**最小交付物：**

- 确认正式 Git 根目录。
- 安装并验证 stable MSVC Rust toolchain。
- 验证 Rustfmt、Clippy、Node 和 pnpm。
- 记录 Node、pnpm 和 Rust 版本策略。
- 建立第一个可审阅的 Git 基线。

**成品要求：**

- `rustc --version`、`cargo --version`、`pnpm --version` 可执行。
- `cargo fmt --version` 和 `cargo clippy --version` 可执行。
- Windows C++ Build Tools 可用于 Tauri 编译。
- 仓库根目录下可以找到文档、未来的 `.github` 和项目配置。

**测试与验收：**

- 新终端可以重复执行工具链检查。
- 不依赖全局安装的 Tauri CLI 或 SQLite CLI。

### M0.1：仓库和工具配置

**所属大板块：** B1 Engineering Foundation

**目的：** 建立可重复安装、检查和构建的项目入口。

**所需知识：** pnpm workspace、Cargo workspace、TypeScript 配置、版本锁定。

**最小交付物：**

- 根 `package.json` 和 pnpm workspace 配置。
- 根 `Cargo.toml`。
- `rust-toolchain.toml`。
- `.gitignore`。
- 前端和 Rust 的 format、lint、typecheck、test、build 脚本入口。

**成品要求：**

- 前端依赖和 Rust 依赖都有 lockfile。
- `node_modules`、`target`、数据库文件和日志不会进入 Git。
- 命令入口在根目录执行，不要求开发者进入多个目录手工拼接命令。

### M0.2：Tauri 桌面壳和 React 前端

**所属大板块：** B1 Engineering Foundation

**目的：** 让桌面应用和浏览器前端都能启动。

**所需知识：** React 组件、Vite、Tauri 配置、开发服务器、Tauri capabilities。

**最小交付物：**

- `apps/desktop` React + TypeScript 应用。
- 一个最小 `App` 页面。
- Tauri `src-tauri` 项目。
- Vite dev server 与 Tauri dev server 的连接配置。
- 最小 capability 配置。

**成品要求：**

- `pnpm dev:web` 可以启动前端开发服务器。
- `pnpm dev` 可以打开 Tauri 窗口。
- Tauri 前端不拥有数据库和文件系统业务逻辑。
- 默认不开放 filesystem、shell 或网络权限。

### M0.3：Rust workspace 和 Core 边界

**所属大板块：** B1 Engineering Foundation

**目的：** 建立最小但明确的 Rust 依赖方向。

**所需知识：** Rust crate、Cargo workspace、所有权、`Result`、模块和测试。

**最小交付物：**

- `nexus-core` library crate。
- `nexus-db` library crate。
- Tauri desktop crate。
- 依赖方向：desktop → core → db。
- Core 和 DB 不依赖 Tauri。

**成品要求：**

- `cargo test --workspace` 可以运行。
- `cargo clippy` 不产生 warning。
- 不创建未来的空 parser/index/search crate。
- 不引入 trait、事件总线或 DI 框架来解决尚不存在的问题。

### M0.4：SQLite 初始化和迁移

**所属大板块：** B1 Engineering Foundation

**目的：** 验证本地数据库能够安全创建和升级。

**所需知识：** SQLite connection、事务、schema version、错误处理。

**最小交付物：**

- `nexus-db` 的数据库打开函数。
- 一个 `0001_foundation.sql` 迁移文件。
- `PRAGMA user_version` 版本控制。
- 一个极小的元数据表，不包含文件索引和正文。
- 新建数据库、重复初始化和未知版本的处理。

**成品要求：**

- 数据库路径由调用方传入。
- 生产路径使用 Tauri app data directory。
- 测试路径使用隔离临时目录。
- 迁移在事务中执行。
- 不能通过删除数据库解决迁移问题。
- 不可访问路径返回带上下文的错误，不 panic。

### M0.5：日志、错误和启动状态

**所属大板块：** B1 Engineering Foundation

**目的：** 让启动失败可观察、可解释，并且不泄露用户数据。

**所需知识：** `tracing`、typed error、Tauri setup、前端 loading/error 状态。

**最小交付物：**

- Rust 启动日志初始化。
- Core 的初始化结果和错误类型。
- Tauri 启动状态 command。
- 前端状态显示：loading、ready、degraded。

**成品要求：**

- 日志不记录文件内容、文件名、搜索词或完整用户路径。
- 数据库失败不会被静默忽略。
- 前端收到可理解的非敏感错误状态。
- 正常环境不使用 `unwrap`、`expect` 处理可恢复错误。

### M0.6：M0 测试

**所属大板块：** B1 Engineering Foundation

**目的：** 证明工程骨架不是只能启动，且失败路径可验证。

**所需知识：** Rust unit/integration test、Vitest、React Testing Library、mock。

**最小交付物：**

- `nexus-db` 临时数据库测试。
- `nexus-core` 初始化和错误传播测试。
- React 页面测试。
- Tauri `invoke` bridge mock。

**成品要求：**

- 新数据库初始化成功。
- 重复初始化幂等。
- 错误状态正确返回。
- UI 可以显示成功和失败状态。
- 测试不触碰真实用户目录。

### M0.7：CI、README 和架构同步

**所属大板块：** B1 Engineering Foundation

**目的：** 让新开发者可以从 clone 到测试和 build 完成闭环。

**所需知识：** GitHub Actions、依赖缓存、README 编写、ADR。

**最小交付物：**

- `README.md`。
- GitHub Actions CI workflow。
- M0 架构 ADR。
- 更新架构文档和命令文档。

**成品要求：**

- CI 自动执行 format、lint、typecheck、test 和 build。
- Windows CI 至少完成一次 Tauri 编译。
- README 说明系统依赖、安装、开发、测试、lint 和 build。
- 文档统一使用仓库中实际存在的编号文件名。

### M0 完成门槛

只有以下条件全部满足，M0 才算完成：

- 新终端可以启动桌面应用。
- SQLite 可以在 app data directory 初始化。
- Rust 和前端测试均通过。
- Rustfmt、Clippy、ESLint、TypeScript 检查通过。
- 前端和桌面 build 通过。
- CI 通过。
- README 可被另一名开发者独立执行。

## 5. M1 — Local File Scanner

### M1.0：文件元数据模型

**所属大板块：** B2 File Scanner

**目的：** 定义真正需要保存的最小文件记录。

**所需知识：** Rust `Path`、文件元数据、时间戳、平台差异、SQLite schema。

**最小交付物：**

- path。
- filename。
- extension。
- size。
- modified/created/accessed timestamps（按平台可用性处理）。
- 文件类型或 MIME 的明确策略。
- hash 暂不默认计算，只在需求证明必要时加入。

**成品要求：**

- 记录模型与 UI 解耦。
- 路径规范化策略明确。
- 非 UTF-8 路径不会导致程序崩溃。
- symlink 策略明确并有测试。

### M1.1：递归遍历

**目的：** 扫描目录并逐条产出文件记录。

**所需知识：** streaming、目录遍历、权限异常、symlink、取消。

**成品要求：**

- 递归遍历目录。
- 单个文件或目录失败不终止整个扫描。
- ignored paths 可配置。
- 不跟随 symlink，除非明确开启。
- 结果可流式处理，不一次性载入全部路径。

### M1.2：元数据提取和批量持久化

**目的：** 将扫描结果安全写入 SQLite。

**所需知识：** SQLite transaction、批处理、upsert、数据库索引。

**成品要求：**

- 批量事务写入。
- 扫描期间被删除或修改的文件不导致全局失败。
- 失败记录可统计。
- 10 万级合成文件测试可以完成。

### M1.3：手动重扫闭环

**目的：** 先用可控的手动扫描验证元数据模型、数据库写入和索引一致性，再引入实时文件监听。

**成品要求：**

- 用户可以手动选择目录并重新扫描。
- 重扫可以正确新增、更新和移除文件记录。
- 重扫结果有明确的成功、失败和跳过统计。
- 这个闭环独立可用，不依赖 watcher。

### M1.4：取消、进度和 UI

**目的：** 用户可以启动、观察和取消扫描。

**所需知识：** Tauri command、后台任务、取消 token、进度事件。

**成品要求：**

- 扫描不阻塞 UI。
- 显示已处理数量、跳过数量和失败数量。
- 取消后数据库保持一致状态。
- 重复启动扫描的行为明确。

### M1.5：M1 验收

- 用户可以选择目录。
- 文件元数据写入本地数据库。
- 权限失败、删除、修改、symlink 和 ignored path 有测试。
- 单文件错误不会终止全局扫描。
- 100,000+ 文件测试有结果记录。

## 6. M2 — Content Parsing

### M2.0：最小 Document 模型

**所属大板块：** B3 Content Parsing

**目的：** 把文件内容转为搜索层可以使用的统一文本结果。

**所需知识：** 数据模型、编码、source/parser/document 边界。

**成品要求：**

- Document 至少包含稳定 ID、来源、标题、正文和基础位置元数据。
- 不创建通用插件系统。
- Parser 不依赖 UI。
- 不支持的格式有明确结果，而不是假装解析成功。

### M2.1：纯文本、Markdown 和代码

- 支持 txt、md、py、rs、js、ts、java、cpp。
- 处理 UTF-8 和必要的 BOM。
- 读取大小限制明确。
- 大文件不一次性无条件载入内存。
- 解析失败包含文件级错误信息。

### M2.2：JSON

- 支持合法 JSON 的文本化处理。
- 明确是否保留原始格式或生成规范化文本。
- malformed JSON 不终止整个批次。
- 嵌套深度和文件大小有边界。

### M2.3：PDF、DOCX、HTML

这是 M2 的第二阶段，不应阻塞纯文本和代码解析的首次交付。

- 每种格式单独实现和测试。
- 明确页码、段落或 DOM 位置是否保留。
- 解析库必须先评估体积、许可证和安全性。
- 恶意或损坏文档不得导致进程崩溃。

### M2.4：M2 验收

- 支持格式能生成统一 Document。
- 不支持和损坏格式可记录失败。
- 解析结果可以通过测试 fixture 重现。
- 不上传文件内容。

## 7. M3 — Full-text Search

### M3.0：搜索数据模型

**所属大板块：** B4 Full-text Search

**目的：** 把文档元数据和正文组织为可查询结构。

**成品要求：**

- 文档元数据表和正文索引表职责明确。
- 搜索结果可以追溯到原始文件。
- Schema migration 可升级。
- 不在 UI 中拼接 SQL。

### M3.1：SQLite FTS5 基础索引

- 建立正文索引。
- 支持插入、更新和删除。
- 明确 tokenizer 选择。
- 保证元数据与 FTS 数据的一致性。
- 先不引入 Tantivy。
- 只有在基准测试证明 FTS5 无法满足延迟、规模或查询能力要求时，才评估 Tantivy，并通过 ADR 记录决定。

### M3.2：查询语法和过滤器

- keyword。
- phrase。
- filename/path。
- extension/type。
- date filters。
- 非法查询返回可理解错误。

### M3.3：ranking 和 snippet

- 使用确定性 ranking。
- 返回匹配片段。
- 结果排序有测试。
- 搜索不依赖 LLM。

### M3.4：搜索 UI

- 输入查询。
- 显示结果、路径、类型、时间和 snippet。
- 处理 loading、空结果、错误和取消。
- 点击结果可以定位或打开原始文件，但权限和安全策略必须明确。

### M3.5：搜索质量评估

- 建立小型固定语料库。
- 建立真实查询集和相关性标注。
- 记录查询延迟、索引大小和召回情况。
- 对比修改前后的结果。

### M3.6：M3 验收

- 可以搜索正文，不只是文件名。
- 支持关键词、短语和基本过滤器。
- 结果带 snippet 和原文件引用。
- 查询性能和质量有记录。

## 8. M4 — Incremental Indexing

### M4.0：变更检测

**所属大板块：** B5 Incremental Indexing

**目的：** 识别文件是否需要重新处理。

**成品要求：**

- 先使用路径、大小、mtime 等便宜信息。
- hash 只用于确实需要的情况。
- 修改中的文件不会被错误地当成稳定内容。

### M4.1：文件事件接入

- CREATE。
- UPDATE。
- DELETE。
- MOVE/rename。
- 事件来源与扫描逻辑解耦。

### M4.2：去重、debounce 和批处理

- 合并短时间内重复事件。
- 不重复解析同一文件。
- 事件顺序异常时最终状态正确。
- 批处理失败可重试。

### M4.3：后台任务和关闭

- watcher 不阻塞 UI。
- 任务可以取消。
- 应用关闭时停止接收新事件。
- 未完成批次不会破坏数据库一致性。

### M4.4：M4 验收

- 创建、修改、删除和移动文件后索引最终一致。
- 临时文件和编辑器原子保存有测试。
- 重复事件不会产生重复记录。
- 关闭和重启后可以恢复。

## 9. M5–M8 后续执行单元

这些阶段只有在 M4 稳定并且有真实使用需求时才启动。

### M5 — Semantic Search

**所属大板块：** B6 Semantic Search

最小顺序：

1. 固定 embedding 模型和数据范围。
2. 明确本地模型、用户自带 API 或云端 API 的隐私边界。
3. 建立向量存储和版本标记。
4. 实现 lexical + vector fusion。
5. 使用 M3 的评估集比较结果。
6. 只有实验证明必要时才加入 reranking。

不允许用语义搜索替换确定性全文搜索。

### M6 — Ask Nexus

**所属大板块：** B7 Ask Nexus

最小顺序：

1. Query analysis。
2. 检索和 rerank。
3. Context 构造。
4. LLM 生成。
5. 文件、页码、片段引用。
6. 失败、幻觉和 prompt injection 测试。

回答没有可追溯来源时，不应显示为可信答案。

### M7 — Personal Timeline

**所属大板块：** B8 Personal Timeline

最小顺序：

1. 定义什么是时间线事件。
2. 只使用已有文件活动和索引数据。
3. 支持按时间过滤。
4. 先提供事实视图，再考虑总结。

如果无法证明比普通文件排序更有价值，应暂停该阶段。

### M8 — Agent Layer

**所属大板块：** B9 Agent Layer

最小顺序：

1. 只读工具：search、read、find related、timeline。
2. 工具参数和结果严格校验。
3. 每次调用可审计。
4. 默认禁止修改和删除用户文件。
5. 增加任务取消、预算和失败恢复。
6. 先做半自动流程，再考虑自主执行。

不应在 M8 之前为 Agent 提前建立复杂插件平台。

### 第一阶段实用化加固（贯穿 M1–M4）

**所属大板块：** B10 Quality and Hardening

这些能力不属于单独的“炫技功能”，但决定第一阶段产品能否长期使用：

- 一键全量重建索引。
- 查看索引状态、失败数量和最近一次运行结果。
- 对失败文件进行重试。
- 配置 ignored paths 和 ignored extensions。
- 明确数据库位置和占用空间。
- 提供安全的清空索引操作，不删除原始文件。
- 提供基础诊断信息，且不记录或上传用户文件内容。

它们应在 M3/M4 的真实流程稳定后按最小需求加入，不应在 M0 提前设计完整的运维平台。

## 10. 所需知识路线

| 阶段 | 需要达到的知识程度 |
|---|---|
| M0 | 能读写基础 Rust/TypeScript，理解 Cargo、React、Tauri、SQLite 和测试 |
| M1 | 理解文件系统、路径、权限、symlink、元数据和批量数据库写入 |
| M2 | 理解文本编码、解析失败、资源限制和统一数据模型 |
| M3 | 学会倒排索引、tokenizer、FTS5、BM25、query parsing 和检索评估 |
| M4 | 理解文件事件、竞态、去重、debounce、取消和一致性 |
| M5 | 理解 Embedding、向量检索、召回率、Fusion 和 reranking |
| M6 | 理解 RAG、引用、幻觉、prompt injection 和隐私边界 |
| M7 | 理解时间建模和事件来源可信度 |
| M8 | 理解工具调用、权限、审计、任务状态和 Agent 评估 |
| 全程 | Git、测试驱动思维、性能测量、日志、错误处理和代码 review |

不要求在进入某阶段前完全掌握所有知识。要求是在实现该阶段时能够解释关键设计，并能独立判断测试是否覆盖风险。

## 11. 成品要求

### 第一阶段产品（M0–M4）

最终至少应满足：

- 用户可以选择本地目录。
- 文件元数据和正文可以本地建立索引。
- 可以按关键词、短语、路径、扩展名和时间搜索。
- 搜索结果包含 snippet 和原文件位置。
- 文件变化后索引可以更新。
- 权限、删除、移动、损坏文件不会导致整个任务崩溃。
- 支持 10 万级文件的测试结果有记录。
- 不需要网络、LLM 或云端服务。
- 用户数据不会默认上传。
- 有 README、架构图、测试、CI 和错误诊断方式。

### 每个交付单元的完成标准

- 代码只覆盖当前单元。
- 单元测试或集成测试已补充。
- 相关 format、lint、typecheck 和 build 已执行。
- 失败路径已检查。
- 没有新增未确认的重大依赖或公共接口。
- Codex 输出实际执行过的命令和结果。
- 操作者完成 diff review。

## 12. 人与 Codex 的分工

### Codex 负责

- 调查当前代码和文档。
- 提出局部方案。
- 实现一个最小交付单元。
- 编写测试。
- 执行检查和 build。
- 解释错误和修复方案。
- 进行只读 review。

### 人负责

- 产品取舍。
- 大型架构决定。
- 依赖和公共接口审批。
- 隐私和安全行为审批。
- 阅读 diff。
- 判断测试是否有意义。
- 最终验收和是否进入下一单元。

### 推荐的单元执行模板

```text
只实现 <单元 ID>：<单元名称>。

先阅读相关文档和当前代码。
不要实现其他 milestone。
不要加入未批准的依赖、抽象或公共接口。

完成后执行：
1. 相关测试
2. format
3. lint
4. typecheck
5. build

最后报告：
1. 修改了哪些文件
2. 实现了什么行为
3. 执行了哪些命令及结果
4. 没有执行哪些检查及原因
5. 仍存在的风险
```

## 13. 时间和节奏建议

以每周 10–15 小时、Codex 负责主要编码为参考：

| 阶段 | 估计时间 |
|---|---:|
| B0 + M0 | 1–2 周 |
| M1 | 3–5 周 |
| M2 | 3–6 周 |
| M3 | 4–8 周 |
| M4 | 3–6 周 |
| 第一阶段完成 | 约 3–6 个月 |
| M5–M6 | 额外约 2–4 个月 |
| M7–M8 | 额外约 3–9 个月 |

推荐节奏：一周完成 1–3 个小单元，每个单元都经过测试和 review；不要为了追求进度一次让 Codex 修改几十个文件。

## 14. 主要风险和停止条件

### 主要风险

- 工程复杂度增长速度超过操作者的理解速度。
- M1 文件系统边界处理不足。
- M3 没有客观搜索质量评估。
- 过早引入 Tantivy、Embedding 或 Agent。
- 未来结构被空 crate、空接口和通用抽象占满。
- 只验证“能运行”，没有验证索引一致性和失败恢复。

### 应暂停并重新评估的情况

- 不能解释当前模块职责和数据流。
- 测试只是为了让 CI 变绿，没有覆盖实际风险。
- 为解决一个局部问题引入大型框架。
- M3 搜索质量没有改善，却继续扩展到 M5。
- 产品没有真实使用者或真实语料。
- Codex 连续修改多个边界模块而没有中间 review。

## 15. 当前下一步

M0.0、M0.1、M0.2、M0.3、M0.4、M0.5、M0.6 和 M0.7 的本地实现已完成。下一步
是提交后确认 GitHub Actions 首次运行；CI 通过后再进入 M1，不提前实现文件扫描、
正文解析或搜索功能。
