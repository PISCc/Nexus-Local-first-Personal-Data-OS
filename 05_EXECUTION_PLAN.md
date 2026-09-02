# Nexus 执行方案

> 文档状态：执行记录与后续规划
>
> 当前主目标：维护 M0–M4 的可靠本地文件全文搜索，并完成已启动的 M5 向量混合检索基线。
>
> 当前约束：按里程碑顺序推进；M4 已形成稳定产品，M5 只完成本地基线验证，不在未做模型决策前引入
> 预训练模型、云端 API、Agent 或复杂扩展架构。

> 产品判断：M0–M4 就是第一个可实用版本；M5–M8 是在真实使用需求和评估结果支持下再决定的后续版本，不是必须完成的交付条件。

## 0. 当前项目状态

### 已完成

- 已完成项目目标、路线图、架构和 M0 规格的阅读与初步分析。
- 已确认 Nexus 的长期方向是 Local-first Personal Data OS。
- 已确认第一阶段最有价值的产品终点应是 M0–M4：本地文件索引、正文解析、全文搜索和增量更新。
- 已完成 M5.0–M5.4 的本地向量混合检索工程基线；真正的预训练模型仍等待单独的产品和运行时决策。

### 当前仓库现状

- 已有 React/Vite 前端和 Tauri 2 桌面壳层，Rust workspace 已包含 desktop、
  `nexus-core` 和 `nexus-db` 三个当前 crate。
- README、CI 和架构决策已建立；SQLite 已升级到 schema 5，当前包含
  `nexus_metadata`、`file_metadata`、canonical `documents`、`documents_fts`、模型登记和本地
  `document_embeddings`，启动检查、文件扫描、重扫和增量监听界面均已接入。
- Node.js 和 pnpm 已存在。
- stable MSVC Rust toolchain、Rustfmt 和 Clippy 已安装并通过验证。
- Visual Studio 2022 Build Tools 的 MSVC x64 编译器和 Windows SDK 已安装并通过验证。
- 文档已统一放置在实际 Git 根目录；后续项目文件也应直接放在该根目录下。
- Git 仓库已配置 `main` 分支和 GitHub `origin`；M0–M2.0 的实现与文档已完成一次提交并推送。

### 当前阶段

当前处于：`M5.0–M5.4` 本地向量混合检索基线完成，M5 评估已通过；M6 尚未开始。

### M0.0 验证记录

- Node.js：`v22.23.2`
- pnpm：`11.19.0`
- Rust：`rustc 1.98.0`，active toolchain 为 `stable-x86_64-pc-windows-msvc`
- Cargo：`1.98.0`
- Rustfmt：`1.9.0-stable`
- Clippy：`0.1.98`
- MSVC：Visual Studio 2022 Build Tools，x64 编译器可用
- Git 根目录和工作区状态已验证；这是 M0.0 开始时的基线记录。

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
- 本地已模拟 README 与 CI 的全部命令并通过；GitHub Actions 已完成首次远程运行，
  前端检查、Rust 核心检查和 Windows Tauri 编译全部通过。

### M1.0 实现记录

- 已将数据库 schema 升级到版本 2，并加入 `0002_file_metadata.sql`。
- 已增加 `FileMetadata` 模型、绝对路径规范化、平台相关路径键和展示路径。
- 已增加单条文件元数据 upsert 与按路径读取入口；数据库层不依赖 UI 或 Tauri。
- 已明确 Unix epoch 毫秒时间戳、可选类型标签、不默认计算哈希以及不跟随符号链接的策略。
- 已覆盖新数据库迁移、schema 1 升级、重复 upsert、无效路径、路径规范化和非 UTF-8 路径边界。
- M1.0 交付时未实现目录递归、批量事务、手动重扫、取消、进度或界面操作；后续单元已分别覆盖目录递归和批量事务。

### M1.1 实现记录

- 已在 `nexus-core` 增加实现 `Iterator` 的 `FileScanner`，逐项产出文件、跳过或失败结果。
- 已支持递归目录遍历、规范化忽略路径和单项错误隔离；结果不会一次性累积为完整路径列表。
- 默认不跟随符号链接；明确开启时通过解析后的目录身份去重，避免目录环路和重复遍历。
- 已覆盖嵌套文件、忽略路径、不可访问根目录和 Windows 下的默认路径行为；Unix 专属的
  符号链接与非 UTF-8 路径测试已加入条件编译测试。
