# M2.1 本地文本解析验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core/src/parser.rs`

## 验收目标

在不修改 Document 模型、不写入正文数据库、不接入 UI 和不增加第三方依赖的
前提下，为本地纯文本、Markdown 和代码文件建立可复现的解析基础。

## 已交付

- 支持 `txt`、`md`、`py`、`rs`、`js`、`ts`、`java`、`cpp`，扩展名大小写不敏感。
- 支持合法 UTF-8，并移除开头的 UTF-8 BOM；其余正文和换行保持不变。
- 使用调用方提供的字节上限，有界读取并拒绝超限文件。
- 生成现有统一 `Document`：调用方提供 ID，来源路径绝对化，标题使用完整文件名，
  位置使用 whole-document。
- 对不支持格式、元数据/打开/读取失败、目录、超限和无效 UTF-8 返回固定安全分类。
- 不新增 Cargo 或 pnpm 依赖，不改 SQLite schema，不接入 Tauri 或 React。

## 验收证据

Windows 本地工作区实际执行并通过：

- `cargo test -p nexus-core`：20 项通过。
- `pnpm test`：前端 6 项、`nexus-core` 20 项、`nexus-db` 14 项、桌面端 3 项，
  共 43 项通过。
- `pnpm format`：通过。
- `pnpm lint`：通过。
- `pnpm typecheck`：通过。
- `pnpm build`：前端生产构建和 Rust workspace 构建通过，退出码为 0。
- `git diff --check`：无差异错误；Windows 工作区仅有 LF/CRLF 转换提示。

## 明确不包含

- JSON、PDF、DOCX、HTML。
- Markdown AST、代码 AST、格式化或语法高亮。
- 正文 SQLite 表、全文搜索、增量索引、桌面解析命令和批量解析调度。
- lossy UTF-8 解码、网络上传或 AI/Agent 功能。

## 结论

M2.1 验收通过。下一单元为 M2.2 JSON 解析；在 M2 完成前不进入全文搜索。
