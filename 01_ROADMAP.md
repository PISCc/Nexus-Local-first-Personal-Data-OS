# Nexus Roadmap

## M0 --- Engineering Foundation

目标：建立可长期维护的工程骨架。

范围： - Tauri Desktop - React + TypeScript UI - Rust workspace -
SQLite - logging / error handling - test / lint / format - CI

验收： - 开发模式可以启动桌面应用。 - Rust/前端测试可以执行。 - 项目可以
build。 - CI 自动执行基础检查。

## M1 --- Local File Scanner

目标：用户选择目录，Nexus 建立文件元数据数据库。

记录： - path - filename - extension - size - timestamps - MIME/type -
hash（按需要）

必须考虑： - 10 万+文件 - permission denied - 文件扫描期间删除/修改 -
symlink - ignored paths - cancellation - progress - 大文件 - 重复文件

验收：10 万级文件扫描可完成，UI 不被阻塞，单文件错误不终止整个扫描。

## M2 --- Content Parsing

先支持： - txt/md - py/rs/js/ts/java/cpp - json

再支持： - PDF - DOCX - HTML

统一为 Document 模型： `Source -> Parser -> Document`

## M3 --- Full-text Search

目标：真正搜索文件正文，而非仅文件名。

支持： - keyword / phrase - filename/path - extension - date filters -
ranking - snippet

学习重点： - inverted index - tokenizer - BM25 - query parser

## M4 --- Incremental Indexing

加入文件监听： CREATE / UPDATE / DELETE / MOVE。

重点处理： - duplicate events - debounce - race conditions - atomic
saves - temporary files - cancellation / shutdown

## M5 --- Semantic Search

加入 Embedding，但保留全文搜索。

目标架构： `BM25 + Vector Search -> Fusion -> Rerank`

重点研究 Hybrid Search，而不是简单 Vector DB Demo。

## M6 --- Ask Nexus

自然语言问题：
`Question -> Query Analysis -> Retrieval -> Rerank -> Context -> LLM -> Answer`

硬性要求：回答必须可追溯到原始资料，尽可能提供文件/页码/片段引用。

## M7 --- Personal Timeline

建立个人资料活动时间线，并支持按时间检索与总结。

## M8 --- Agent Layer

Agent 可以调用： - search_documents - read_document - find_related -
search_code - get_timeline

示例任务： "整理最近半年所有 Agent Memory 资料，分类比较并生成报告。"

## 范围控制

当前 milestone 未完成前，不主动实现后续 milestone。 尤其 M0--M4
阶段不要因为"AI 很有趣"提前加入 Agent。