- M1.1 交付时未实现数据库写入、扫描统计、取消、后台任务或 UI 进度；M1.2 只补齐批量数据库边界，不提前组合手动重扫。

### M1.2 实现记录

- 已在 `nexus-db` 增加有界的 `upsert_file_metadata_batch`，输入可以逐项提供成功记录或失败结果。
- 每个有界批次在独立 SQLite 事务中执行 upsert；已提交批次不会因后续批次失败而回滚。
- 输入错误和元数据校验失败会计入 `FileMetadataBatchSummary.failed` 并继续处理；数据库级错误保留为明确错误返回。
- 已增加 `received`、`written`、`failed` 和 `batches` 统计，并覆盖零批次大小、批次提交、单条失败隔离和重复路径 upsert 边界。
- 已完成 100,000 条合成文件元数据测试；测试使用 1,024 条一批，验证了有界批处理和最终行数。
- M1.2 不实现目录重扫、旧记录删除、取消、后台任务或 UI 进度；这些内容留给 M1.3 和 M1.4。

### M1.3 实现记录

- 已在 `nexus-core` 增加 `rescan_directory`，组合目录扫描器和数据库重扫会话；调用方可传入任意已选择的扫描根目录。
- 已在 `nexus-db` 增加 `FileMetadataRescan`，使用连接级临时表记录本次见到的路径和失败/跳过保护路径，不改变持久化 schema。
- 手动重扫会正确新增和更新文件记录；扫描确认不存在的旧文件记录会被移除，失败或跳过路径及其子路径会被保留。
- 已返回成功、失败、跳过、移除和已提交批次五项统计；数据库级错误仍会明确返回，不伪装成单文件失败。
- 已覆盖首次扫描、更新文件、删除文件、新增文件、忽略路径保留、相邻目录边界和零批次大小；Unix 专属的符号链接失败保护测试已加入条件编译。
- M1.3 不实现目录选择器、后台任务、取消、实时 watcher 或 UI 进度；这些内容留给 M1.4。

### M1.4 实现记录

- `nexus-core` 已增加可在线程之间共享的 `RescanControl`、`RescanProgress` 和带回调的 `rescan_directory_with_control`；取消检查位于每个扫描结果前和最终提交前。
- `nexus-db` 的重扫会话将成功元数据先写入连接级 pending 临时表；未进入 `finish` 的取消任务不会把本轮半成品应用到持久化 `file_metadata`。
- Tauri 已增加单活动重扫管理器、后台线程和 `scanId`；`start_rescan`、`cancel_rescan`、`get_rescan_status` 负责命令边界，重复启动会被明确拒绝。
- 已增加 `rescan-progress` 和 `rescan-finished` 事件；进度、完成、取消和失败状态均使用稳定的 `camelCase` payload 与非敏感中文说明。
- React 桌面页已扩展为全中文文件索引工作区：输入完整目录路径后可以启动/取消重扫，显示已处理、成功、跳过、失败、移除旧记录和批次统计。
- 前端会校验 command/event payload 并忽略不属于当前任务的事件；浏览器预览无 Tauri 核心时仍显示可理解的降级界面。
- 已新增 M1.4 决策记录 `docs/decisions/0006-m1-rescan-control-and-ui.md`；未引入新依赖，原生目录选择器留给 M1.5 评估。

### M1.5 验收记录

- 已通过 M1 当前约定范围验收：用户可以输入完整目录路径启动本地重扫，文件元数据写入 SQLite，扫描失败可隔离统计，取消不会提交半成品数据。
- 已确认 M1.0–M1.4 的代码测试覆盖新增、更新、移除、权限/单项失败、删除期间变化、符号链接、忽略路径、重复任务和取消一致性。
- 已确认 100,000 条合成文件元数据按 1,024 条一批完成；Windows 本地测试通过，Unix 专属测试保留给 Unix CI 环境执行。
- 已实际启动 `pnpm dev` 的 Tauri 开发构建，窗口标题为 `Nexus — 文件索引`，启动日志确认本地核心就绪。
- 目录选择当前采用完整路径输入，不新增原生目录选择器依赖；原生选择器是否值得引入留给后续真实使用反馈。
- 正式验收记录见 `docs/acceptance/0001-m1-file-scanner.md`。

### M2.0 实现记录

