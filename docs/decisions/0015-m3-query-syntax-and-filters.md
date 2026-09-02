# ADR-0015：M3.2 查询语法和基本过滤器

- 状态：Accepted
- 日期：2026-09-01
- 范围：M3.2 查询语法和基本过滤器

## 背景

M3.1 已经建立 `documents_fts` external-content FTS5 索引，但尚未提供公共查询入口。
M3.2 需要让本地调用方能够执行确定性的关键词、短语和基本元数据过滤，同时保持
canonical 文档、SQLite 访问和安全错误都在数据库边界内。

## 决策

### 1. 查询入口继续放在 `nexus-db`

提供：

```rust
search_documents(connection, query, limit) -> Result<Vec<SearchResult>, SearchError>
```

当前不创建独立的 `nexus-search` crate，也不增加数据库或查询引擎依赖。M3.2 的查询
仍然是数据库边界之上的薄封装；等出现 ranking、snippet 或跨存储检索的真实边界后
再评估独立 search crate。

### 2. 使用受限查询语法，调用方不直接传入原始 FTS5 表达式

支持的语法为：

- 裸关键词：`alpha`；多个关键词按 AND 组合。
- 双引号短语：`"alpha beta"`。
- `filename:value` 和 `path:value`：不区分 ASCII 大小写的包含匹配。
- `ext:value` / `extension:value`：扩展名精确匹配，去除开头点号。
- `type:value`：文件类型精确匹配。
- `modified`、`created`、`accessed` 或 `date`：支持 `field:YYYY-MM-DD`、
  `field>=YYYY-MM-DD` 和 `field<=YYYY-MM-DD`；`date` 是 `modified` 的别名。

解析器把关键词和短语转为安全的 FTS5 phrase，并拒绝未支持的操作符、字段、空值、
非法日期和冲突筛选。筛选值和 SQL 参数全部使用绑定参数，不拼接用户输入。

### 3. 日期按 UTC 日历日处理

文件元数据继续使用 Unix epoch 毫秒。无比较符的日期表示整天；`<=YYYY-MM-DD` 的
上界转换为次日零点的 exclusive bound，因此包含指定日期全天。当前不引入日期依赖。

### 4. 搜索结果只返回可追溯元数据

`SearchResult` 返回文档 ID、来源路径、标题、可选行范围和当前可用的文件元数据，
不返回完整正文。结果按 `document_id` 的 binary 顺序稳定排序，单次最多返回 1000 条；
默认建议调用方使用 100 条上限。M3.3 再决定 ranking 和 snippet。

正文关键词使用独立的 `rowid IN (SELECT ... MATCH ...)` 子查询；这样避免 SQLite
FTS5 在可选条件 `OR` 上下文中的 MATCH planner 限制。只有过滤器的查询不执行 FTS
匹配，仍可按来源路径和文件元数据筛选。

## 未采用的方案

- **直接把输入传给 `documents_fts MATCH`**：会暴露 FTS5 操作符语义，错误信息和兼容性
  边界也会泄露到上层；当前查询语言只需要关键词和短语。
- **用 `LIKE` 扫描正文**：无法复用 M3.1 倒排索引，规模增长后成本不可接受。
- **新增 `nexus-search` crate**：当前没有跨存储、ranking 或 snippet 的独立职责，暂不
  提前拆分边界。
- **引入 chrono 等日期依赖**：M3.2 只需严格的 ISO 日期和 epoch 毫秒换算，标准库实现
  足够，避免扩大依赖面。

## 风险和后续影响

- `extension`、`type` 和日期过滤依赖 `file_metadata`；没有对应元数据的文档仍可做正文
  或路径查询，但不会命中过滤器。
- `unicode61` 不是中文分词方案，中文召回质量留给真实语料评估。
- 当前没有分页、ranking、snippet、搜索 UI 或增量索引；这些属于后续单元。
- 搜索结果保留本地路径引用，但错误分类不回显查询内容、完整路径、正文或原始 SQLite
  错误。
