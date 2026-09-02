# M2.3a HTML 解析验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core/src/parser/html.rs`、`crates/nexus-core/src/parser.rs`、
  `crates/nexus-core/src/lib.rs`、`crates/nexus-core/Cargo.toml`

## 验收目标

在不修改 `Document` 模型、不写入正文数据库、不接入 UI 和不开始 DOCX/PDF 解析的前提下，
为本地 HTML/HTM 文件建立确定性的可见文本输出、输入/输出资源边界和安全错误行为。

## 已交付

- 增加直接依赖 `dom_query 0.27.0`，并公开单文件
  `parse_html_file(document_id, path, max_input_bytes, max_output_bytes)`。
- 只处理扩展名大小写不敏感的 `.html` / `.htm` 普通文件；严格校验 UTF-8，只移除开头
  BOM；标题使用完整文件名，位置使用 whole-document。
- 按 HTML5 规则容错解析，过滤 `script`、`style`、`noscript` 和 `template` 及其内容，
  对可见文本进行确定性的空白和块边界规范化。
- 原始文件大小沿用有界读取；提取后的正文超过上限时返回 `parse_output_too_large`，
  展示错误不回显路径、文件名或正文。
- 不修改 SQLite schema，不接入 Tauri、React、批量解析、正文持久化、全文搜索或网络。

## 验收证据

Windows 本地工作区实际执行并通过：

- `cargo test -p nexus-core`：28 项通过。
- `pnpm test`：前端 6 项、`nexus-core` 28 项、`nexus-db` 14 项、桌面端 3 项，
  共 51 项通过。
- `pnpm format`：通过；前端 Prettier 和 Rustfmt 均通过。
- `pnpm lint`：通过；ESLint 和 Rust Clippy 均通过。
- `pnpm typecheck`：通过；TypeScript 和 Rust workspace 检查均通过。
- `pnpm build`：前端生产构建和 Rust workspace 构建通过，退出码为 0；Windows 增量
  编译目录清理有非致命的访问提示，不影响构建结果。
- `git diff --check`：通过，无差异错误；Windows 工作区仅有 LF/CRLF 转换提示。

单元测试覆盖 HTML 可见文本、非内容元素、实体、大小写扩展名、malformed HTML、BOM、
无效 UTF-8、零限制和输出超限。

## 明确不包含

- DOCX、PDF 和其他压缩容器或二进制格式。
- DOM 节点路径、元素偏移、页码、段落级位置或新的 `Document` 字段。
- HTML 脚本执行、网络请求、资源下载、Markdown 转换或页面渲染。
- 批量解析调度、正文 SQLite 表、全文搜索、增量索引、AI/Agent 功能。