- 已在 `crates/nexus-core/src/document.rs` 定义 `Document`、`DocumentId`、`DocumentSource` 和 `DocumentLocation`，统一表达来源、标题、正文和可选行范围。
- `DocumentSource::LocalFile` 要求非空路径，转为绝对路径，并复用 `nexus-db` 的跨平台路径规范化规则；不做 `canonicalize`，避免把模型绑定到文件当前是否存在。
- 已加入非空 ID、非空标题、有效 1-based 行范围和非空来源路径校验；错误分类不会回显用户正文或完整路径。
- 已覆盖本地文件模型构造、空正文、位置范围和非法输入测试；本阶段不实现具体 Parser，不增加第三方依赖。
- 已新增决策记录 `docs/decisions/0007-m2-document-model.md` 和验收记录
  `docs/acceptance/0002-m2-document-model.md`。

### M2.1 实现记录

- 已在 `crates/nexus-core/src/parser.rs` 增加单文件 `parse_local_file`，由调用方提供
  `DocumentId`、本地路径和最大字节数；未创建通用 Parser trait 或 parser crate。
- 支持 `txt`、`md`、`py`、`rs`、`js`、`ts`、`java`、`cpp`，扩展名大小写不敏感；Markdown
  和代码按普通 UTF-8 文本读取，不进行 AST 解析或格式化。
- 使用文件元数据检查和有界读取拒绝超过上限的文件；只移除开头 UTF-8 BOM，保持其余正文
  和换行不变；标题使用完整文件名，位置使用 `whole_document`。
- 已增加安全的 `ParseError` 分类，覆盖格式、元数据、普通文件、打开、读取、大小限制、
  UTF-8 和 Document 构造失败；展示消息不包含路径、文件名或正文。
- `nexus-core` 解析器不写 SQLite、不接入 Tauri 或 UI，不新增第三方依赖；M2.2 JSON、
  PDF、DOCX、HTML 和全文搜索仍未开始。
- 已新增决策记录 `docs/decisions/0008-m2-content-parser.md` 和验收记录
  `docs/acceptance/0003-m2-content-parser.md`。

### M2.2 实现记录

- 已在 `crates/nexus-core/src/parser.rs` 增加单文件 `parse_json_file`，由调用方提供
  `DocumentId`、本地路径、最大字节数和最大嵌套深度，并从 `nexus-core` 公共导出。
- 已增加直接依赖 `serde_json 1.0.151`；只接受扩展名大小写不敏感的 `.json` 普通文件，
  严格校验 UTF-8 并只移除开头 BOM。
- JSON 合法性通过后生成统一 `Document`，正文保留去除 BOM 后的原始空白、换行、缩进
  和字段顺序；不执行格式化、字段抽取或 schema 校验。
- `max_bytes` 复用 M2.1 的有界文件读取；`max_depth` 以根标量为 0、容器层级递增，
  超限返回独立的 JSON 深度错误。malformed JSON 返回独立的安全分类。
- 已覆盖合法 JSON、原始正文、大小写扩展名、根标量深度、超深 JSON、malformed JSON
  和不支持扩展名；解析器仍不写 SQLite、不接入 Tauri/React、不实现批量调度。
- 已新增决策记录 `docs/decisions/0009-m2-json-parser.md` 和验收记录
  `docs/acceptance/0004-m2-json-parser.md`。

### M2.3a 实现记录

- 已在 `crates/nexus-core/src/parser/html.rs` 增加单文件 `parse_html_file`，由调用方提供
  `DocumentId`、本地路径、原始输入字节上限和提取后输出字节上限，并从 `nexus-core`
  公共导出。
- 已增加直接依赖 `dom_query 0.27.0`；该版本已经存在于工作区锁文件，HTML 解析使用
  HTML5 容错规则，不手写 HTML tokenizer，也不建立新的通用 Parser trait。
- 只接受大小写不敏感的 `.html` / `.htm` 普通文件，严格校验 UTF-8 并只移除开头 BOM；
  `script`、`style`、`noscript` 和 `template` 内容被排除，正文由可见文本和块边界规范化生成。
- 输入文件使用 M2.1 的有界读取；提取结果超过调用方输出上限时返回独立的
  `parse_output_too_large` 安全分类。标题使用完整文件名，位置使用
  `whole_document`，不改变 `Document` 模型。
- 已覆盖 HTML 可见文本、非内容元素、实体、大小写扩展名、malformed HTML、BOM、无效
  UTF-8、零限制和输出超限；解析器仍不写 SQLite、不接入 Tauri/React、不实现批量调度。
- 已新增决策记录 `docs/decisions/0010-m2-html-parser.md` 和验收记录
  `docs/acceptance/0005-m2-html-parser.md`。

