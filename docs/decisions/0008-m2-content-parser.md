# ADR 0008：M2.1 本地文本解析边界

- 状态：已接受
- 日期：2026-09-01
- 范围：`crates/nexus-core`

## 背景

M2.0 已经固定了统一的 `Document` 模型，但还没有把本地文件正文转换为
`Document`。M2.1 需要先提供可测试的纯文本读取基础，同时避免提前进入 JSON、
PDF、DOCX、HTML、全文搜索或通用插件架构。

## 决策

### 1. 解析器作为 nexus-core 内的独立模块

实现放在 `crates/nexus-core/src/parser.rs`，不新增 `nexus-parser` crate。当前
`Document` 模型已经位于 `nexus-core`，在此阶段拆分 crate 会改变依赖方向，
但不会带来实际功能收益。

### 2. 使用单文件显式 API

公开入口为：

```text
parse_local_file(document_id, path, max_bytes) -> Result<Document, ParseError>
```

文档 ID 由调用方提供；解析器不生成 UUID、哈希或数据库主键。标题使用完整文件名，
来源路径复用 `DocumentSource::local_file` 的绝对化规则，位置使用
`DocumentLocation::whole_document()`。

### 3. 当前支持范围是普通 UTF-8 文本

支持 `txt`、`md`、`py`、`rs`、`js`、`ts`、`java` 和 `cpp`，扩展名大小写不敏感。
Markdown 和代码在本单元按普通文本处理，不解析 AST、不格式化正文、不规范化换行。

### 4. 正文读取必须有界

调用方必须提供大于零的 `max_bytes`。解析器先检查文件大小，再通过有界读取处理
文件在读取期间增长的情况；超过上限返回错误，不把完整大文件无条件载入内存。

### 5. UTF-8 和 BOM 行为固定

正文必须是合法 UTF-8。只移除开头的 UTF-8 BOM，其他字节和换行保持不变；无效
UTF-8 返回分类错误，不使用 lossy 解码。

### 6. 错误和副作用边界

`ParseError` 为格式、元数据、普通文件、打开、读取、大小限制、UTF-8 和
`Document` 构造失败提供固定分类。`Display` 和用户消息不包含路径、文件名、正文
或原始系统错误；原始错误仅保留在进程内错误链中。

解析器只读取一个调用方指定的文件，不遍历目录、不写 SQLite、不发送 UI 事件、不
依赖 Tauri，也不上传内容。目录扫描阶段的符号链接策略仍由 `FileScanner` 决定；
解析器本身不执行目录遍历。

## 备选方案

- 新增 `nexus-parser` crate：可以提前隔离未来依赖，但会在 `Document` 仍位于
  `nexus-core` 时改变依赖方向，增加当前阶段的边界成本。
- 使用 Markdown 或代码解析库：当前只需要统一正文文本，会增加依赖、体积和安全
  审核范围，留给后续真实需求。
- 使用 lossy UTF-8 解码：可以提高表面成功率，但会静默损坏正文，影响后续搜索。
- 无大小限制读取：实现简单，但不符合大文件和本地可靠性要求。

## 结果与取舍

- M2.1 不改变 Document 数据模型，不修改数据库 schema，不需要新增依赖。
- 解析结果暂不持久化，也不接入桌面 UI；M3 再定义正文存储和搜索索引边界。
- 非 UTF-8 文件名的展示标题使用替代字符，但 `DocumentSource` 仍保留平台路径。
- 缺失文件在元数据检查阶段返回 `parse_file_metadata`；打开阶段的失败返回
  `parse_file_open`，两者均不回显敏感输入。

## 验证

- 覆盖 8 种扩展名、大小写扩展名、空文件、BOM、无效 UTF-8、大小限制、目录、
  缺失文件和安全错误消息。
- 使用隔离临时目录测试；没有读取真实用户目录。
