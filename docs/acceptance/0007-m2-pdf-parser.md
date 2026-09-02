# M2.3c PDF 解析验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core/src/parser/pdf.rs`、`crates/nexus-core/src/parser.rs`、
  `crates/nexus-core/src/lib.rs`、`crates/nexus-core/Cargo.toml`

## 验收目标

在不修改 `Document` 模型、不写入正文数据库、不接入 UI 和不开始 M3 的前提下，为本地
PDF 文件建立确定性的页序文本输出、输入/解压流/正文资源边界和安全错误行为。

## 已交付

- 增加直接依赖 `lopdf 0.44.0`，关闭默认 feature，并公开单文件
  `parse_pdf_file(document_id, path, max_input_bytes, max_decompressed_bytes, max_output_bytes)`。
- 只处理扩展名大小写不敏感的 `.pdf` 普通文件；原始文件先使用既有有界读取，再在内存中
  通过 PDF 解析器处理。
- 按 PDF 逻辑页码顺序逐页提取文本；每页边缘空白被裁剪，空页跳过，非空页以两个换行
  连接；标题使用完整文件名，位置使用 whole-document。
- 使用加载阶段和页面提取阶段的解压上限，防止受限输入中的单个压缩流无限膨胀；提取后
  正文另有独立输出上限。
- 无效 PDF、解压流超限、输出超限、零限制、不支持扩展名和解析器异常均返回安全分类；
  不渲染页面、不执行 OCR、不处理密码、附件、嵌入媒体或外部资源。
- 不修改 SQLite schema，不接入 Tauri、React、批量解析、正文持久化、全文搜索或网络。

## 验证证据

Windows 本地工作区实际执行并通过：

- `cargo test -p nexus-core`：36 项通过，包含 4 项 PDF 专项测试。
- `cargo test --workspace --locked`：`nexus-core` 36 项、`nexus-db` 14 项、桌面端 3 项，
  共 53 项通过。
- `pnpm test`：前端 6 项、`nexus-core` 36 项、`nexus-db` 14 项、桌面端 3 项，共 59 项通过。
- `pnpm format`：通过；前端 Prettier 和 Rustfmt 均通过。
- `pnpm lint`：通过；ESLint 和 Rust Clippy 均通过。
- `pnpm typecheck`：通过；TypeScript 和 Rust workspace 检查均通过。
- `pnpm build`：前端生产构建和 Rust workspace 构建通过，退出码为 0；Windows 增量
  编译目录清理有非致命的访问提示，不影响构建结果。
- `git diff --check`：通过，无差异错误；Windows 工作区仅有 LF/CRLF 转换提示。

单元测试覆盖多页页序、损坏输入、安全错误展示、输入上限、解压流上限、输出上限、
扩展名和零限制。

## 明确不包含

- OCR、图片渲染、扫描件文本识别、密码输入和受保护 PDF 的解密流程。
- 页码、字符偏移、渲染坐标、对象路径或新的 `Document` 字段。
- 附件、嵌入媒体、JavaScript、外部链接、网络请求和 PDF 内容上传。
- 批量解析调度、正文 SQLite 表、全文搜索、增量索引、AI/Agent 功能。

## 结论

M2.3c 实现完成；M2.4 汇总验收已记录于
`docs/acceptance/0008-m2-content-parsing.md`。下一步进入 M3.0，但本记录不表示 M3 已实现。