### M2.3b 实现记录

- 已在 `crates/nexus-core/src/parser/docx.rs` 增加单文件 `parse_docx_file`，由调用方提供
  `DocumentId`、本地路径、原始 ZIP 字节上限、主文档 XML 条目上限和提取后输出字节上限，
  并从 `nexus-core` 公共导出。
- 已增加直接依赖 `zip 8.6.0`（仅启用 `deflate-flate2-zlib-rs`）和 `quick-xml 0.41.0`；
  DOCX 只读取 `word/document.xml`，不把 ZIP 条目解压到文件系统。
- 只接受大小写不敏感的 `.docx` 普通文件；ZIP 无效、主文档条目缺失/非文件、条目超限、
  XML 无效和输出超限均返回独立安全分类。XML 文本支持实体、段落、换行和制表符。
- 标题使用完整文件名，位置使用 `whole_document`，不改变 `Document` 模型；不执行宏、
  不读取页眉/页脚/批注/关系目标，不写 SQLite、不接入 Tauri/React、不实现批量调度。
- 已覆盖 Deflate DOCX fixture、实体、大小写扩展名、ZIP 无效、主文档缺失、条目上限、
  输出上限、malformed XML 和安全错误展示。
- 已新增决策记录 `docs/decisions/0011-m2-docx-parser.md` 和验收记录
  `docs/acceptance/0006-m2-docx-parser.md`。

### GitHub 同步状态

- GitHub 远程仓库为 `https://github.com/PISCc/Nexus-Local-first-Personal-Data-OS.git`。
- 已推送提交 `18b1d6c`，`main` 和 `origin/main` 当时同步，包含 M0.0–M2.0 的源码、测试和工程文档。
- 本次进度对齐会新增文档修改；完成检查后再单独决定是否创建文档提交并推送。
- 当前已确认并实现 pnpm workspace、Tauri 2、`rusqlite + bundled`、启动日志、
  降级状态、CI 配置和 `nexus-core` / `nexus-db` 的最小结构；远程 CI 首跑已确认
  通过。
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

- 用户可以通过输入完整路径选择目录。
- 文件元数据写入本地数据库。
- 权限失败、删除、修改、symlink 和 ignored path 有测试。
- 单文件错误不会终止全局扫描。
- 100,000+ 文件测试有结果记录。

## 6. M2 — Content Parsing

### M2.0：最小 Document 模型

**所属大板块：** B3 Content Parsing

**目的：** 定义文件内容进入后续搜索层时使用的统一文本 `Document` 边界；本单元不实现具体格式解析。

**所需知识：** 数据模型、编码、source/parser/document 边界。

**成品要求：**

- Document 至少包含稳定 ID、来源、标题、正文和基础位置元数据。
- 本地文件来源使用绝对路径，并通过统一的跨平台路径规范化规则表达。
- ID、标题、来源路径和行范围的非法输入必须返回安全、可分类的错误。
- 不创建通用插件系统。
- Parser 不依赖 UI。
- 具体格式支持和解析失败契约留给 M2.1；本单元不得假装已经支持格式解析。

### M2.1：纯文本、Markdown 和代码

- 支持 txt、md、py、rs、js、ts、java、cpp。
- 处理 UTF-8 和必要的 BOM。
- 读取大小限制明确。
- 大文件不一次性无条件载入内存。
- 解析失败包含文件级错误信息。

### M2.2：JSON

**状态：已完成。**

- 通过 `parse_json_file(document_id, path, max_bytes, max_depth)` 支持单文件 JSON
  文本化处理。
- 使用 `serde_json` 校验合法性；正文只移除开头 BOM，保留原始格式，不生成规范化文本。
- malformed JSON 返回固定安全错误，不改变解析器状态，调用方可以继续处理后续文件。
- `max_bytes` 限制原始文件大小；根标量深度为 0，容器每增加一层深度加 1，超过
  `max_depth` 返回独立错误。
- 不实现 JSON schema、字段抽取、批量调度或正文持久化。

### M2.3：PDF、DOCX、HTML（已完成）

这是 M2 的第二阶段，不应阻塞纯文本和代码解析的首次交付。

- 每种格式单独实现和测试。
- 明确页码、段落或 DOM 位置是否保留。
- 解析库必须先评估体积、许可证和安全性。
- 恶意或损坏文档不得导致进程崩溃。

#### M2.3a：HTML（已完成）

