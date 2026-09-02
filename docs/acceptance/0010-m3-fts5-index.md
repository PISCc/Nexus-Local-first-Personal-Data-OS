# M3.1 SQLite FTS5 基础索引验收记录

## 目标

在 M3.0 canonical `documents` 表之上建立本地 SQLite FTS5 正文索引，明确 tokenizer，
并证明插入、更新、删除、迁移和事务回滚不会造成 canonical 文档与 FTS5 索引不一致。

## 本单元范围

- schema 3 → schema 4 migration。
- `documents_fts` external-content FTS5 表。
- `unicode61 remove_diacritics 1` tokenizer。
- SQLite insert/update/delete triggers。
- 已有文档的 migration rebuild。
- FTS5 integrity-check 和事务回滚验证。

明确不包含：查询语法公共 API、过滤器、BM25、snippet、搜索 UI、批量解析、文件 watcher、
增量索引和 Tantivy。

## 验收证据

| 检查项 | 证据 |
| --- | --- |
| FTS5 可用且配置明确 | 新数据库创建 `documents_fts`，schema 含 `unicode61` |
| trigger 完整 | insert、update、delete 三个 trigger 存在 |
| 新文档索引 | `keeps_fts_in_sync_for_document_insert_update_and_delete` |
| 更新移除旧词 | 同一测试确认旧词无结果、新词有结果 |
| 删除移除索引 | 同一测试确认删除后无结果 |
| 既有文档 rebuild | `rebuilds_existing_documents_when_migrating_to_fts_schema` |
| 事务一致性 | `rolls_back_document_and_fts_changes_together` |
| 索引完整性 | 各 FTS 测试执行 `integrity-check` |

## 结果

M3.1 的 FTS5 schema、同步 triggers、migration rebuild 和一致性测试已实现。当前只提供
索引维护，不提供搜索查询接口。下一步为 M3.2 查询语法和基本过滤器。

## 实际执行的检查

- `pnpm format`：通过。
- `pnpm lint`：通过，包含全 workspace Clippy `-D warnings`。
- `pnpm typecheck`：通过，包含前端 TypeScript 和 Rust workspace 检查。
- `pnpm test`：通过，前端 6 个测试、Rust 59 个测试全部通过。
- `cargo test --workspace --locked`：通过，Rust 36 + 20 + 3 个测试全部通过。
- `pnpm build`：通过；Windows 仅报告已知的增量编译目录清理权限提示，不影响构建产物。
- `git diff --check`：通过。
