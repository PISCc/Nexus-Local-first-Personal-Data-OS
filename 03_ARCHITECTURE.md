# Nexus 架构

## 推荐技术栈

-   桌面：Tauri
-   前端：React + TypeScript
-   核心：Rust
-   本地数据库：SQLite
-   初始全文搜索：SQLite FTS5
-   后续搜索引擎候选：Tantivy
-   文件监听：Rust `notify`
-   AI 层：后期按需求使用独立 Python 服务或 API
-   测试：Rust tests + Vitest + Playwright
-   CI：GitHub Actions

以上是初始方案，不是不可更改的教条。重大调整需要 ADR。

## 推荐仓库结构

``` text
nexus/
├── AGENTS.md
├── README.md
├── 00_PROJECT_BRIEF.md
├── 01_ROADMAP.md
├── 02_CODEX_WORKFLOW.md
├── 03_ARCHITECTURE.md
├── 04_M0_SPEC.md
├── 05_EXECUTION_PLAN.md
├── docs/
│   ├── decisions/
│   ├── specs/
│   └── research/
├── apps/
│   └── desktop/
├── crates/
│   ├── nexus-core/
│   ├── nexus-db/
│   ├── nexus-index/
│   ├── nexus-parser/
│   └── nexus-search/
├── packages/
│   └── ui/
├── tests/
│   ├── fixtures/
│   └── integration/
├── scripts/
└── .github/workflows/
```

## M0 实际 Rust 骨架

M0.3 只创建当前需要的三个 Rust crate，不提前创建 parser、index 或
search crate：

``` text
apps/desktop/src-tauri/  # Tauri desktop 壳层
crates/nexus-core/       # 与 UI 和平台无关的核心边界
crates/nexus-db/         # 本地数据库边界
```

依赖方向固定为：

``` text
nexus-desktop → nexus-core → nexus-db
```

`nexus-core` 和 `nexus-db` 不依赖 Tauri。M0.3 不定义数据库连接或业务
模型；SQLite 初始化和迁移由 M0.4 单独实现。

## M0.4 数据库边界

`crates/nexus-db` 对外提供按调用方传入路径初始化数据库的入口，以及文件元数据和
统一文档的本地持久化边界：

- `initialize_database(path)` 负责打开数据库、读取 `PRAGMA user_version`
  并执行待处理迁移。
- 当前 schema 版本为 `5`；迁移文件依次为
  `crates/nexus-db/migrations/0001_foundation.sql`、
  `crates/nexus-db/migrations/0002_file_metadata.sql` 和
  `crates/nexus-db/migrations/0003_documents.sql`、
  `crates/nexus-db/migrations/0004_documents_fts.sql`、
  `crates/nexus-db/migrations/0005_embeddings.sql`。
- 首个迁移只创建 `nexus_metadata(key, value)`，不包含文件索引或正文。
- M3.0 的 `documents` 表保存 canonical 文档 ID、local-file 来源路径、标题、正文和
  可选行范围；`source_path_key` 用于无损追溯，`source_path_display` 用于展示。
- `nexus-db` 通过 `DocumentRecord`、`upsert_document`、`get_document` 和
  `delete_document` 提供文档存储边界。
- M3.1 的 `documents_fts` 是以 `documents` 为 external content 的 FTS5 虚拟表，
  使用 `unicode61 remove_diacritics 1`；canonical 表上的 triggers 负责索引同步，
  schema 迁移对已有文档执行 rebuild。
- M5.1 的 `embedding_models` 登记模型 ID、版本、provider 类型和维度；
  `document_embeddings` 以文档 ID、模型 ID 和版本为联合主键，保存标题/正文输入指纹及
  little-endian 向量字节。模型身份和维度冲突不会被静默覆盖。
- 文档标题或正文更新、文档删除时，SQLite trigger 清理对应旧向量；因此新正文不会继续
  使用旧 embedding。向量损坏由读取边界拒绝，不让它静默参与搜索。
