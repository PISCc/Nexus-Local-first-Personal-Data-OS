# M2.3b DOCX 解析验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core/src/parser/docx.rs`、`crates/nexus-core/src/parser.rs`、
  `crates/nexus-core/src/lib.rs`、`crates/nexus-core/Cargo.toml`

## 验收目标

在不修改 `Document` 模型、不写入正文数据库、不接入 UI 和不开始 PDF 解析的前提下，
为本地 DOCX 文件建立主文档文本输出、ZIP/XML/正文资源边界和安全错误行为。

## 已交付

- 增加 `zip 8.6.0`（仅启用 `deflate-flate2-zlib-rs`）和已在锁文件中的
  `quick-xml 0.41.0`，并公开单文件
  `parse_docx_file(document_id, path, max_input_bytes, max_entry_bytes, max_output_bytes)`。
- 只处理扩展名大小写不敏感的 `.docx` 普通文件；只读取 `word/document.xml`，不解压到
  文件系统，不执行宏或其他嵌入资源。
- 提取主文档 `w:t` 文本，支持段落、换行、制表符和 XML 实体；标题使用完整文件名，
  位置使用 whole-document。
- 原始 ZIP 文件大小、主文档 XML 未压缩条目大小和提取后正文大小均有上限；无效 ZIP、
  缺少主文档、条目超限、无效 XML、UTF-8 错误和正文超限均返回安全分类。
- 不修改 SQLite schema，不接入 Tauri、React、批量解析、正文持久化、全文搜索或网络。

## 验收证据

Windows 本地工作区实际执行并通过：

- `cargo test -p nexus-core`：32 项通过。
- `pnpm test`：前端 6 项、`nexus-core` 32 项、`nexus-db` 14 项、桌面端 3 项，
  共 55 项通过。
- `pnpm format`：通过；前端 Prettier 和 Rustfmt 均通过。
- `pnpm lint`：通过；ESLint 和 Rust Clippy 均通过。
- `pnpm typecheck`：通过；TypeScript 和 Rust workspace 检查均通过。
- `pnpm build`：前端生产构建和 Rust workspace 构建通过，退出码为 0；Windows 增量
  编译目录清理可能有非致命的访问提示，不影响构建结果。
- `git diff --check`：通过，无差异错误；Windows 工作区仅有 LF/CRLF 转换提示。

单元测试覆盖 Deflate DOCX fixture、实体、大小写扩展名、ZIP 无效、主文档缺失、条目上限、
输出上限、malformed XML 和安全错误展示。

## 明确不包含

- PDF、DOCX 页眉/页脚/批注/脚注、媒体、嵌入对象、关系目标和其他非主文档条目。
- ZIP 条目落盘、宏或脚本执行、外部链接访问、网络请求和内容上传。
- 页码、段落 ID、XML 路径、字符偏移或新的 `Document` 字段。
- 批量解析调度、正文 SQLite 表、全文搜索、增量索引、AI/Agent 功能。
