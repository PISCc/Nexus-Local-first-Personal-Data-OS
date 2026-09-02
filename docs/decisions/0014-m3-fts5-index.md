# ADR-0014：M3.1 SQLite FTS5 基础索引

- 状态：Accepted
- 日期：2026-09-01
- 范围：M3.1 SQLite FTS5 基础索引

## 背景

M3.0 已建立 `documents` canonical 表，正文只保存一份。M3.1 需要把标题和正文变成可
检索的本地倒排索引，并保证文档插入、更新、删除和 schema 升级不会产生静默的索引漂移。
查询语法、ranking、snippet 和 UI 仍属于后续单元。

## 决策

### 1. 使用 external-content FTS5 表

schema 4 新增：

```sql
documents_fts(document_id UNINDEXED, title, body)
```

该表使用 `documents` 作为 external content，`rowid` 作为关联键。正文仍以
`documents.body` 为唯一事实来源，FTS5 只保存索引数据和未索引的文档 ID 映射，不复制
canonical 正文作为另一份应用数据源。

### 2. 使用 SQLite triggers 维护一致性

在 `documents` 上创建 insert、update、delete 三个 trigger：

- insert 写入新 rowid、文档 ID、标题和正文。
- update 先发出 FTS5 `delete` 命令，再写入新值。
- delete 发出 FTS5 `delete` 命令。

这样既有 `upsert_document` 和 `delete_document` API，也能在同一个 SQLite statement/
transaction 中完成 canonical 表和 FTS5 的变化，不需要在 UI 或调用方维护第二套索引逻辑。

schema 迁移创建表和 triggers 后执行 FTS5 `rebuild`，确保 M3.0 已存在的文档也进入索引。
该 external-content 与 trigger/rebuild 组合遵循 [SQLite FTS5 external-content 文档](https://www.sqlite.org/fts5.html)。

### 3. 首版 tokenizer 固定为 `unicode61`

索引配置为 `unicode61 remove_diacritics 1`。它由当前 `rusqlite + bundled` SQLite 提供，
不增加依赖。M3.1 只记录并验证其确定性行为；中文分词、查询解析和召回质量不在本单元
擅自引入额外 tokenizer。

### 4. 不引入 Tantivy 或搜索查询公共 API

FTS5 是当前规模和本地 SQLite 架构下的首选实现。M3.1 不创建独立 search crate，不实现
`MATCH` 查询封装、过滤器、BM25、snippet 或桌面 UI；这些分别留给 M3.2–M3.4。

## 未采用的方案

- **contentful FTS5 表**：会额外复制正文，增加数据源和更新一致性风险。
- **只在 Rust API 中手动同步 FTS**：容易被未来的其他数据库写入路径绕过；SQLite trigger
  能把一致性约束放在存储边界。
- **`trigram` tokenizer**：可能改善中文子串检索，但索引体积和查询成本更高；等真实
  语料和 M3.2 查询评估后再决定是否需要调整。
- **Tantivy**：当前没有基准证据证明 SQLite FTS5 不足，暂不增加复杂度和依赖。

## 风险与后续影响

- `unicode61` 不等于中文分词器；中文检索能力必须在真实语料评估中单独验证。
- external-content 索引如果被手工破坏，需要使用 FTS5 `rebuild` 恢复；后续可增加受控的
  重建入口。
- M3.1 未启用 FTS5 `secure-delete`，因为这会增加更新/删除成本；删除内容的物理残留
  和本地数据库安全擦除属于后续隐私评估，不改变当前搜索结果一致性。