- 新数据库的迁移和版本更新在同一个事务中完成；未知版本返回明确错误。
- 数据库 crate 不假设 Tauri app data directory，生产路径由上层调用方决定。

## M0.5 启动与错误边界

- Tauri 桌面壳层负责初始化 stderr 日志、解析应用数据目录、准备目录，并调用
  `nexus-core::initialize`。
- `nexus-core` 负责组织数据库初始化并返回 `CoreError`；核心层不依赖 Tauri
  或前端。
- 数据库错误提供不含路径、内容或原始 SQLite 信息的 `kind` 分类和用户说明。
  原始错误仍保留在进程内用于错误链，但不会直接进入日志或 UI。
- 启动失败不会被静默忽略。桌面壳层将失败记录为安全分类，并把
  `ready` 或 `degraded` 状态交给 `get_startup_status` 命令。
- 前端先显示 `loading`，收到 command 结果后显示 `ready` 或 `degraded`；浏览器
  预览无法连接 Tauri 核心时也显示可理解的降级状态。
- M0.5 不写持久化日志，不上传数据，也不持有后续索引服务的数据库运行时连接。

## M1.0 文件元数据模型

- `nexus-db` 提供 `FileMetadata`、路径规范化、单条 upsert 和按路径读取入口。
- `file_metadata` 保存路径、文件名、扩展名、大小、可选的三个 Unix epoch 毫秒时间戳和可选类型标签。
- 路径只做绝对化，不执行 `canonicalize`，不跟随符号链接，也不进行大小写折叠。
- 数据库使用平台相关的 `path_key` 保存路径身份，同时用 `path_display` 提供展示文本，避免非 UTF-8
  路径导致崩溃。
- M1.0 不扫描目录、不读取文件内容、不计算哈希，也不创建正文或全文搜索表。

## M1.1 流式递归遍历

- `nexus-core` 提供实现 `Iterator` 的 `FileScanner`，逐项产出文件、跳过或失败结果。
- 默认不跟随符号链接；明确开启后使用解析后的目录身份去重，避免目录环路。
- 忽略路径按规范化路径及其子路径匹配；单项错误不会终止剩余扫描。
- 扫描器只读取文件系统元数据，不读取正文、不写数据库、不依赖 Tauri。
- 扫描统计、取消、后台任务和界面进度属于后续 M1 单元。

## M1.2 文件元数据批量持久化

- `nexus-db` 提供 `upsert_file_metadata_batch`，从结果迭代器逐批接收文件元数据，不要求调用方先收集完整路径列表。
- 每个批次使用独立 SQLite 事务；批次提交成功后才计入 `written` 和 `batches`，因此单个已提交批次不会被后续批次的错误回滚。
- 输入中的单条错误和元数据校验失败计入 `failed` 后继续；数据库连接、事务或 SQL 错误仍返回调用方，因为这表示持久化边界本身不可用。
- `FileMetadataBatchSummary` 提供 `received`、`written`、`failed` 和 `batches` 四项确定性统计。
- M1.2 不负责目录重扫、旧记录删除、取消、后台任务或 UI；扫描结果与手动重扫闭环在 M1.3 组合。

## M1.3 手动重扫闭环

- `nexus-core` 提供 `rescan_directory`，接收调用方传入的数据库路径、扫描根目录、扫描选项和批次大小。
- 重扫使用 `nexus-db` 的 `FileMetadataRescan` 会话：成功文件进入有界批次，失败和跳过路径进入保护集合，扫描完成后再清理确认不存在的旧记录。
- 会话使用 SQLite 连接级临时表保存本次扫描见到的路径和保护路径，不新增持久化 schema 字段，也不要求调用方收集完整路径列表。
- 只有位于选定根目录、未被本次扫描见到、且不在失败/跳过路径及其子树中的记录会被删除；相邻但不属于根目录的路径不会被误删。
- 返回 `RescanSummary`，明确提供成功文件数、失败数、跳过路径数、移除记录数和已提交批次数。
- 成功元数据在 `finish` 前暂存于连接级 `nexus_scan_pending`，取消或丢弃会话时不会应用到持久化表；最终应用和旧记录清理在一个事务中完成。

