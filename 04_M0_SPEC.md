# M000 --- Engineering Foundation

## Goal

建立 Nexus 的最小、可靠、可持续开发的工程骨架。

本 milestone 不实现正式搜索功能，不实现 AI。

## Required Outcomes

1.  Tauri desktop application 可以启动。
2.  React + TypeScript 前端可工作。
3.  Rust workspace/核心 crate 结构明确。
4.  SQLite 能完成最小连接/初始化验证。
5.  logging 和基础 error handling 可用。
6.  Rust 与前端测试框架可运行。
7.  format/lint/typecheck/build 有明确命令。
8.  GitHub Actions 执行基础 CI。
9.  README 记录本地开发命令。
10. ARCHITECTURE.md 与实际骨架保持一致。

## Out of Scope

-   文件扫描业务
-   PDF parsing
-   FTS
-   Embedding
-   LLM
-   Agent
-   Cloud
-   Login
-   Sync
-   Plugin system

## Acceptance Criteria

新开发者 clone 后，可以根据 README： 1. 安装依赖。 2. 启动 desktop app。
3. 运行全部基础测试。 4. 运行 lint/format/typecheck。 5. 构建项目。

CI 对同样的核心检查提供自动验证。

## First Codex Task

将下面内容作为第一个任务发送给 Codex：

``` text
Read AGENTS.md, ARCHITECTURE.md, ROADMAP.md, and this M000 specification.

Do not write code yet.

Inspect the repository and propose the smallest maintainable M0 architecture.

Return:
1. recommended repository structure
2. exact technologies/dependencies required for M0
3. files that should be created
4. development commands
5. testing strategy
6. CI strategy
7. major decisions that require human approval
8. risks or unnecessary complexity you recommend avoiding

Important:
- Do not implement M1 features.
- Do not add AI/LLM functionality.
- Avoid premature abstractions.
- Prefer the smallest architecture that can cleanly grow into M1–M4.
```

先审阅它的方案，再授权实现。
