# M3.0 搜索数据模型验收记录

## 目标

在不提前实现 M3.1 FTS5 搜索的前提下，为 M2 产出的统一文档建立可迁移、可追溯的本地
canonical 存储边界。

## 本单元范围

- schema 2 → schema 3 迁移。
- `documents` canonical 表。
- `DocumentRecord` 的按 ID upsert、读取和删除。
- 本地来源路径、标题、正文和可选行范围的校验与 round-trip。
- 安全错误分类，不回显正文或完整路径。

明确不包含：FTS5 虚拟表、tokenizer、查询语法、BM25、snippet、搜索 UI、批量解析、
文件 watcher 和增量索引。

## 验收证据

| 检查项 | 证据 |
| --- | --- |
| 新数据库迁移到 schema 3 | `initialize_database` 测试检查 `nexus_metadata`、`file_metadata` 和 `documents` |
| 既有 schema 2 可升级 | v2 → v3 migration 测试并确认既有 metadata 保留 |
| 重复初始化安全 | 既有初始化幂等测试继续通过 |
| 文档读写边界 | `stores_updates_reads_and_deletes_document_records` |
| 来源追溯 | 文档 round-trip 恢复规范化本地路径 |
| 位置元数据 | 行范围和 whole-document（空范围）均覆盖 |
| 非法输入 | ID、标题、来源路径和位置范围测试覆盖，数据库无部分写入 |
| 隐私边界 | 错误文本不包含测试正文或完整路径 |

## 实际执行的检查

- `pnpm format`：通过。
- `pnpm lint`：通过，包含 workspace Clippy `-D warnings`。
- `pnpm typecheck`：通过，包含 workspace `cargo check`。
- `pnpm test`：通过；前端 6 项、`nexus-core` 36 项、`nexus-db` 17 项、桌面 3 项。
- `cargo test --workspace --locked`：通过；Rust 测试总计 56 项。
- `pnpm build`：通过。Windows 增量编译目录清理输出了非致命的权限提示，不影响构建结果。
- `git diff --check`：通过。

## 结果

M3.0 的 schema、数据库 API 和测试已实现。当前 canonical 正文只保存在本地 SQLite；
没有网络、AI、云端同步或 FTS5 依赖。下一步为 M3.1 SQLite FTS5 基础索引。