## M1.4 可取消重扫、进度事件和桌面界面

- `nexus-core` 提供 `RescanControl` 和 `rescan_directory_with_control`；任务在每个扫描结果前及最终提交前检查原子取消标记，并通过 `RescanProgress` 回调报告处理量、成功、失败和跳过统计。
- Tauri 壳层使用单活动任务槽位和后台线程；同一时间只允许一个重扫，重复启动返回 `rescan_already_running`，取消命令必须携带匹配的 `scanId`。
- 桌面层通过 `start_rescan`、`cancel_rescan`、`get_rescan_status` 三个 command，以及 `rescan-progress`、`rescan-finished` 两个事件与前端通信。事件字段使用 `camelCase`，错误只暴露固定的非敏感分类和中文说明。
- React 前端保留 M0 的暖纸张、苔绿色和锐利边框视觉系统，增加全中文文件索引工作区；用户输入完整目录路径后可启动、观察和取消重扫。
- 前端运行时校验 command 和事件 payload，并忽略不属于当前任务的事件；浏览器预览没有 Tauri 核心时保持诚实的降级提示。
- M1.4 不引入原生目录选择器、异步运行时、实时 watcher 或正文解析；其核心
  `rescan_directory_with_control` 仍保持 metadata-only。M3.6 在桌面任务中显式启用初始正文
  索引模式；M1.5 已按完整路径输入完成验收，原生选择器留作后续体验改进。

## M1 验收结果

- M1.0–M1.4 的代码、测试和桌面开发构建已完成；M1.5 验收记录见 `docs/acceptance/0001-m1-file-scanner.md`。
- 验收确认新增、更新、移除、失败保护、忽略路径、取消一致性、重复任务拦截和 100,000 条合成元数据批处理均有证据。
- M1 的元数据重扫路径只保存文件元数据，不保存正文；M2.0 已定义最小 `Document` 模型，
  M2.1 已增加纯文本、Markdown 和代码解析，正文数据库和全文索引随后由 M3 建立。

## 领域边界

-   UI 不直接承担数据库/索引业务逻辑。
-   Parser 不依赖 UI。
-   Search 面向统一 Document，而不是面向特定扩展名。
-   平台相关行为尽量隔离。
-   Core 层保持可测试。

## Document 思路

长期核心抽象不是 File，而是 Document。

``` text
Source -> Parser -> Document
```

Document 最终可能来自： - Local File - PDF - Code - Webpage - Note -
Email - Repository

M0–M2 不要为了未来数据源提前建立复杂插件系统。

## M2.0 最小 Document 模型

M2.0 只定义统一内容结果的领域边界，不实现具体格式解析。当前模型位于
`crates/nexus-core/src/document.rs`，包含以下字段和约束：

- `DocumentId` 是非空不透明字符串；模型不在本阶段自行生成 UUID、哈希或其他持久化 ID。
- `DocumentSource::LocalFile` 保存本地文件来源；路径要求非空、转为绝对路径，并复用数据库层的跨平台路径规范化规则，但不做 `canonicalize`，避免把解析模型绑定到文件当前是否仍然存在。
- `title` 要求非空；`body` 可以为空，以允许后续解析器表达空文件或只有元数据的结果。
- `DocumentLocation` 使用可选的 1-based 起止行号；整篇文档使用无范围位置表示。
- 校验失败返回可分类的安全错误，不回显正文、完整路径或其他用户内容。

