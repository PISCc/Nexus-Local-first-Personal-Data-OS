# M0 —— 工程基础

## 目标

建立 Nexus 的最小、可靠、可持续开发的工程骨架。

本 milestone 不实现正式搜索功能，不实现 AI。

## 必须交付

1.  Tauri desktop application 可以启动。
2.  React + TypeScript 前端可工作。
3.  Rust workspace/核心 crate 结构明确。
4.  SQLite 能完成最小连接/初始化验证。
5.  logging 和基础 error handling 可用。
6.  Rust 与前端测试框架可运行。
7.  format/lint/typecheck/build 有明确命令。
8.  GitHub Actions 执行基础 CI。
9.  README 记录本地开发命令。
10. 03_ARCHITECTURE.md 与实际骨架保持一致。

## 范围外

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

## 验收标准

新开发者 clone 后，可以根据 README： 1. 安装依赖。 2. 启动桌面应用。
3. 运行全部基础测试。 4. 运行 lint/format/typecheck。 5. 构建项目。

CI 对同样的核心检查提供自动验证。

## 第一个 Codex 任务

将下面内容作为第一个任务发送给 Codex：

``` text
阅读 AGENTS.md、03_ARCHITECTURE.md、01_ROADMAP.md 和本 M0 规格。

现在不要写代码。

检查仓库，并提出最小且可维护的 M0 架构。

请返回：
1. 推荐的仓库结构
2. M0 所需的确切技术和依赖
3. 应创建的文件
4. 开发命令
5. 测试策略
6. CI 策略
7. 需要人工确认的重大决定
8. 建议避免的风险或不必要复杂度

重要要求：
- 不要实现 M1 功能。
- 不要加入 AI/LLM 功能。
- 避免过早抽象。
- 优先选择能够平稳扩展到 M1–M4 的最小架构。
```

先审阅它的方案，再授权实现。
