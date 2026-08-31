# Nexus 路线图

## M0 —— 工程基础

目标：建立可长期维护的工程骨架。

范围：- Tauri 桌面应用 - React + TypeScript 界面 - Rust 工作区 - SQLite -
日志与错误处理 - 测试 / lint / format - CI

验收：- 开发模式可以启动桌面应用。- Rust/前端测试可以执行。- 项目可以构建。
- CI 自动执行基础检查。

## M1 —— 本地文件扫描

目标：用户选择目录，Nexus 建立文件元数据数据库。

记录：- 路径 - 文件名 - 扩展名 - 大小 - 时间戳 - MIME/类型 - 哈希（按需要）

必须考虑：- 10 万+文件 - 权限拒绝 - 文件扫描期间删除/修改 - 符号链接 -
忽略路径 - 取消 - 进度 - 大文件 - 重复文件

验收：10 万级文件扫描可完成，UI 不被阻塞，单文件错误不终止整个扫描。

## M2 —— 内容解析

先支持：- txt/md - py/rs/js/ts/java/cpp - json

再支持：- PDF - DOCX - HTML

统一为 Document 模型：`Source → Parser → Document`

## M3 —— 全文搜索

目标：真正搜索文件正文，而非仅文件名。

支持：- 关键词 / 短语 - 文件名/路径 - 扩展名 - 日期过滤 - 排序 - 匹配片段

学习重点：- 倒排索引 - 分词器 - BM25 - 查询解析器

## M4 —— 增量索引

加入文件监听：创建 / 更新 / 删除 / 移动。

重点处理：- 重复事件 - 防抖 - 竞态条件 - 原子保存 - 临时文件 - 取消 / 关闭

## M5 —— 语义搜索

加入 Embedding，但保留全文搜索。

目标架构： `BM25 + Vector Search -> Fusion -> Rerank`

重点研究混合搜索，而不是简单的向量数据库演示。

## M6 —— Ask Nexus

自然语言问题：
`Question -> Query Analysis -> Retrieval -> Rerank -> Context -> LLM -> Answer`

硬性要求：回答必须可追溯到原始资料，尽可能提供文件/页码/片段引用。

## M7 —— 个人时间线

建立个人资料活动时间线，并支持按时间检索与总结。

## M8 —— Agent 层

Agent 可以调用：- search_documents - read_document - find_related - search_code -
get_timeline

示例任务： "整理最近半年所有 Agent Memory 资料，分类比较并生成报告。"

## 范围控制

当前里程碑未完成前，不主动实现后续里程碑。尤其 M0–M4 阶段不要因为“AI 很有趣”
提前加入 Agent。