M2.1 已在不改变上述模型边界的前提下增加纯文本、Markdown 和代码解析；实现位于
`crates/nexus-core/src/parser.rs`，解析器不依赖 UI，使用标准库执行大小限制、UTF-8
和 BOM 处理。Markdown 与代码在本阶段按普通文本读取，不建立 AST 或格式化输出。

M2.2 在同一解析模块中增加 `parse_json_file` 单文件入口，使用直接依赖
`serde_json` 校验 `.json` 内容。解析结果仍然是统一 `Document`；正文只移除开头
UTF-8 BOM，保留原始 JSON 文本。调用方提供文件大小和嵌套深度上限，错误使用独立的
`parse_json_invalid` 与 `parse_json_depth_exceeded` 分类。JSON 解析仍不写数据库、不
接入 UI 或批量调度。

M2.3a 在 `crates/nexus-core/src/parser/html.rs` 增加 `parse_html_file` 单文件入口，
使用直接依赖 `dom_query 0.27.0` 按 HTML5 规则容错构建临时 DOM。解析器只接受 `.html`
和 `.htm`，严格校验 UTF-8 并移除开头 BOM；`script`、`style`、`noscript` 和 `template`
及其内容不进入正文，其他内容通过确定性的空白和块边界规则生成可见文本。调用方分别
提供原始输入和提取后输出的字节上限；结果继续使用完整文件名和
`DocumentLocation::whole_document()`，不扩展当前位置模型。HTML 解析不写数据库、不
接入 UI 或批量调度。

M2.3b 在 `crates/nexus-core/src/parser/docx.rs` 增加 `parse_docx_file` 单文件入口，
使用直接依赖 `zip 8.6.0` 打开 DOCX ZIP 容器，并使用已有锁定版本的 `quick-xml 0.41.0`
流式读取 `word/document.xml`。解析器只提取主文档中 `w:t` 文本、段落、换行、制表符和
XML 实体；不解压到文件系统，不执行宏或嵌入资源，也不读取页眉、页脚、批注或关系目标。
原始 ZIP、主文档 XML 条目和提取后正文均有独立大小边界，结果继续使用完整文件名和
`DocumentLocation::whole_document()`，不扩展当前位置模型。DOCX 解析不写数据库、不接入
UI 或批量调度。

M2.3c 在 `nexus-core` 中增加单文件 `parse_pdf_file`：使用直接依赖 `lopdf 0.44.0` 在内存中
加载 PDF，按逻辑页序逐页提取文本，并以空行连接非空页面。调用方分别提供原始输入、单个
解压流和提取后正文的字节上限；`LoadOptions` 与页面文本提取的有界 API 共同防止压缩流
无限膨胀。解析器不渲染页面、不执行 OCR、不读取附件或外部资源，不处理密码，异常和 panic
均转换为安全错误；结果继续使用完整文件名和 `DocumentLocation::whole_document()`，不扩展
当前位置模型。PDF 解析不写数据库、不接入 UI 或批量调度。

## M3.0 搜索数据模型

M3.0 在 `nexus-db` 中引入 schema 3 和 `documents` canonical 表。该表保存统一文档
的稳定 ID、当前仅支持的 `local_file` 来源、无损来源路径键、展示路径、标题、正文和
可选的 1-based 行范围。文档 ID 是 upsert 主键，但不对来源路径施加“一文件一文档”
约束，以保留后续切分或多文档来源的空间。

正文先保存在 canonical 表中，作为 FTS5 索引的唯一事实来源；M3.0 本身不创建 FTS5
虚拟表，也不选择 tokenizer。数据库层不依赖 `nexus-core`，通过独立的 `DocumentRecord`
保持现有 `nexus-core → nexus-db` 依赖方向。当前不把 `documents` 与 `file_metadata`
建立级联外键，避免文件元数据重扫删除记录时静默删除用户正文；两者通过规范化来源路径
键保持可关联。

## M3.1 SQLite FTS5 基础索引

