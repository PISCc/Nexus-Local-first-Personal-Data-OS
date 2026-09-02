# ADR-0013：M3.0 搜索数据模型

- 状态：Accepted
- 日期：2026-09-01
- 范围：M3.0 搜索数据模型

## 背景

M2 已经在 `nexus-core` 中定义了统一的 `Document`，解析器能够在本地内存中产出
标题、正文、来源路径和可选行范围。M1 的 `file_metadata` 只保存扫描得到的文件
元数据，不能作为正文的 canonical 存储，也不能单独承载后续全文搜索结果。

M3.0 需要先建立可迁移的本地文档存储边界，同时避免提前引入 FTS 查询、增量索引或
UI 数据库逻辑。

## 决策

### 1. 使用 schema 3 的 `documents` canonical 表

新增 `crates/nexus-db/migrations/0003_documents.sql`，表中保存：

- `document_id`：非空稳定 ID，作为 upsert 主键。
- `source_kind`：当前固定为 `local_file`。
- `source_path_key`：平台相关的无损路径键。
- `source_path_display`：用于展示的路径文本。
- `title`：非空标题。
- `body`：canonical 正文，可以为空。
- `line_start` / `line_end`：可选的 1-based 行范围。

正文先保存在该表中，后续 FTS5 索引以它为唯一事实来源。M3.0 不创建 FTS5 虚拟表，
不选择 tokenizer，也不实现搜索查询。

### 2. 数据库 crate 使用独立的存储记录类型

`nexus-db` 提供 `DocumentRecord`，而不是反向依赖 `nexus-core::Document`。这样保持
现有依赖方向 `nexus-core → nexus-db`，并让核心领域模型与 SQLite 表结构保持清晰边界。

本单元提供：

- `upsert_document`：按 ID 插入或完整更新。
- `get_document`：按 ID 读取，并恢复平台原始路径。
- `delete_document`：按 ID 删除并返回是否实际删除。

数据库不对来源路径施加“一文件一文档”唯一约束，具体来源适配器负责生成稳定 ID。

### 3. 来源追溯使用双路径表示

路径键与 M1 的路径规范化规则一致：路径绝对化，但不 `canonicalize`、不解析符号链接。
数据库同时保存无损键和展示文本。`documents` 不建立到 `file_metadata` 的级联外键，
避免文件元数据重扫时静默删除已持久化正文；两张表可通过规范化来源路径键关联。

### 4. 错误和迁移保持本地、安全、可恢复

文档 ID、标题、来源路径和行范围在写入前校验；schema migration 在现有事务机制中执行，
旧数据不删除。错误通过固定 `kind` 和中文 `user_message` 暴露，默认不回显正文、完整
路径或原始 SQLite 内容；原始错误仅保留在进程内错误链中。

## 未采用的方案

- **单独的 `document_content` 表**：职责分离更明显，但 M3.0 会增加不必要的 join 和
  一致性边界；当前 canonical 表已能直接承载后续外部内容 FTS5。
- **先创建 FTS5 表再补 canonical 存储**：会让索引承担唯一事实来源，增加重建和损坏
  恢复风险，因此推迟到 M3.1。
- **对 `file_metadata` 建立级联外键**：会把扫描记录生命周期错误地绑定到正文生命周期。
- **新增独立 search/index crate**：M3.0 尚无足够业务边界，继续使用现有 `nexus-db`
  存储边界。

## 后续影响

- M3.1 必须在写入、更新和删除 canonical 文档时明确维护 FTS5 一致性。
- M3.1 需要决定 tokenizer，并验证本地语料上的关键词、短语和 Unicode 行为。
- M4 的增量索引需要处理 `file_metadata` 与 `documents` 之间的 stale 记录清理，不能
  假定文件元数据删除会自动删除正文。
