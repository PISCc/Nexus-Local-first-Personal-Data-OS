# Nexus 架构

## 推荐技术栈

-   桌面：Tauri
-   前端：React + TypeScript
-   核心：Rust
-   本地数据库：SQLite
-   初始全文搜索：SQLite FTS5
-   后续搜索引擎候选：Tantivy
-   文件监听：Rust `notify`
-   AI 层：后期按需求使用独立 Python 服务或 API
-   测试：Rust tests + Vitest + Playwright
-   CI：GitHub Actions

以上是初始方案，不是不可更改的教条。重大调整需要 ADR。

## 推荐仓库结构

``` text
nexus/
├── AGENTS.md
├── README.md
├── 00_PROJECT_BRIEF.md
├── 01_ROADMAP.md
├── 02_CODEX_WORKFLOW.md
├── 03_ARCHITECTURE.md
├── 04_M0_SPEC.md
├── 05_EXECUTION_PLAN.md
├── docs/
│   ├── decisions/
│   ├── specs/
│   └── research/
├── apps/
│   └── desktop/
├── crates/
│   ├── nexus-core/
│   ├── nexus-db/
│   ├── nexus-index/
│   ├── nexus-parser/
│   └── nexus-search/
├── packages/
│   └── ui/
├── tests/
│   ├── fixtures/
│   └── integration/
├── scripts/
└── .github/workflows/
```

## M0 实际 Rust 骨架

M0.3 只创建当前需要的三个 Rust crate，不提前创建 parser、index 或
search crate：

``` text
apps/desktop/src-tauri/  # Tauri desktop 壳层
crates/nexus-core/       # 与 UI 和平台无关的核心边界
crates/nexus-db/         # 本地数据库边界
```

依赖方向固定为：

``` text
nexus-desktop → nexus-core → nexus-db
```

`nexus-core` 和 `nexus-db` 不依赖 Tauri。M0.3 不定义数据库连接或业务
模型；SQLite 初始化和迁移由 M0.4 单独实现。

## M0.4 数据库边界

`crates/nexus-db` 对外只提供按调用方传入路径初始化数据库的入口：

- `initialize_database(path)` 负责打开数据库、读取 `PRAGMA user_version`
  并执行待处理迁移。
- 当前 schema 版本为 `2`；迁移文件依次为
  `crates/nexus-db/migrations/0001_foundation.sql` 和
  `crates/nexus-db/migrations/0002_file_metadata.sql`。
- 首个迁移只创建 `nexus_metadata(key, value)`，不包含文件索引或正文。
- 新数据库的迁移和版本更新在同一个事务中完成；未知版本返回明确错误。
- 数据库 crate 不假设 Tauri app data directory，生产路径由上层调用方决定。

## M0.5 启动与错误边界

- Tauri 桌面壳层负责初始化 stderr 日志、解析应用数据目录、准备目录，并调用
  `nexus-core::initialize`。
- `nexus-core` 负责组织数据库初始化并返回 `CoreError`；核心层不依赖 Tauri
  或前端。
- 数据库错误提供不含路径、内容或原始 SQLite 信息的 `kind` 分类和用户说明。
  原始错误仍保留在进程内用于错误链，但不会直接进入日志或 UI。
- 启动失败不会被静默忽略。桌面壳层将失败记录为安全分类，并把
  `ready` 或 `degraded` 状态交给 `get_startup_status` 命令。
- 前端先显示 `loading`，收到 command 结果后显示 `ready` 或 `degraded`；浏览器
  预览无法连接 Tauri 核心时也显示可理解的降级状态。
- M0.5 不写持久化日志，不上传数据，也不持有后续索引服务的数据库运行时连接。

## M1.0 文件元数据模型

- `nexus-db` 提供 `FileMetadata`、路径规范化、单条 upsert 和按路径读取入口。
- `file_metadata` 保存路径、文件名、扩展名、大小、可选的三个 Unix epoch 毫秒时间戳和可选类型标签。
- 路径只做绝对化，不执行 `canonicalize`，不跟随符号链接，也不进行大小写折叠。
- 数据库使用平台相关的 `path_key` 保存路径身份，同时用 `path_display` 提供展示文本，避免非 UTF-8
  路径导致崩溃。
- M1.0 不扫描目录、不读取文件内容、不计算哈希，也不创建正文或全文搜索表。

## M1.1 流式递归遍历

- `nexus-core` 提供实现 `Iterator` 的 `FileScanner`，逐项产出文件、跳过或失败结果。
- 默认不跟随符号链接；明确开启后使用解析后的目录身份去重，避免目录环路。
- 忽略路径按规范化路径及其子路径匹配；单项错误不会终止剩余扫描。
- 扫描器只读取文件系统元数据，不读取正文、不写数据库、不依赖 Tauri。
- 扫描统计、取消、后台任务和界面进度属于后续 M1 单元。

## M1.2 文件元数据批量持久化

- `nexus-db` 提供 `upsert_file_metadata_batch`，从结果迭代器逐批接收文件元数据，不要求调用方先收集完整路径列表。
- 每个批次使用独立 SQLite 事务；批次提交成功后才计入 `written` 和 `batches`，因此单个已提交批次不会被后续批次的错误回滚。
- 输入中的单条错误和元数据校验失败计入 `failed` 后继续；数据库连接、事务或 SQL 错误仍返回调用方，因为这表示持久化边界本身不可用。
- `FileMetadataBatchSummary` 提供 `received`、`written`、`failed` 和 `batches` 四项确定性统计。
- M1.2 不负责目录重扫、旧记录删除、取消、后台任务或 UI；扫描结果与手动重扫闭环在 M1.3 组合。

