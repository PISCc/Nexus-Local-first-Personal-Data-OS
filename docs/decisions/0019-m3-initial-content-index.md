# ADR-0019：M3.6 初始正文索引接入

- 状态：Accepted
- 日期：2026-09-01
- 范围：M3.6 M3 整体验收

## 背景

M2 已经可以把单个 txt、Markdown、代码、JSON、HTML、DOCX 和 PDF 文件解析成统一
`Document`，M3 也已经有 canonical `documents` 表、SQLite FTS5、查询语法、ranking、
snippet 和桌面搜索页。但此前 M1 的真实目录重扫只写入 `file_metadata`，没有任何生产链路
把扫描出来的文件送进解析器；新数据库因此可以完成重扫，却没有可搜索的正文。

## 决策

### 1. 增加显式的初始索引入口，保留 M1 metadata-only API

`nexus-core::index_directory` 和 `index_directory_with_control` 复用已有流式扫描器、
`FileMetadataRescan`、取消控制和进度回调。每个普通文件先进入元数据重扫，再按扩展名调用
`parse_file`；成功的 `Document` 转成 `DocumentRecord`，在同一 SQLite 连接上写入
`documents`。现有 FTS trigger 负责同步 `documents_fts`。

`rescan_directory` / `rescan_directory_with_control` 不改变行为，仍然只处理文件元数据。
Tauri 的 `start_rescan` 请求增加默认值为 `false` 的 `indexContent`；桌面文件索引页明确
传入 `true`，因此旧的 core 和命令调用方不会被隐式改变。

### 2. 把批量索引的解析边界集中在核心层

`ParseOptions::default()` 使用以下固定上限：

- 普通文本、JSON、HTML、DOCX、PDF 原始输入：16 MiB；
- HTML、DOCX、PDF 提取正文：16 MiB；
- DOCX 主文档 XML 条目：16 MiB；
- PDF 单个解压流：64 MiB；
- JSON 嵌套深度：32。

这些默认值只供初始索引编排使用；单文件解析函数继续要求调用方显式提供限制。M3.6
不扩大 M2 已验收的格式范围，也不增加依赖。

### 3. 逐文件分类失败，不让环境异常终止全局扫描

不支持的扩展名计入 `documents_skipped`；损坏、权限、编码或资源超限等单文件解析错误
计入 `documents_failed`，并继续扫描后续文件。`upsert_document` 或 SQLite 连接/事务
失败表示持久化边界不可用，升级为任务级错误。单文档写入及其 FTS trigger 由 SQLite
单条写操作保持原子一致。

### 4. 使用不暴露路径的确定性文件文档 ID

初始本地文件文档 ID 为 `file:<16 位十六进制 FNV-1a>`，哈希输入是规范化路径的
平台无损表示。这样同一文件在重复初始索引中仍然 upsert 同一条记录，同时不把用户路径
写入 ID。文件移动后的身份迁移和碰撞处理不在本单元扩展，留给 M4 的变更检测与陈旧记录
策略。

### 5. 取消的语义

取消会在下一个扫描结果边界停止后续处理；已经提交的单文件正文记录可以保留，且每条记录
与其 FTS 状态保持一致。`FileMetadataRescan` 尚未 `finish` 的元数据暂存不会应用到最终
元数据表。UI 明确展示这一部分边界，不声称取消会回滚所有已完成正文写入。

## 未采用的方案

- **让 M1 `rescan_directory_with_control` 直接改变为读正文**：会破坏已验收的 metadata-only
  核心契约，也会让旧调用方突然承担解析 IO 和资源上限。
- **新增独立 watcher 或增量任务**：初始索引只需要一次显式扫描；创建/修改/删除/移动和
  陈旧正文清理属于 M4。
- **在前端直接访问 SQLite 或解析文件**：违反 UI 与 domain/database 边界。
- **引入哈希或搜索框架依赖**：标准库 FNV-1a、现有解析器和 SQLite FTS5 已满足当前规模。

## 后续影响和风险

- 首次索引会读取已支持文件正文，界面必须明确这是本地行为；不上传内容、不记录正文、
  文件名或搜索词。
- 默认上限是初始索引的产品策略，后续若需要用户可配置，应单独设计设置和迁移边界。
- 取消可能留下部分已建立的正文索引；它是可搜索且 canonical/FTS 一致的部分状态，M4
  需要补齐重新索引和陈旧清理策略。
- FNV-1a 只用于当前路径身份稳定性，不是安全哈希；若未来需要跨设备身份或抗碰撞保证，
  必须通过新 ADR 重新定义 ID 模型。
