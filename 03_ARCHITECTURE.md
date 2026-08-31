# Nexus Architecture

## 推荐技术栈

-   Desktop: Tauri
-   Frontend: React + TypeScript
-   Core: Rust
-   Local DB: SQLite
-   Initial full-text search: SQLite FTS5
-   Later search engine option: Tantivy
-   File watcher: Rust `notify`
-   AI layer: 后期按需求使用独立 Python 服务或 API
-   Testing: Rust tests + Vitest + Playwright
-   CI: GitHub Actions

以上是初始方案，不是不可更改的教条。重大调整需要 ADR。

## 推荐仓库结构

``` text
nexus/
├── AGENTS.md
├── README.md
├── ARCHITECTURE.md
├── ROADMAP.md
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