## M1.3 手动重扫闭环

- `nexus-core` 提供 `rescan_directory`，接收调用方传入的数据库路径、扫描根目录、扫描选项和批次大小。
- 重扫使用 `nexus-db` 的 `FileMetadataRescan` 会话：成功文件进入有界批次，失败和跳过路径进入保护集合，扫描完成后再清理确认不存在的旧记录。
- 会话使用 SQLite 连接级临时表保存本次扫描见到的路径和保护路径，不新增持久化 schema 字段，也不要求调用方收集完整路径列表。
- 只有位于选定根目录、未被本次扫描见到、且不在失败/跳过路径及其子树中的记录会被删除；相邻但不属于根目录的路径不会被误删。
- 返回 `RescanSummary`，明确提供成功文件数、失败数、跳过路径数、移除记录数和已提交批次数。
- 成功元数据在 `finish` 前暂存于连接级 `nexus_scan_pending`，取消或丢弃会话时不会应用到持久化表；最终应用和旧记录清理在一个事务中完成。

## M1.4 可取消重扫、进度事件和桌面界面

- `nexus-core` 提供 `RescanControl` 和 `rescan_directory_with_control`；任务在每个扫描结果前及最终提交前检查原子取消标记，并通过 `RescanProgress` 回调报告处理量、成功、失败和跳过统计。
- Tauri 壳层使用单活动任务槽位和后台线程；同一时间只允许一个重扫，重复启动返回 `rescan_already_running`，取消命令必须携带匹配的 `scanId`。
- 桌面层通过 `start_rescan`、`cancel_rescan`、`get_rescan_status` 三个 command，以及 `rescan-progress`、`rescan-finished` 两个事件与前端通信。事件字段使用 `camelCase`，错误只暴露固定的非敏感分类和中文说明。
- React 前端保留 M0 的暖纸张、苔绿色和锐利边框视觉系统，增加全中文文件索引工作区；用户输入完整目录路径后可启动、观察和取消重扫。
- 前端运行时校验 command 和事件 payload，并忽略不属于当前任务的事件；浏览器预览没有 Tauri 核心时保持诚实的降级提示。
- M1.4 不引入原生目录选择器、异步运行时、实时 watcher 或正文解析；M1.5 已按完整路径输入完成验收，原生选择器留作后续体验改进。

## M1 验收结果

- M1.0–M1.4 的代码、测试和桌面开发构建已完成；M1.5 验收记录见 `docs/acceptance/0001-m1-file-scanner.md`。
- 验收确认新增、更新、移除、失败保护、忽略路径、取消一致性、重复任务拦截和 100,000 条合成元数据批处理均有证据。
- M1 的数据库只保存文件元数据，不保存正文；M2.0 已定义最小 `Document` 模型，下一阶段进入 M2.1 的纯文本、Markdown 和代码解析。

## 领域边界

-   UI 不直接承担数据库/索引业务逻辑。
-   Parser 不依赖 UI。
-   Search 面向统一 Document，而不是面向特定扩展名。
-   平台相关行为尽量隔离。
-   Core 层保持可测试。

## Document 思路

长期核心抽象不是 File，而是 Document。

``` text
Source -> Parser -> Document
```

Document 最终可能来自： - Local File - PDF - Code - Webpage - Note -
Email - Repository

M0/M1 不要为了未来数据源提前建立复杂插件系统。

## M2.0 最小 Document 模型

M2.0 只定义统一内容结果的领域边界，不实现具体格式解析。当前模型位于
`crates/nexus-core/src/document.rs`，包含以下字段和约束：

- `DocumentId` 是非空不透明字符串；模型不在本阶段自行生成 UUID、哈希或其他持久化 ID。
- `DocumentSource::LocalFile` 保存本地文件来源；路径要求非空、转为绝对路径，并复用数据库层的跨平台路径规范化规则，但不做 `canonicalize`，避免把解析模型绑定到文件当前是否仍然存在。
- `title` 要求非空；`body` 可以为空，以允许后续解析器表达空文件或只有元数据的结果。
- `DocumentLocation` 使用可选的 1-based 起止行号；整篇文档使用无范围位置表示。
- 校验失败返回可分类的安全错误，不回显正文、完整路径或其他用户内容。

M2.1 才会在不改变上述模型边界的前提下增加纯文本、Markdown 和代码解析；解析器仍保持在 Core/领域层之外，不依赖 UI。

## 数据流（M1--M4）

``` text
Filesystem
   |
Scanner
   |
File Metadata
   |
Parser
   |
Document
   |
SQLite + FTS
   |
Query Layer
   |
Desktop UI
```

## 后期数据流（M5+）

``` text
                    Query
                      |
            +---------+---------+
            |                   |
         Lexical             Semantic
            |                   |
            +---------+---------+
                      |
                    Fusion
                      |
                   Rerank
                      |
                    Result
                      |
                  AI / UI
```

## 性能假设

长期按以下数量级思考，但不要过早优化： - 100,000+ files - 1,000,000+
indexed records - multi-GB datasets

大操作优先 streaming/batching，避免无理由一次性载入全部数据。