- 使用 `parse_html_file(document_id, path, max_input_bytes, max_output_bytes)` 生成统一
  `Document`。
- 过滤脚本、样式、noscript 和 template 内容，输出确定性的可见文本。
- 严格 UTF-8、输入文件大小和提取后正文大小均有边界；malformed HTML 使用 HTML5
  容错规则继续提取可用文本。

#### M2.3b：DOCX（已完成）

- 使用 `parse_docx_file(document_id, path, max_input_bytes, max_entry_bytes, max_output_bytes)`
  提取 `word/document.xml` 的主文档文本。
- 先检查 ZIP 条目未压缩大小再读取；使用流式 XML 事件解析，支持段落、换行、制表符和实体。
- 不解压到文件系统，不执行宏或资源，不读取页眉/页脚/批注/关系目标；不提前扩展
  `DocumentLocation`。

#### M2.3c：PDF（已完成）

- 使用 `parse_pdf_file(document_id, path, max_input_bytes, max_decompressed_bytes,
  max_output_bytes)` 在内存中按 PDF 逻辑页序提取文本。
- 选择 `lopdf 0.44.0`，关闭默认日期、并行和时间特性；使用 `LoadOptions` 和页面有界
  文本提取 API 限制单个解压流，避免把压缩流无限展开到内存。
- 非空页面经过边缘空白裁剪后以两个换行连接；结果保持完整文件名和
  `DocumentLocation::whole_document()`，不扩展页码位置模型。
- 原始输入、解压流和提取后正文分别受限；PDF 无效、解压流超限、输出超限、零限制、
  不支持扩展名和解析器异常均返回安全分类。不渲染页面、不执行 OCR、不处理密码、附件
  或外部资源，不写 SQLite、不接入 Tauri/React、不实现批量调度。
- 已覆盖多页页序、损坏 PDF、安全错误展示、输入上限、解压流上限、输出上限、扩展名和
  零限制；新增决策记录 `docs/decisions/0012-m2-pdf-parser.md` 和验收记录
  `docs/acceptance/0007-m2-pdf-parser.md`。

### M2.4：M2 验收

**状态：已完成。**

- 当前支持的纯文本/Markdown/代码、JSON、HTML/HTM、DOCX 和 PDF 入口都能生成统一
  `Document`，并保持来源、完整文件名、正文和 whole-document 位置边界。
- 不支持扩展名、缺失或损坏文件、无效编码、结构错误和各类资源超限均通过
  `ParseError::kind()` 返回可记录的安全分类；错误展示不包含路径、文件名或正文。
- 每种格式均有隔离临时目录或库生成的可重现 fixture；核心测试、数据库测试、桌面端测试
  和前端测试均已通过。
- 解析只在本地内存中完成，不上传文件内容，不写正文 SQLite，不接入 UI 批量调度，不依赖
  网络或 AI。
- 正式验收记录见 `docs/acceptance/0008-m2-content-parsing.md`。

## 7. M3 — Full-text Search

### M3.0：搜索数据模型

**所属大板块：** B4 Full-text Search

**目的：** 把文档元数据和正文组织为可查询结构。

**状态：** 已完成。

**成品要求：**

- canonical 文档表和后续正文索引表职责明确。
- 搜索结果可以追溯到原始文件。
- Schema migration 可升级。
- 不在 UI 中拼接 SQL。

**本单元交付：**

- schema 3 的 `documents` canonical 表，保存文档 ID、本地来源路径、标题、正文和可选行范围。
- `source_path_key` 和 `source_path_display` 双路径表示，支持原始文件追溯和展示。
- `nexus-db` 的 `DocumentRecord`、`upsert_document`、`get_document` 和 `delete_document`。
- 按 ID upsert、读写校验、位置范围校验和不回显正文/路径的安全错误分类。
- M3.0 不创建 FTS5 虚拟表；FTS5 表、tokenizer 和一致性事务属于 M3.1。

### M3.1：SQLite FTS5 基础索引

**状态：** 已完成。

- 建立正文索引。
- 支持插入、更新和删除。
- 明确 tokenizer 选择。
- 保证元数据与 FTS 数据的一致性。
- 先不引入 Tantivy。
- 只有在基准测试证明 FTS5 无法满足延迟、规模或查询能力要求时，才评估 Tantivy，并通过 ADR 记录决定。

**本单元交付：**

