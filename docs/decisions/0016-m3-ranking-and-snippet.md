# ADR-0016：M3.3 确定性 ranking 和匹配片段

- 状态：Accepted
- 日期：2026-09-01
- 范围：M3.3 ranking 和 snippet

## 背景

M3.2 已经提供关键词、短语和基本过滤查询，但结果仍只按文档 ID排序，也没有告诉调用方
命中的正文上下文。M3.3 需要增加可解释、可重复的 lexical ranking 和有限长度的匹配片段，
同时保持本地数据库边界，不引入 LLM 或新的搜索引擎。

## 决策

### 1. 使用 SQLite FTS5 `bm25` 做第一版 lexical ranking

正文查询使用：

```sql
-bm25(documents_fts, 1.0, 5.0, 1.0)
```

`document_id` 是未索引列，标题权重为 5，正文权重为 1；取负值后得到“数值越大越相关”
的 `relevance`。标题权重是当前简单、可解释的启发式，不声称已经完成搜索质量优化。

排序规则为 `relevance DESC, document_id COLLATE BINARY ASC`。这样相关性相同的结果也有
稳定的 deterministic 顺序。仅过滤器查询没有正文匹配，不产生 relevance，按文档 ID排序。

### 2. 使用 FTS5 `snippet()` 返回纯文本片段

同时计算标题和正文片段，优先返回标题片段，否则返回正文片段。片段最多 32 个 tokenizer
tokens，使用 `⟦` 和 `⟧` 标记命中范围，使用 `…` 表示省略。结果只带片段，不把完整
`documents.body` 复制到 `SearchResult`，也不生成 HTML 或要求 UI 使用不安全的 HTML 注入。

`SearchResult` 增加可选 `relevance: Option<f64>` 和 `snippet: Option<String>`；没有正文
查询时两者均为 `None`。

### 3. 保持现有查询和数据库边界

M3.3 继续复用 `nexus-db::search_documents`、M3.1 的 FTS5 表和 M3.2 的受限查询解析器。
关键词仍转换为安全的 FTS5 phrase，查询值继续使用绑定参数，不新增依赖，不创建 UI 或
独立 search crate。搜索仍不依赖 LLM。

## 未采用的方案

- **只按命中次数排序**：没有利用文档长度和字段权重，标题命中与正文命中难以区分。
- **把 BM25 分数暴露为唯一产品质量指标**：当前分数只用于确定性排序，权重需要真实语料
  和 M3.5 评估后再调整。
- **自己扫描正文生成 snippet**：会绕过 FTS5 的命中定位并额外读取完整正文。
- **返回 HTML `<mark>`**：会把渲染和转义责任推给 UI，增加用户内容被当作 HTML 的风险。
- **引入 LLM 或语义 reranker**：不属于 lexical M3，也会破坏核心功能的本地确定性。

## 风险和后续影响

- 标题权重 5、正文权重 1 是启发式，尚未经过真实语料相关性评估。
- `unicode61` 的中文分词限制仍然存在，snippet 质量取决于当前 tokenizer。
- 当前只返回一个有限片段，不支持多片段、分页或 UI 渲染；这些留给后续单元。
- relevance 使用浮点数，但 tie-break 明确使用文档 ID，避免相同分数下结果顺序漂移。
