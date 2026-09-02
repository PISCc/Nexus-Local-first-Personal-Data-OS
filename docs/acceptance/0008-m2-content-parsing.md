# M2 内容解析汇总验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core` 内容模型与单文件解析器，以及对应工程文档和测试

## 验收目标

在不进入 M3、不把正文写入 SQLite、不接入批量解析 UI 的前提下，确认 M2 当前支持的本地
文件格式都能产出统一 `Document`，失败行为可分类、fixture 可重现，且内容保持本地。

## 支持格式与统一结果

| 单元 | 入口 | 当前行为 |
| --- | --- | --- |
| M2.0 | `Document` | 统一 ID、来源、标题、正文和位置模型；来源路径为绝对路径 |
| M2.1 | `parse_local_file` | UTF-8 纯文本、Markdown 和代码；有界读取；只移除开头 BOM |
| M2.2 | `parse_json_file` | JSON 合法性和嵌套深度校验；保留去 BOM 后的原始正文 |
| M2.3a | `parse_html_file` | HTML5 容错、可见文本提取、非内容元素过滤和输出上限 |
| M2.3b | `parse_docx_file` | DOCX 主文档 XML 文本；ZIP、条目和输出上限 |
| M2.3c | `parse_pdf_file` | PDF 逻辑页序文本；输入、解压流和输出上限 |

所有入口均为调用方提供 `DocumentId` 和资源上限，结果使用完整文件名和
`DocumentLocation::whole_document()`。M2 不引入页码、DOM 路径、XML 路径、字符偏移或
渲染坐标字段。

## 失败、资源和隐私验收

- 不支持扩展名、文件元数据/打开/读取失败、普通文件检查失败和 UTF-8 失败都有独立安全
  分类；JSON、HTML、DOCX 和 PDF 的格式或结构失败也有对应分类。
- 原始文件读取均有上限；JSON 有嵌套深度上限；HTML、DOCX 和 PDF 的提取阶段分别有正文、
  条目/解压流和正文上限。超限不会生成部分 `Document`。
- `ParseError::kind()` 可供上层记录失败类型；`Display`/用户消息不回显完整路径、文件名、
  正文、XML、ZIP 条目名、PDF 对象内容或底层系统错误。
- DOCX 不落盘解压、不执行宏或读取非主文档资源；PDF 不渲染、不 OCR、不读取附件或外部
  资源。所有解析均不访问网络、不上传文件内容、不写正文 SQLite。
- 单文件失败只返回 `Result`，不改变解析器状态；调用方可以继续处理后续文件。

## 可重现性与验证证据

测试使用隔离临时目录。纯文本、JSON、HTML 和 malformed 输入由测试直接生成；DOCX 使用
测试 ZIP fixture；PDF 使用测试库生成的最小多页 fixture。测试不读取真实用户目录、不访问
网络、不依赖用户数据。

Windows 本地工作区实际执行并通过：

- `cargo test --workspace --locked`：`nexus-core` 36 项、`nexus-db` 14 项、桌面端 3 项，
  共 53 项通过。
- `pnpm test`：前端 6 项、`nexus-core` 36 项、`nexus-db` 14 项、桌面端 3 项，共 59 项通过。
- `pnpm format`：通过；前端 Prettier 和 Rustfmt 均通过。
- `pnpm lint`：通过；ESLint 和 Rust Clippy 均通过。
- `pnpm typecheck`：通过；TypeScript 和 Rust workspace 检查均通过。
- `pnpm build`：前端生产构建和 Rust workspace 构建通过，退出码为 0；Windows 增量
  编译目录清理有非致命的访问提示，不影响构建结果。
- `git diff --check`：通过，无差异错误；Windows 工作区仅有 LF/CRLF 转换提示。

## 明确不包含

- 正文 SQLite 表、FTS 索引、搜索查询、批量解析调度、文件 watcher 和增量索引。
- OCR、语义搜索、LLM、Agent、云端同步、认证、多用户和任何内容上传。
- 页码/段落/DOM/字符/坐标级位置扩展，以及 PDF 附件、媒体、脚本和外部链接处理。

## 结论

M2 内容解析验收通过。下一步为 M3.0 全文搜索数据模型；本记录不表示 M3 已实现。