- schema 4 的 `documents_fts` external-content FTS5 表，索引 `title` 和 `body`。
- 明确使用 `unicode61 remove_diacritics 1`，不新增数据库或搜索引擎依赖。
- SQLite triggers 维护 canonical 文档插入、更新和删除时的 FTS5 索引。
- migration 对 M3.0 已存在文档执行 FTS5 rebuild。
- 测试覆盖新增、更新、删除、旧文档重建、事务回滚和 FTS integrity-check。

### M3.2：查询语法和过滤器

**状态：** 已完成。

- `nexus-db::search_documents` 提供受限查询入口。
- 支持关键词、多个关键词 AND、双引号短语、文件名、路径、扩展名和文件类型过滤。
- 支持 modified、created、accessed 日期过滤，以及 `date` modified 别名。
- 查询值全部绑定，拒绝 raw FTS5 操作符；非法查询、日期、空值和冲突筛选返回安全错误。
- 结果按文档 ID 稳定排序，单次结果数量限制为 1–1000，不返回完整正文。
- 测试覆盖关键词、短语、过滤器、日期边界、仅过滤器查询、数量上限和错误分类。

### M3.3：ranking 和 snippet

**状态：** 已完成。

- 使用 SQLite FTS5 BM25 计算确定性 lexical relevance，标题权重高于正文。
- relevance 降序排序，文档 ID 作为稳定 tie-break；过滤器-only 查询继续按文档 ID排序。
- 使用 FTS5 `snippet()` 返回最多 32 tokens 的纯文本命中片段，使用 `⟦` / `⟧` 标记。
- `SearchResult` 返回可选 relevance 和 snippet，不返回完整正文。
- 测试覆盖标题/正文权重、tie-break、snippet 和过滤器-only 行为。

### M3.4：搜索 UI

**状态：** 已完成。

- 输入查询。
- 显示结果、路径、类型、时间和 snippet。
- 处理 loading、空结果、错误和取消。
- 点击结果可以通过平台系统打开器打开原始文件；命令校验文件存在性，失败返回安全错误。
- 默认进入搜索页，M1 文件索引页和重扫流程继续保留；UI 只通过 Tauri 命令访问数据库能力。
- 搜索结果 DTO 不返回完整正文；前端以纯文本节点渲染 snippet，不解释 HTML。
- 前端请求 ID 使取消后的迟到结果失效，但不引入数据库中断或持久连接架构。
- 测试覆盖默认搜索页、侧栏导航、查询结果、空结果、错误、取消和打开文件命令映射。
- 决策记录：`docs/decisions/0017-m3-search-ui.md`；验收记录：`docs/acceptance/0013-m3-search-ui.md`。

### M3.5：搜索质量评估

**状态：** 已完成。

- 增加 `crates/nexus-db/tests/search_quality.rs` 离线评估 harness，不读取真实用户资料，不增加生产 API 或依赖。
- 固定 10 条代表性文档和 9 个带人工相关性标注的查询；以 M3.2 文档 ID 顺序作为同候选集基线。
- 记录 Recall@3、Top-1 命中、每个查询 15 次测量的中位/p95 延迟、SQLite 文件大小和 FTS5 segment bytes。
- 本次结果：BM25 macro Recall@3 `0.9722`，基线 `0.9444`；测试要求当前值不低于基线。
- 决策记录：`docs/decisions/0018-m3-search-quality.md`；验收记录：`docs/acceptance/0014-m3-search-quality.md`。

### M3.6：M3 验收

**状态：** 已完成。

- 可以搜索正文，不只是文件名。
- 支持关键词、短语和基本过滤器。
- 结果带 snippet 和原文件引用。
- 查询性能和质量有记录。

**本单元实现：**

- `nexus-core::index_directory` / `index_directory_with_control` 复用 M1 流式扫描和取消
  边界，按文件扩展名分派 M2 已验收的单文件解析器，并将 `Document` 转为
  `nexus-db::DocumentRecord` 写入 canonical 表；SQLite FTS trigger 自动维护正文索引。
- `ParseOptions::default()` 固定初始索引的有界读取策略：普通输入/输出 16 MiB、PDF 解压
  流 64 MiB、DOCX 主文档条目 16 MiB、JSON 深度 32。此处不增加解析格式或依赖。
- 不支持格式计入 `documents_skipped`，单文件解析错误计入 `documents_failed` 并继续；文档
  持久化错误返回任务级错误。初始文档 ID 使用规范化路径的稳定 FNV-1a `file:<hex>` 表示，
  不在 ID 中暴露用户路径。
