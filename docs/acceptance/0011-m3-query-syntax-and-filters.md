# M3.2 查询语法和基本过滤器验收记录

## 目标

在 M3.1 FTS5 索引之上提供受限、确定性的关键词/短语查询和基本元数据过滤，并对
非法查询返回可理解且不泄露敏感内容的错误。

## 本单元范围

- 关键词和多个关键词的 AND 查询。
- 双引号短语查询。
- 文件名、来源路径、扩展名和文件类型过滤。
- modified、created、accessed 日期过滤，以及 `date` 到 modified 的别名。
- 稳定排序和有界结果数量。
- 仅过滤器查询。
- 非法字段、空值、未闭合引号、非法日期和冲突过滤的安全错误。

明确不包含：raw FTS5 操作符、BM25/ranking、snippet、分页、搜索 UI、批量解析、文件
watcher、增量索引和语义搜索。

## 验收证据

| 检查项 | 证据 |
| --- | --- |
| 关键词和短语 | `searches_text_phrases_metadata_filters_and_date_ranges` |
| 文件名、路径、扩展名和类型 | 同一测试覆盖四类元数据过滤 |
| 日期范围和整日边界 | 同一测试；`parses_date_boundaries_and_leap_days` 覆盖日期换算 |
| 仅过滤器查询和结果上限 | `supports_filter_only_queries_and_bounds_result_count` |
| 非法查询和冲突筛选 | `rejects_invalid_query_syntax_and_conflicting_filters` |
| 不暴露原始 FTS 语法 | `parses_keywords_phrases_and_filters_without_exposing_raw_fts_syntax` |
| 稳定排序 | 关键词搜索断言按文档 ID 返回 |
| 安全错误和数量边界 | `SearchError` 分类、用户说明以及 1–1000 上限 |

## 结果

M3.2 已实现 `nexus-db::search_documents`、受限查询解析器、基本过滤器和安全错误。
正文搜索和过滤查询均保持在数据库边界内；结果带有原始文件追溯信息但不携带完整正文。
下一步为 M3.3 ranking 和 snippet。

## 实际执行的检查

- `pnpm format`：通过。
- `pnpm lint`：通过，包含全 workspace Clippy `-D warnings`。
- `pnpm typecheck`：通过，包含前端 TypeScript 和 Rust workspace 检查。
- `pnpm test`：通过，前端 6 个测试、Rust 64 个测试全部通过。
- `cargo test --workspace --locked`：通过，Rust 36 + 25 + 3 个测试全部通过。
- `pnpm build`：通过，前端和 Rust workspace 均成功构建。
- `git diff --check`：通过。