`documents_fts` 使用 external-content 模式，只保存标题和正文的倒排索引数据，正文仍
从 `documents` 读取。`documents_fts_after_insert`、`documents_fts_after_update` 和
`documents_fts_after_delete` 三个 SQLite triggers 将 canonical 表的变化同步到 FTS5，
因此现有 `DocumentRecord` API 不需要绕过数据库边界维护第二份事实来源。

迁移先创建 FTS5 表和 triggers，再执行 `rebuild` 覆盖 M3.0 已存在的文档。当前 tokenizer
明确为 `unicode61 remove_diacritics 1`；中文分词和查询语法留给后续真实语料评估与 M3.2。

## M3.2 查询语法和基本过滤器

`nexus-db::search_documents` 是当前查询边界，接收受限查询字符串和结果上限，返回带有
文档 ID、标题、来源路径、位置和可用文件元数据的 `SearchResult`，不返回完整正文。查询
支持关键词、双引号短语、文件名、来源路径、扩展名、文件类型，以及 modified/created/
accessed 的 UTC 日期过滤；多个条件按 AND 组合，结果按文档 ID 稳定排序。

查询解析器不把 raw FTS5 表达式交给 SQLite，而是将文本条件转换为绑定参数中的 phrase。
元数据过滤也全部使用绑定参数；有正文条件时使用独立的 FTS5 `MATCH` 查询分支，再与
canonical 文档表连接，避免可选条件 `OR` 与 MATCH planner 的冲突。没有正文条件时，
可以只按文件名、路径或元数据过滤。单次结果限制为 1–1000 条，M3.3 再增加 ranking
和 snippet；分页与质量评估留给后续单元。

## M3.3 确定性 ranking 和匹配片段

正文查询使用 FTS5 `bm25` 计算 lexical relevance：标题权重为 5，正文权重为 1，取负值
转换为数值越大越相关。结果按 relevance 降序、`document_id COLLATE BINARY` 升序排序，
保证相同分数下仍有确定性顺序。仅过滤器查询没有正文匹配，因此不产生 relevance，继续
按文档 ID排序。

查询同时通过 FTS5 `snippet()` 生成标题和正文片段，优先返回标题片段，否则返回正文片段。
片段最多 32 个 tokenizer tokens，命中范围使用纯文本 `⟦` / `⟧` 标记，省略使用 `…`；
`SearchResult` 不携带完整正文，也不生成 HTML。M3.3 不引入 LLM 或新搜索依赖，分页、
多片段和质量评估留给后续单元。

## M3.4 搜索 UI 和桌面命令边界

搜索 UI 以 `SearchView` 作为现有 M1 界面的扩展，默认显示全文搜索页；M1 文件索引页仍通过
侧栏保留。React 层只负责查询输入、loading、空结果、错误、取消后的展示和结果交互，不直接
读取 SQLite，也不持有数据库连接。

`search_documents` 是桌面命令边界：收到显式查询后打开现有应用数据目录中的 SQLite 数据库，
调用 `nexus-db::search_documents`，再返回不包含完整正文的 UI DTO。`open_document` 根据不透明
文档 ID 从数据库解析 canonical 来源路径，校验目标仍是文件后交给当前平台的系统打开器；命令
不会把路径交给前端以外的网络服务，也不记录正文、查询或完整路径。

前端为每次搜索分配请求 ID；取消时只撤销当前 UI 请求，迟到的命令结果会被丢弃。该取消语义不
中断已经提交给 SQLite 的同步查询，避免为 M3.4 引入新的数据库连接池或并发架构。搜索结果
中的 snippet 按纯文本分隔标记渲染为安全的 React 文本节点，不将后端字符串当作 HTML。

## M3.5 搜索质量评估边界

M3.5 的评估实现位于 `crates/nexus-db/tests/search_quality.rs`，只在测试目标中编译和运行，
不改变 `search_documents` 的生产签名，不添加 telemetry 或新的数据库依赖。测试将固定的
10 条代表性文档写入隔离临时数据库，使用 9 个带人工相关文档 ID 标注的关键词/短语查询。

