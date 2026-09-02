# M3.3 确定性 ranking 和匹配片段验收记录

## 目标

在 M3.2 查询结果上增加不依赖 LLM 的确定性 lexical ranking 和有限长度匹配片段，确保
结果排序、tie-break 和无正文过滤查询行为可重复。

## 本单元范围

- SQLite FTS5 `bm25` relevance。
- 标题相对正文的字段权重。
- 相关性降序和文档 ID 二级排序。
- 标题/正文 `snippet()`，纯文本命中标记和长度边界。
- 仅过滤器查询不产生 relevance/snippet。

明确不包含：搜索 UI、HTML 渲染、分页、多片段 snippet、中文分词优化、语义搜索、LLM
reranking 和完整 M3.5 质量评估。

## 验收证据

| 检查项 | 证据 |
| --- | --- |
| 标题命中优先于正文命中 | `ranks_title_matches_before_body_matches_and_uses_id_tiebreak` |
| relevance 存在且排序稳定 | 同一测试断言分数顺序和文档 ID tie-break |
| snippet 返回命中标记 | `searches_text_phrases_metadata_filters_and_date_ranges` |
| snippet 不返回完整正文 | `SearchResult` 仅包含片段字段 |
| 过滤器-only 无 ranking/snippet | `supports_filter_only_queries_and_bounds_result_count` |
| 无 LLM 或新搜索依赖 | `nexus-db` 仍只依赖现有 `rusqlite` |

## 结果

M3.3 已实现 FTS5 BM25 相关性、稳定排序和纯文本匹配片段。标题权重、片段长度和中文
召回质量仍需在 M3.5 使用固定语料评估；下一步为 M3.4 搜索 UI。

## 实际执行的检查

- `pnpm format`：通过。
- `pnpm lint`：通过，包含 Clippy `-D warnings`。
- `pnpm typecheck`：通过。
- `pnpm test`：通过，前端 6 个、Rust 65 个测试。
- `cargo test --workspace --locked`：通过，Rust 36 + 26 + 3 个测试。
- `pnpm build`：通过，前端生产构建和 Rust 工作区构建均完成。
- `git diff --check`：通过。
