# Nexus --- Project Brief

## 项目定位

Nexus 是一个 **Local-first Personal Data OS（本地优先个人信息中枢）**。

目标不是做一个简单的 AI/RAG
Demo，而是做一个可以长期迭代的中大型软件系统：持续索引用户本地文件、代码、论文、网页等个人信息，提供可靠的全文搜索、语义搜索、AI
问答，最终支持 Agent 在个人数据上执行复杂任务。

## 为什么做这个项目

核心目标是训练"一个人 + Codex 做团队级工程"的能力。

人负责： - 产品方向 - 架构和技术决策 - 需求拆分 - Code Review - 验收 -
理解整个系统

Codex 负责： - 仓库调查 - 实现 - 测试 - Debug - 重构 - 文档 - Review
辅助

原则：**Codex 放大产能，不替代工程判断。**

## 核心产品体验

用户可以： 1. 选择本地目录并建立索引。 2.
按文件名、路径、类型、时间和正文快速搜索。 3.
后续通过自然语言进行语义搜索。 4.
对自己的资料提问，并获得可追溯到原始文件的答案。 5.
查看个人信息/文件活动时间线。 6. 最终让 Agent
调用搜索、读取、关联等工具完成资料整理与研究任务。

## 长期架构

``` text
Data Sources
    |
Scanner / Connectors
    |
Parsers
    |
Normalized Document Model
    |
    +------------------+
    |                  |
Metadata Store     Search Index
    |                  |
    +--------+---------+
             |
        Query Layer
             |
             UI
             |
       AI / Agent Layer
```

## 第一原则

-   Local-first。
-   核心搜索不依赖 LLM。
-   可靠性 \> 功能数量。
-   搜索质量 \> 炫酷 UI。
-   先做确定性全文搜索，再做 Embedding/LLM。
-   不提前引入云同步、多用户、微服务等复杂度。
-   项目必须可测试、可观测、可维护。