每个查询执行 15 次，记录中位和 p95 延迟；质量指标为 Recall@3 和 Top-1 命中数；存储指标
同时记录 SQLite 文件总大小以及 `documents_fts_data` segment block 字节数。M3.2 的文档
ID 升序作为同一候选集合上的可解释基线，当前 M3.3 BM25 结果与其并列输出。延迟不设置机器
相关的硬阈值，评估报告只记录观测值。

当前结果显示该固定语料上的 BM25 macro Recall@3 为 0.9722，ID 顺序基线为 0.9444；这
支持保留现有确定性 ranking，但不足以证明中文、复杂格式或大规模个人资料的真实质量。
真实脱敏语料、更多查询标注和 M3 整体验收已在 M3.6 的范围记录中处理；本单元不调整
tokenizer、权重或搜索引擎，也不读取用户资料。

## M3.6 初始正文索引和整体验收

M3.6 将 M1 的流式扫描、M2 的单文件解析器和 M3 的 canonical/FTS 存储接成一个明确的
初始索引入口：`nexus-core::index_directory_with_control`。它使用同一个扫描器逐条读取
文件元数据；成功文件按扩展名进入 `parse_file`，转换成 `DocumentRecord` 后通过当前重扫
会话的 SQLite 连接写入 `documents`，现有 FTS5 trigger 随之维护 `documents_fts`。

`parse_file` 的默认边界集中在核心层：普通输入和输出 16 MiB、PDF 解压流 64 MiB、DOCX
主文档条目 16 MiB、JSON 最大嵌套深度 32。支持范围不扩大：txt、md、py、rs、js、ts、
java、cpp、json、html/htm、docx、pdf。未支持格式计入 `documents_skipped`，解析失败计入
`documents_failed` 并继续；文档写入或 SQLite 连接错误返回任务级错误。

为了不改变 M1 核心调用方，`rescan_directory_with_control` 继续 metadata-only；Tauri 的
`start_rescan` 请求增加默认关闭的 `indexContent` 开关，桌面文件索引页显式打开它。文档 ID
由规范化路径的稳定 FNV-1a 值生成 `file:<hex>`，不把用户路径放入 ID；文件移动后的身份
重建、陈旧正文清理和修改检测仍属于 M4。取消会停止后续扫描，已提交的单文档正文写入保持
canonical 与 FTS 原子一致，未完成的元数据重扫不会进入最终应用事务。

M3.6 的验收证据包括核心层真实目录 fixture 的正文搜索回归、解析失败继续处理、桌面端
indexContent 请求与进度/结果字段测试，以及 M3.5 固定语料质量、延迟和存储记录。当前不
引入新依赖、不上传内容、不建立 watcher 或语义检索。

## M4.0 文件变化判定

M4.0 在 `nexus-core` 提供独立的 `FileSnapshot` 和 `detect_file_changes`。调用方可以把
数据库中的历史文件元数据和当前扫描器产出的文件元数据分别转换为快照，再得到新增、
修改、未变化和消失四类结果。变化判定只使用路径、大小和修改时间；访问时间、创建时间
等不代表正文变化的字段不会触发重新处理。

结果按路径排序，以便后续批处理和诊断保持稳定顺序。只有大小相同且两侧都明确提供相同
修改时间时才认为文件未变化；任一侧缺少修改时间都会保守地归入修改。空路径或同一输入
侧的重复路径会返回安全错误，不会静默覆盖状态。

M4.0 的局部数据流为：

``` text
Previous File Metadata ─┐
                        ├─> FileSnapshot ─> Change Detection
Current Scanner Output ─┘                         |
                              Added / Modified / Unchanged / Removed
```

