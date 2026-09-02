# M2.2 JSON 解析验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core/src/parser.rs`、`crates/nexus-core/src/lib.rs`、
  `crates/nexus-core/Cargo.toml`

## 验收目标

在不修改 `Document` 模型、不写入正文数据库、不接入 UI 和不开始 M2.3 的前提下，
为本地 JSON 文件建立合法性、原始文本保留、文件大小和嵌套深度边界。

## 已交付

- 增加直接依赖 `serde_json 1.0.151`，并公开单文件
  `parse_json_file(document_id, path, max_bytes, max_depth)`。
- 只处理扩展名大小写不敏感的 `.json` 普通文件；来源路径、完整文件名和
  whole-document 位置沿用统一 `Document` 模型。
- 严格校验 UTF-8，只移除开头 BOM；合法 JSON 的正文保留原始空白、换行、缩进和
  字段顺序，不做格式化或字段重排。
- `max_bytes` 必须大于零，文件大小和读取期间增长均受上限约束；`max_depth` 以根标量
  为 0、容器层级递增的规则限制嵌套深度。
- malformed JSON 和超深 JSON 返回固定安全分类；路径、文件名和正文不会出现在展示
  错误中，单次失败不改变后续调用能力。
- 不修改 SQLite schema，不接入 Tauri、React、批量解析、正文持久化、全文搜索或网络。

## 验收证据

Windows 本地工作区实际执行并通过：

- `cargo test -p nexus-core`：24 项通过。
- `pnpm test`：前端 6 项、`nexus-core` 24 项、`nexus-db` 14 项、桌面端 3 项，
  共 47 项通过。
- `pnpm format`：通过。
- `pnpm lint`：通过。
- `pnpm typecheck`：通过。
- `pnpm build`：前端生产构建和 Rust workspace 构建通过，退出码为 0；Windows 增量
  编译目录清理有非致命的访问提示，不影响构建结果。
- `git diff --check`：通过，无差异错误；Windows 工作区仅有 LF/CRLF 转换提示。

## 明确不包含

- PDF、DOCX、HTML 和其他结构化格式。
- JSON schema 校验、字段抽取、格式化输出或专用字段索引。
- 批量解析调度、正文 SQLite 表、全文搜索、增量索引、桌面解析命令。
- lossy UTF-8 解码、网络上传或 AI/Agent 功能。

## 结论

M2.2 验收通过。下一单元为 M2.3 PDF、DOCX、HTML 解析；在 M2 完成前不进入全文搜索。