- `start_rescan` 增加默认关闭的 `indexContent` 请求字段；桌面文件索引页显式打开它，保持
  既有 core metadata-only `rescan_directory_with_control` 调用方兼容。进度和完成事件增加
  正文写入、失败和格式跳过统计。
- 取消时停止后续扫描；已提交的单文档正文与 FTS 保持原子一致，尚未完成的元数据重扫不
  进入最终应用事务。文件变化检测、陈旧正文清理和 watcher 不属于本单元，留给 M4。
- 验收记录见 `docs/acceptance/0015-m3-acceptance.md`，架构决策见
  `docs/decisions/0019-m3-initial-content-index.md`。

## 8. M4 — Incremental Indexing

### M4.0：变更检测

**所属大板块：** B5 Incremental Indexing

**目的：** 识别文件是否需要重新处理。

**成品要求：**

- 先使用路径、大小、mtime 等便宜信息。
- hash 只用于确实需要的情况。
- 修改中的文件不会被错误地当成稳定内容。

**本单元实现：**

- `nexus-core` 增加 `FileSnapshot` 和 `detect_file_changes`，比较上次与本次快照并输出
  新增、修改、未变化和消失四类结果。
- 当前只使用路径、大小和修改时间；两侧均有相同修改时间且大小相同才判定为未变化。
  修改时间缺失时保守地归入修改，避免把无法确认稳定性的文件跳过。
- 结果按路径排序；空路径和同一输入侧重复路径返回安全错误，不静默覆盖输入。
- 本单元不读取正文、不计算 hash、不写数据库、不引入 watcher；监听、事件归并和自动
  更新留给后续 M4.1–M4.4。
- 决策记录：`docs/decisions/0020-m4-change-detection.md`；验收记录：
  `docs/acceptance/0016-m4-change-detection.md`。

### M4.1：文件事件接入

- CREATE。
- UPDATE。
- DELETE。
- MOVE/rename。
- 事件来源与扫描逻辑解耦。

**本单元实现：**

- `nexus-core` 使用稳定版 `notify 8.2.0` 增加递归 `watch_directory`，底层平台通知不
  直接进入扫描或数据库逻辑。
- 统一输出 `FileEvent` 的创建、修改、删除、重命名和 `RescanRequired`；监听根目录之外
  的事件被过滤，移入/移出范围的移动分别转换为新增/删除。
- 支持底层一次提供两端路径的重命名，以及连续 From/To 通知的基础配对；事件丢失信号
  转换为完整重扫请求。
- `FileWatcher` 生命周期控制底层监听，调用方通过有界等待或非阻塞读取消费事件；监听
  回调不读取正文、不写数据库。
- 本单元不实现去重、防抖、文件稳定性等待、批处理或自动更新；这些内容留给 M4.2–M4.4。
- 决策记录：`docs/decisions/0021-m4-file-event-source.md`；验收记录：
  `docs/acceptance/0017-m4-file-event-source.md`。

### M4.2：去重、debounce 和批处理

- 合并短时间内重复事件。
- 不重复解析同一文件。
- 事件顺序异常时最终状态正确。
- 批处理失败可重试。

**本单元实现：**

- `nexus-core` 增加 `EventBatcher`，以 250ms 安静窗口和 128 路径上限合并事件；同一路径
  只保留最终操作，重命名拆成旧路径移除和新路径更新，完整重扫信号覆盖局部事件。
- 增量更新在解析前后重新确认文件元数据；文件仍在变化时有限等待并重试，解析失败保留
  既有正文，不把半截内容写入 canonical 文档。
- `nexus-db` 增加 `apply_file_mutations`，在一个事务中同时更新文件元数据、canonical
  文档及 FTS 一致性；事务失败时整个批次回滚并最多重试两次。
- 已覆盖重复事件、异常顺序、重命名、完整重扫、创建/更新/删除、重复文档行和损坏正文
  保留旧内容等边界。
- 决策记录：`docs/decisions/0022-m4-event-coalescing-and-batch-write.md`；验收记录：
  `docs/acceptance/0018-m4-event-coalescing-and-batch-write.md`。

### M4.3：后台任务和关闭

- watcher 不阻塞 UI。
- 任务可以取消。
- 应用关闭时停止接收新事件。
- 未完成批次不会破坏数据库一致性。

**本单元实现：**

- 桌面层增加单活动 `WatchManager`，在独立线程中持有 watcher、事件归并器和增量处理
  任务；UI 通过 `get_watch_status` 及安全事件接收状态，不直接参与事件等待或数据库写入。