该模块不读取正文、不计算 hash、不写数据库、不启动文件监听，也不负责把变化应用到
`documents` 或 `documents_fts`。因此 M4.0 可以独立于平台事件源验证；M4.1 再将 CREATE、
UPDATE、DELETE、MOVE/rename 接入，M4.2 再处理重复事件和防抖。移动在当前阶段表示为旧
路径消失和新路径新增，不能提前声称两者是同一文件。

## M4.1 文件事件来源

M4.1 在 `nexus-core` 使用稳定版 `notify 8.2.0` 提供递归 `watch_directory`。底层平台
通知被转换为不依赖 `notify` 类型的 `FileEvent`：创建、修改、删除、重命名，以及表示
事件可能丢失的 `RescanRequired`。事件只携带规范化路径；监听回调不读取正文、读取元数据
或写入数据库。

监听根目录之外的路径被过滤。底层一次给出两端路径时直接生成重命名事件；底层分开发出
From/To 时，对连续的一对进行配对，无法配对的起点会在下一个非 To 事件到达时按移除处理。
移入或移出监听范围的重命名分别表现为新增或移除。`FileWatcher` 持有底层 watcher，
丢弃它即停止监听；`recv_timeout` 和 `try_recv` 只负责把事件交给调用方。

底层报告 `need_rescan` 时，适配器清除待配对重命名并输出完整根目录的 `RescanRequired`。
这比继续依赖可能不完整的局部事件更安全。M4.1 不实现事件去重、防抖、文件稳定性等待、
批处理、正文解析或数据库更新；这些属于 M4.2–M4.3。

## M4.2 事件归并与增量写入

M4.2 在 `nexus-core` 增加 `EventBatcher`。它以短暂安静窗口收集通知，并按路径保留最终
操作：创建或修改进入一次更新，删除进入一次移除，重命名拆为旧路径移除和新路径更新。
完整重扫信号覆盖尚未提交的局部事件；输出按路径稳定排序并限制单批次路径数量。

更新操作不会直接相信通知到达顺序。核心层在解析前后重新读取文件状态，文件仍在变化时
进行有限次等待和确认；解析失败保留已有 canonical 正文。准备完成后，`nexus-db` 在一个
SQLite 事务中同时更新 `file_metadata`、`documents` 和 FTS trigger 维护的索引状态，事务
失败时整个批次回滚并按有限次数重试。

该层只负责归并、稳定性确认和增量数据操作，不创建后台线程，也不持有应用窗口生命周期。
M4.3 的桌面任务负责持续消费事件、取消、关闭和重启恢复。

## M4.3 后台监听任务与关闭

桌面层的 `WatchManager` 只维护一个活动监听任务。任务在独立线程中依次持有
`FileWatcher`、`EventBatcher` 和 M4.2 的增量处理入口；UI 通过 command 查询状态，通过
事件接收启动、停止和批次统计，不直接访问文件正文或 SQLite。

任务同时使用已有的 `RescanControl` 和独立关闭标记。手动初始索引开始前会停止旧监听，
成功后才保存目录并启动新监听；应用退出时先取消重扫，再取消监听并等待线程结束。批次
提交失败会保留待处理批次并延迟重试，取消后不再接收新事件。

完成正文初始索引后，桌面层只在本地应用数据目录保存一个监听根目录。启动时读取该配置，
先执行一次完整索引，再恢复监听；因此关闭期间遗漏的操作不需要单独保存事件队列。状态、
错误和统计只使用固定说明与计数，不向 UI 或日志传递正文和底层路径。

## M4.4 增量索引最终验收

M4.4 以临时目录验证文件创建、修改、删除和移动后的最终记录；以临时扩展名写入再重命名
验证编辑器常见的原子保存路径；以重复通知验证同一个文件不会产生重复 canonical 或 FTS
行；以重新打开数据库和恢复配置验证关闭后的继续处理。

最终一致性依赖三层边界共同成立：事件归并器只输出有限且稳定的操作，核心层以当前文件
状态准备变更，数据库层用一个事务提交元数据、文档和 FTS。任何一层遇到可恢复异常时都
保留旧状态或走完整重扫，不把半条结果当成成功。

