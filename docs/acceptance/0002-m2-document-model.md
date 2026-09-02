# M2.0 Document 模型验收记录

- 状态：已完成
- 日期：2026-09-01
- 范围：`crates/nexus-core` 的最小统一文档模型

## 验收目标

在不实现具体格式解析、不读取文件正文、不增加第三方依赖的前提下，固定后续
M2.1 解析器可以共同使用的 `Document` 数据边界。

## 已交付

- `Document` 包含 ID、来源、标题、正文和位置元数据。
- `DocumentSource::LocalFile` 支持本地文件来源；路径要求非空，转为绝对路径，并复用数据库层的跨平台路径规范化规则。
- `DocumentId` 是非空不透明标识；M2.0 不擅自决定哈希、UUID 或数据库主键方案。
- `DocumentLocation` 支持整篇文档和 1-based 起止行范围。
- 非法 ID、标题、来源路径和行范围返回固定错误分类与中文用户说明，不回显正文或完整路径。

## 验收证据

Windows 本地工作区已执行并通过：

- `pnpm format`
- `pnpm lint`
- `pnpm typecheck`
- `pnpm test`：前端 6 项、`nexus-core` 13 项、`nexus-db` 14 项、桌面端 3 项全部通过。
- `pnpm build`：前端生产构建和 Rust 工作区构建全部通过。
- `git diff --check`：通过，仅有 Windows 工作区的 LF/CRLF 转换提示。

## 明确不包含

- 不读取 txt、Markdown、代码或 JSON 文件正文。
- 不处理 UTF-8 BOM、文件大小限制或解析失败。
- 不建立正文数据库、全文搜索或语义搜索接口。
- 不上传文件内容，也不引入解析器插件框架。

## 结论

M2.0 验收通过。下一步实现 M2.1：在现有模型边界上增加纯文本、Markdown 和代码
解析，并为编码、大小限制、单文件失败和测试 fixture 建立明确契约。