- 监听任务复用 `RescanControl`，并使用关闭标记停止接收新事件；停止时等待线程退出，
  让未提交批次继续受 M4.2 的事务边界保护。
- 增量批次暂时无法提交时保留批次并延迟重试；单文件失败只计入统计，不上传路径、正文
  或底层错误文本。
- 初始正文索引成功后保存监听根目录；启动时对已保存目录先做一次完整恢复，再开始监听。
- 前端增加自动同步状态、最近一次增量统计和 payload 校验；浏览器预览仍可安全降级。
- 决策记录：`docs/decisions/0023-m4-background-watch-task.md`；验收记录：
  `docs/acceptance/0019-m4-background-watch-task.md`。

### M4.4：M4 验收

- 创建、修改、删除和移动文件后索引最终一致。
- 临时文件和编辑器原子保存有测试。
- 重复事件不会产生重复记录。
- 关闭和重启后可以恢复。

**本单元实现：**

- 通过真实临时目录验证创建、修改、删除和移动后，文件元数据、canonical 文档和 FTS
  最终保持一致。
- 通过临时扩展名写入后重命名到最终文件的测试覆盖编辑器原子保存；临时路径不会成为
  正文记录。
- 通过重复事件归并和记录数量断言确认同一文件不会产生重复 canonical 或 FTS 行；损坏
  正文和事务失败的旧状态保护作为回归场景保留。
- 通过重新打开数据库、重新创建事件归并器和监听目录配置读写测试，确认关闭后的变化
  可以在恢复流程中继续处理。
- 决策记录：`docs/decisions/0024-m4-acceptance.md`；验收记录：
  `docs/acceptance/0020-m4-acceptance.md`。

## 9. M5–M8 后续执行单元

M5.0–M5.4 已完成工程基线；M6–M8 仍需等待真实使用需求和新的产品决策。

### M5 — Semantic Search

**所属大板块：** B6 Semantic Search

**状态：本地工程基线完成，预训练模型未选定。**

本次执行结果：

- 固定了 provider 边界、模型 ID/版本、256 维输出和“标题 + 正文”的数据范围；不读取路径、
  文件名和时间字段，不访问网络。
- 建立了 schema 5 的模型登记、版本化向量存储、输入指纹和文档更新/删除清理机制；模型
  身份或维度冲突会安全失败，不静默覆盖。
- 保留 SQLite FTS5 的 BM25 lexical 检索，增加本地向量候选并使用固定常数 60 的 RRF 融合；
  metadata 过滤同时作用于两条路径，模型缺失、向量缺失或向量损坏时回退到 lexical。
- 全量索引和 M4 增量监听都按有界批次刷新向量；支持取消，已提交批次保持有效，单个文档
  解析/向量失败不拖垮其他文档。
- 复用 M3 固定评估集：BM25 与 hybrid 的 Recall@3 均为 `0.9722`，Top-1 均为 `9/9`；
  当前 hybrid 有额外计算开销，因此没有加入 reranking。

当前基线不是预训练语言模型，不应把它描述为已经具备通用同义理解。下一阶段如要采用
真正的本地模型，必须先决定模型文件来源、许可、体积、运行时、设备资源、升级策略和真实
脱敏语料评估方式。

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
- 过早引入 Tantivy、预训练模型或 Agent。
- 未来结构被空 crate、空接口和通用抽象占满。
- 只验证“能运行”，没有验证索引一致性和失败恢复。

### 应暂停并重新评估的情况

- 不能解释当前模块职责和数据流。
- 测试只是为了让 CI 变绿，没有覆盖实际风险。
- 为解决一个局部问题引入大型框架。
- M5 质量没有改善，却继续扩展到 M6。
- 产品没有真实使用者或真实语料。
- Codex 连续修改多个边界模块而没有中间 review。

## 15. 当前下一步

M0.0–M0.7、M1.0、M1.1、M1.2、M1.3、M1.4、M1.5、M2.0、M2.1、M2.2、M2.3a、M2.3b、
M2.3c、M2.4、M3.0、M3.1、M3.2、M3.3、M3.4、M3.5、M3.6、M4.0、M4.1、M4.2、M4.3、M4.4
以及 M5.0–M5.4 的本地向量混合检索基线已完成。预训练模型选择、M6 问答、云端同步、多用户、
认证和 Agent 系统尚未开始。