## M5.0–M5.4 本地向量与混合检索基线

M5 在既有全文搜索之上增加可替换的 `EmbeddingProvider` 边界，但不让 UI、数据库或文件
监听器直接依赖推理引擎。当前 provider 是 `nexus-core` 内的确定性本地特征向量实现：对
标题和正文做 signed hashing 与 L2 归一化，固定为 256 维，模型 ID 和版本分别为
`nexus-local-feature-hash` 与 `1`。它没有预训练语言模型知识，只用于验证本地向量数据流、
版本管理和检索回退；来源路径、文件名和时间字段不进入向量输入。

`nexus-db` schema 5 增加两个持久化表：`embedding_models` 保存模型身份、provider 类型和
维度；`document_embeddings` 按 `(document_id, model_id, model_version)` 保存输入指纹和
向量字节。写入前校验模型冲突、维度、有限数值、非零范数和文档存在性；同一模型版本的
配置不允许静默改变。标题或正文更新、文档删除会通过 SQLite trigger 清理旧向量，避免
过期向量继续参与查询。历史模型版本可以共存，但只有调用方明确请求的版本参与搜索。

查询仍先经过 M3 的受限语法解析。文本条件继续进入 FTS5 BM25，文本内容同时由同一个本地
provider 生成查询向量；文件名、路径、扩展名、类型和日期过滤器同时应用于 lexical 与
semantic 分支。semantic 分支只读取指定模型版本的本地向量，损坏或维度不匹配的记录会被
跳过；模型不存在或没有可用向量时，结果安全回退到 lexical。两路候选使用固定常数 60 的
Reciprocal Rank Fusion，按融合分数和文档 ID稳定排序，不用不具可比性的原始 BM25 与余弦
分数直接相加，也不替换确定性全文搜索。

初始索引成功后，桌面层执行一次有界 embedding 重建；M4 的文件更新只刷新受影响来源，
删除和解析失败依赖数据库清理旧向量并保留 lexical 可用性。核心层用文档 ID 游标和有限
批次处理，支持取消；已经提交的批次保持有效，后续失败不回滚之前的本地结果。搜索 DTO
只增加可选的语义相似度、两路排名和融合分数，前端没有新增数据库或文件读取职责。

M5.4 复用 M3 的固定 10 条文档、9 个查询评估集：当前基线与 hybrid 的 Recall@3 都为
`0.9722`，Top-1 命中均为 `9/9`。在当前小型 Debug 评估中 hybrid 还有额外计算开销，
因此暂不加入 reranking。采用真正的预训练本地模型前，必须另行确定模型文件来源、许可、
体积、运行时、设备资源、版本迁移和真实脱敏语料评估方案。

## 数据流（M1--M4）

``` text
Filesystem
   |
   +── Scanner ────────────────┐
   |                           |
   |                           +── FileWatcher ─> EventBatcher
   |                                                   |
   |                                                   +── Incremental Apply
   |
   +── File Metadata ───────────────┐       |
   |                               |       |
   +── Initial Index ─ Parser ─ Document ───┘
                                          |
                                          v
                                    SQLite + FTS
   |
Query Layer
   |
Desktop UI

Desktop WatchManager：监听任务、取消与关闭等待
```

## M5 数据流

``` text
                         Query
                           |
             +-------------+-------------+
             |                           |
       Content terms                Local provider
             |                           |
        SQLite FTS5                 Query vector
             |                           |
             +-------------+-------------+
                           |
                    RRF Fusion / fallback
                           |
                         Result
                           |
                         Desktop UI
```

## 性能假设

长期按以下数量级思考，但不要过早优化： - 100,000+ files - 1,000,000+
indexed records - multi-GB datasets

大操作优先 streaming/batching，避免无理由一次性载入全部数据。
