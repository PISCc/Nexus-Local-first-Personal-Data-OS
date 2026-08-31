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
