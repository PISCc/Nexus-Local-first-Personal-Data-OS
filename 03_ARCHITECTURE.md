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
- 当前 schema 版本为 `1`，迁移文件为
  `crates/nexus-db/migrations/0001_foundation.sql`。
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
