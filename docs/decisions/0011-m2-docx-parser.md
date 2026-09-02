# ADR 0011：M2.3b DOCX 解析边界

- 状态：已接受
- 日期：2026-09-01
- 范围：`crates/nexus-core`

## 背景

M2.3a 已建立 HTML 到统一文本 `Document` 的单文件解析边界。M2.3b 需要处理本地
DOCX 文档，同时面对 DOCX 的 ZIP 容器、压缩条目大小、XML 解析和嵌入内容风险。当前
目标仍是可靠的本地文本基础，不提前建立正文数据库、批量调度或位置扩展模型。

## 决策

### 1. 使用 ZIP 容器和 XML 流式解析

`nexus-core` 增加：

- `zip = { version = "8.6.0", default-features = false, features = ["deflate-flate2-zlib-rs"] }`
  读取常见 Deflate DOCX，并避免 AES、BZip2、LZMA、XZ、Zstd 等不需要的压缩能力。
- `quick-xml = "0.41.0"` 作为直接依赖；该版本已经存在于工作区锁文件，使用其事件读取
  API 处理 WordprocessingML，不反序列化为大型通用对象树。

DOCX 本质上是 ZIP 包；解析器不把任何条目解压到用户文件系统，只在内存中读取主文档
XML，避免路径穿越和嵌入文件落盘行为。

### 2. 保持单文件显式 API

公开入口为：

```text
parse_docx_file(document_id, path, max_input_bytes, max_entry_bytes, max_output_bytes)
    -> Result<Document, ParseError>
```

调用方提供原始 ZIP 字节上限、`word/document.xml` 未压缩条目上限和提取后正文上限。
解析器只处理一个 `.docx` 普通文件，扩展名大小写不敏感。

### 3. 只提取主文档文本

解析器要求存在普通文件条目 `word/document.xml`，只提取其中 WordprocessingML 的
`w:t` 文本节点。`w:p` 产生段落边界，`w:br`/`w:cr` 产生换行，`w:tab` 产生制表符，
XML 预定义实体和字符引用会被还原。文本事件按顺序拼接，段落末尾和文档末尾的多余
空格/换行会被裁剪，以得到确定性正文。

不读取页眉、页脚、批注、脚注、关系目标、媒体、嵌入对象或其他 ZIP 条目；不执行宏、
外部链接或任何脚本。这样可以在不解释整个 OOXML 包的前提下交付可用于后续词法搜索的
主文档文本。

### 4. 沿用当前 Document 位置边界

标题使用完整文件名，来源使用规范化后的本地路径，位置使用
`DocumentLocation::whole_document()`。本单元不增加页码、段落 ID、XML 路径或字符偏移
字段；若后续真实需求需要段落定位，再单独提出数据模型 ADR。

### 5. 明确错误和资源行为

三个大小上限必须大于零。原始 ZIP 超限沿用 `parse_file_too_large`；主文档 XML 条目
超限返回 `parse_docx_entry_too_large`；提取正文超限返回 `parse_output_too_large`。
无效 ZIP、缺少主文档、无效 XML 和 UTF-8 错误分别返回固定安全分类。压缩、解压或 XML
失败不会 panic，不会写数据库或阻止调用方处理后续文件。

`Display` 和用户消息不包含路径、文件名、XML 正文、ZIP 条目名或原始系统错误；底层
ZIP/XML 错误只通过进程内 `Error::source` 保留诊断信息。失败时不会创建输出文件或发送
UI/网络事件。

## 备选方案

- 手写 ZIP 解压器：会重复实现 ZIP 格式、压缩算法和边界检查，可靠性和维护成本更差，
  因此使用成熟库并关闭未使用的压缩特性。
- 先把整个 DOCX 反序列化为通用 OOXML 结构：会增加大量结构和内存占用；当前只需要
  主文档文本，因此采用事件流读取。
- 读取所有 ZIP 条目：会把媒体、嵌入对象和关系文件带入解析边界，扩大隐私与资源风险，
  因此只读取固定的主文档条目。
- 扩展 DocumentLocation 保存段落或 XML 路径：当前搜索层还未消费该字段，提前改变公共
  模型会增加迁移成本，因此保留 whole-document。

## 结果与取舍

- 新增 `zip 8.6.0` 是 DOCX 必需的直接依赖；`quick-xml 0.41.0` 已在锁文件中，改为
  `nexus-core` 的直接依赖。ZIP 只启用读取 DOCX 所需的 Deflate 解压能力。
- `ZipArchive` 会读取 ZIP 中央目录，主文档 XML 会在内存中完整保留后再流式解析；三个
  上限限制了当前可观测资源边界，但极端中央目录和 XML 嵌套仍需真实语料测量。
- 只支持 UTF-8 主文档 XML；不会静默转码。DOCX 中的页眉、页脚、批注和嵌入内容暂不
  进入正文，若产品需要应作为独立范围评估。
- 输出丢失 OOXML 格式和细粒度位置，但保留可追溯本地来源，可直接交给后续确定性搜索。

## 验证

- 覆盖 Deflate DOCX、大小写扩展名、段落、换行、制表符、实体、无效 ZIP、缺失主文档、
  条目大小上限、输出大小上限、malformed XML、UTF-8 和安全错误展示。
- 使用隔离临时目录测试；不读取真实用户目录，不写入解压文件，不执行宏，不访问网络，
  不写 SQLite。
