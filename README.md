# Nexus

Nexus 是一个本地优先的个人数据操作系统，目标是把文件、笔记、代码和其他
个人资料组织成可靠、可检索、可追溯的本地基础。

当前项目已完成 M0 工程基础、M1 本地文件扫描、M2 内容解析（M2.0–M2.4）、M3 全文搜索以及
M4.0–M4.4 增量索引；M5.0–M5.4 已完成本地向量混合检索基线。第一阶段具备本地建立、搜索和
自动维护文件内容索引的基础；M5 当前使用确定性本地特征向量验证完整链路，尚未选定预训练模型，
M6 问答和其他人工智能功能尚未开始。

## 产品原则

- 默认本地运行，核心功能不依赖网络或人工智能。
- 用户拥有自己的数据；未经明确选择，不上传文件、路径、内容或搜索历史。
- 先建立确定性的索引和搜索，再考虑语义能力。
- 失败必须可观察、可解释，单个文件或目录异常不应拖垮整个任务。
- 优先保持简单、明确、可测试的模块边界。

## 当前进度

| 阶段      | 状态     | 结果                                       |
| --------- | -------- | ------------------------------------------ |
| M0.0–M0.3 | 已完成   | 工具链、仓库、Tauri 壳层和 Rust crate 边界 |
| M0.4      | 已完成   | SQLite 初始化、迁移和版本控制              |
| M0.5      | 已完成   | 启动日志、核心错误和启动状态               |
| M0.6      | 已完成   | 数据库、核心和前端状态测试                 |
| M0.7      | 已完成   | README、GitHub Actions 和 M0 架构决策      |
| M1.0      | 已完成   | 文件元数据模型、schema 2 和数据库读写入口  |
| M1.1      | 已完成   | 流式递归遍历、忽略路径和符号链接策略       |
| M1.2      | 已完成   | 有界批量事务写入、upsert 和失败统计        |
| M1.3      | 已完成   | 手动重扫、新增/更新/移除和安全统计         |
| M1.4      | 已完成   | 后台重扫、取消、进度事件和全中文界面       |
| M1.5      | 已完成   | 本地文件扫描验收和结果记录                 |
| M2.0      | 已完成   | 最小 Document 模型、来源和位置元数据边界   |
| M2.1      | 已完成   | 纯文本、Markdown 和代码解析                |
| M2.2      | 已完成   | JSON 原始文本解析、大小和嵌套深度边界      |
| M2.3a     | 已完成   | HTML 可见文本解析、过滤和输出大小边界      |
| M2.3b     | 已完成   | DOCX 主文档文本解析和 ZIP/XML 资源边界     |
| M2.3c     | 已完成   | PDF 文本解析和输入/解压流/输出资源边界     |
| M2.4      | 已完成   | M2 支持格式、失败行为和隐私边界验收        |
| M3.0      | 已完成   | canonical 文档存储、来源追溯和安全读写边界 |
| M3.1      | 已完成   | SQLite FTS5 正文索引和一致性维护           |
| M3.2      | 已完成   | 查询语法和基本过滤器                        |
| M3.3      | 已完成   | 确定性 ranking 和匹配片段                  |
| M3.4      | 已完成   | 搜索 UI、状态和结果交互                     |
| M3.5      | 已完成   | 固定语料、查询集和搜索质量评估              |
| M3.6      | 已完成   | 初始正文索引、端到端搜索验收和边界记录      |
| M4.0      | 已完成   | 基于路径、大小和修改时间的变化判定          |
| M4.1      | 已完成   | 本地文件事件监听、归一化和重扫信号          |
| M4.2      | 已完成   | 事件去重、防抖、稳定性确认和事务批量写入    |
| M4.3      | 已完成   | 后台监听任务、取消、关闭和启动恢复          |
| M4.4      | 已完成   | 文件生命周期、原子保存和重启恢复验收        |
| M5.0–M5.4 | 基线完成 | 本地向量、版本化存储、BM25+向量混合检索和评估 |

## 系统要求

桌面应用当前按 Windows MSVC 环境验证，需要：

- Windows 10 或更高版本。
- Node.js `>=22.23.2`。
- pnpm `11.19.0`。
- Rust stable toolchain，以及 Rustfmt、Clippy 和
  `x86_64-pc-windows-msvc` 目标。
- Visual Studio 2022 Build Tools 的 C++ 工具链和 Windows SDK。
- 可用的 WebView2 运行时。

不需要全局安装 Tauri CLI 或 SQLite CLI；Tauri CLI 已作为前端开发依赖锁定在
仓库中，SQLite 由 Rust 的 `rusqlite + bundled` 依赖提供。

## 安装

```text
git clone https://github.com/PISCc/Nexus-Local-first-Personal-Data-OS.git
cd Nexus
pnpm install --frozen-lockfile
```

Rust 会读取根目录的 `rust-toolchain.toml`。在 Windows 上，如果 Rust 或 C++
工具链尚未安装，请先完成系统要求中的安装，再执行项目命令。

## 开发与检查命令

所有命令都从仓库根目录执行：

| 命令             | 用途                             |
| ---------------- | -------------------------------- |
| `pnpm dev`       | 启动 Tauri 桌面应用              |
| `pnpm dev:web`   | 启动浏览器前端预览               |
| `pnpm format`    | 检查前端和 Rust 格式             |
| `pnpm lint`      | 执行 ESLint 和 Rust Clippy       |
| `pnpm typecheck` | 执行 TypeScript 和 Rust 类型检查 |
| `pnpm test`      | 执行前端与 Rust 测试             |
| `pnpm build`     | 构建前端和 Rust 工作区           |

浏览器预览没有 Tauri 桌面核心，因此启动状态会显示“降级 / 需处理”；这是
预览环境的诚实提示。使用 `pnpm dev` 启动桌面应用后，Tauri 会在应用数据目录
中初始化 `nexus.sqlite3`，正常情况下显示“本地 / 就绪”。

## 当前架构

M0 的 Rust 依赖方向是：

```text
nexus-desktop → nexus-core → nexus-db
```

核心层不依赖 Tauri，数据库层不假设 Tauri 路径。当前数据库包含
`nexus_metadata`、`file_metadata`、`documents`、`embedding_models` 和
`document_embeddings` 表；`documents.body` 是 canonical 正文；`documents_fts` 是使用
`unicode61` 的 SQLite FTS5 external-content 索引，由 triggers 与 canonical 表保持同步。
向量表按模型 ID/版本保存，仅由本地 embedding 管线使用；文档标题或正文更新、删除时，
对应旧向量会自动清理。

长期数据流按以下方向演进：

```text
Source → Parser → Document → SQLite + FTS → Query Layer → Desktop UI
```

M1.0–M1.3 负责文件元数据模型、流式递归遍历、批量持久化和手动重扫；M1.4 将其接入后台任务、取消、进度事件和全中文界面，不实现内容解析、全文搜索或人工智能层。

M2.0 在 `nexus-core` 中定义最小 `Document` 模型：包含不透明 ID、本地文件来源、标题、正文和可选的 1-based 行范围；本地文件路径统一为绝对路径并使用数据库层的跨平台规范化规则。M2.1 在 `nexus-core` 中实现单文件纯文本、Markdown 和代码解析：按扩展名选择支持范围，使用调用方提供的字节上限读取 UTF-8 内容，移除开头 BOM，并生成 whole-document 结果。该阶段不引入新的解析依赖，也不写入正文数据库。

核心层的 `rescan_directory_with_control` 接收调用方传入的数据库路径、扫描根目录、取消控制器和进度回调，完成一次可取消的元数据重扫；M3.6 增加 `index_directory_with_control`，在同一流式任务中把已支持文件解析为 `Document` 并写入 `documents`/FTS。桌面层通过 `start_rescan` 的 `indexContent` 模式、`cancel_rescan`、`rescan-progress` 和 `rescan-finished` 接入任务控制。当前界面要求用户输入完整目录路径，暂不增加原生目录选择器依赖。

M2.2 在 `nexus-core` 中增加单文件 `parse_json_file`：使用 `serde_json` 校验 JSON，
保留去除 BOM 后的原始正文，并由调用方提供文件大小和嵌套深度上限。M2.2 不写入正文
数据库，也不接入批量调度、桌面 UI 或全文搜索。

M2.3a 在 `nexus-core` 中增加单文件 `parse_html_file`：使用已存在于工作区锁文件中的
`dom_query 0.27.0` 按 HTML5 规则容错解析，移除 `script`、`style`、`noscript` 和
`template` 内容，并把规范化后的可见文本生成统一 `Document`。输入和提取后正文均有
调用方提供的大小上限；结果使用完整文件名和 whole-document 位置。M2.3a 不写入正文
数据库，不接入批量调度、桌面 UI 或全文搜索。

M2.3b 在 `nexus-core` 中增加单文件 `parse_docx_file`：使用 `zip 8.6.0` 读取 DOCX
压缩容器中的 `word/document.xml`，使用 `quick-xml 0.41.0` 提取段落、换行、制表符和
XML 实体。原始 ZIP、主文档 XML 条目和提取后正文分别有调用方提供的大小上限；不解压
到文件系统，不执行宏或资源，也不写入正文数据库、接入桌面 UI 或全文搜索。

M2.3c 在 `nexus-core` 中增加单文件 `parse_pdf_file`：使用 `lopdf 0.44.0` 在内存中按
PDF 逻辑页序提取文本，以空行连接非空页面，并生成 whole-document `Document`。原始
文件、单个解压流和提取后正文分别有调用方提供的大小上限；有界 PDF 加载和页面提取
不渲染页面、不执行 OCR、不处理密码或外部资源，解析器 panic 也会被转换为安全错误。

M3.0 在 `nexus-db` 中增加 schema 3 和 `documents` canonical 表，保存统一文档 ID、
本地来源路径、标题、正文和可选行范围；数据库同时保留无损路径键与展示路径，使记录
可以追溯到原始文件。`DocumentRecord` 提供按 ID 的 upsert、读取和删除入口，校验和
错误分类不回显正文或路径。M3.0 不建立 FTS5 表、不实现查询语法、ranking、snippet
或 UI；这些属于 M3.1 及后续单元。

M3.1 在 schema 4 中增加 `documents_fts` external-content FTS5 表，索引标题和正文，
明确使用 `unicode61 remove_diacritics 1`。SQLite triggers 负责 canonical 文档插入、
更新和删除时的索引维护；迁移会对已有文档执行 rebuild。M3.1 已验证新增、更新、删除、
迁移重建和事务回滚的一致性，但不提供查询语法、ranking、snippet 或 UI。

M3.2 在 `nexus-db` 中增加 `search_documents` 查询入口，支持关键词、双引号短语、文件名、
路径、扩展名、文件类型和 UTC 日期过滤。多个条件按 AND 组合，结果按文档 ID 稳定排序并
限制数量；查询解析器不直接暴露 raw FTS5 操作符，错误分类不回显查询内容、路径或正文。
M3.2 不实现 ranking、snippet、分页或 UI。

M3.3 在同一查询入口上增加 FTS5 BM25 relevance 和有限长度匹配片段：标题权重高于正文，
结果按 relevance 降序并以文档 ID 作为稳定 tie-break；snippet 以纯文本 `⟦命中⟧` 标记，
不返回完整正文、不生成 HTML，也不依赖 LLM。M3.3 不实现 UI、分页或完整搜索质量评估。

M3.4 在 Tauri 桌面壳层接入搜索 UI：默认进入全文搜索页，支持显式提交查询、loading、空结果、
错误和取消后的安全状态；结果显示标题、路径、类型、时间、相关性和纯文本 snippet。UI 只调用
`search_documents` 和 `open_document` 命令，不直接访问 SQLite；结果不携带完整正文，点击结果可
通过本地系统打开原文件。M1 文件索引页和重扫流程继续保留；M3.4 不实现分页、增量索引或搜索
质量评估。

M3.5 增加仅测试可见的离线质量评估 harness：使用 10 条固定代表性文档和 9 个带人工相关性
标注的查询，记录 Recall@3、Top-1 命中、15 次测量的中位/p95 延迟、SQLite 文件大小和
FTS5 segment bytes，并与 M3.2 文档 ID 顺序基线比较。本次评估中 BM25 macro Recall@3 为
0.9722，基线为 0.9444；结果详见 M3.5 验收记录。M3.5 不读取真实用户资料、不调整算法
权重；M3.6 在此基础上补齐初始正文索引和整体验收。

M3.6 补齐初始索引的端到端链路：桌面文件索引页默认启动 `indexContent` 模式，流式扫描
文件元数据后按扩展名调用 M2 已验收的解析器，将 canonical `DocumentRecord` 写入 SQLite，
由 FTS5 trigger 同步正文索引。支持 txt、md、代码、JSON、HTML/HTM、DOCX 和 PDF；默认
原始输入/正文上限为 16 MiB，PDF 解压流上限为 64 MiB，JSON 深度上限为 32。未支持格式
计入跳过，单文件解析失败计入失败并继续，数据库写入失败才终止任务。M1 的核心元数据
重扫 API 仍保持 metadata-only；增量更新、陈旧正文清理和文件变更监听留给 M4。

M4.0 增加独立的文件变化判定：核心层比较上次和本次快照，按路径、大小和修改时间识别
新增、修改、未变化和消失的文件。该单元不读取正文、不计算 hash、不写数据库，也不引入
文件监听；事件接入、去重、防抖和自动更新留给 M4.1–M4.4。

M4.1 增加本地文件事件来源：使用 `notify 8.2.0` 递归监听指定目录，并将平台差异转换为
创建、修改、删除、重命名和完整重扫信号。事件来源与扫描、解析和数据库逻辑分离；当前
不自动更新索引，去重、防抖和批处理留给 M4.2。

M4.2 增加事件归并和增量写入：以短暂安静窗口合并重复事件，同一路径只保留最终操作，
并在正文解析前后确认文件状态。成功的元数据、canonical 正文和 FTS 更新在一个本地
事务中提交；数据库失败可重试，单文件解析失败不会覆盖已有正文。

M4.3 增加桌面后台监听任务：事件等待和增量处理不占用 UI 线程，任务可以取消；应用关闭时
停止接收新事件并等待任务退出。成功完成初始正文索引后，监听目录保存于本地数据目录，
重启时先执行一次完整恢复，再重新开始自动同步。

M4.4 完成创建、修改、删除、移动、编辑器临时文件原子保存、重复事件、事务回滚和重启后
最终一致性验收。相关实现不引入网络、LLM 或云端服务。

M5.0–M5.4 增加了可替换的 embedding provider 边界、版本化的本地向量存储、查询向量生成和
BM25 + vector 的 Reciprocal Rank Fusion（RRF）检索。当前默认 provider 是不依赖网络、模型
文件或新推理运行时的确定性特征向量基线，只使用文档标题和正文；它用于验证数据流和回退行为，
不宣称具备预训练语言模型的同义理解能力。全文搜索始终保留，缺少向量或向量异常时自动回退
到 lexical 结果。M4 的全量索引、文件更新和重扫流程都会在本地维护对应向量；本次固定评估集
上 hybrid 与 BM25 的 Recall@3 和 Top-1 相同，因此暂不加入 reranking。

## 数据与隐私

- 数据库默认位于 Tauri 应用数据目录。
- 启动日志只输出到 stderr，只记录固定消息和安全错误分类。
- 日志不记录文件内容、文件名、搜索词或完整用户路径。
- 数据库初始化失败会进入可理解的降级状态，不会静默忽略。
- 项目当前不上传数据、不要求云服务，也不要求 LLM。
- 核心层的 M1 元数据重扫只读取文件元数据；桌面端的 M3.6 初始索引会在本地读取已支持
  文件正文并写入 `documents`/FTS，目录路径仅作为本地命令参数使用，不进入日志或事件说明。
- M5 的向量只在本地由标题和正文生成并写入本地 SQLite；不访问网络，不发送文件、路径、
  向量或查询历史，也不把来源路径和文件名作为向量输入。

## 项目文档

- [项目简报](./00_PROJECT_BRIEF.md)
- [路线图](./01_ROADMAP.md)
- [Codex 协作流程](./02_CODEX_WORKFLOW.md)
- [架构说明](./03_ARCHITECTURE.md)
- [M0 规格](./04_M0_SPEC.md)
- [执行计划](./05_EXECUTION_PLAN.md)
- [M0 架构决策记录](./docs/decisions/0001-m0-engineering-foundation.md)
- [M1.0 文件元数据决策记录](./docs/decisions/0002-m1-file-metadata-model.md)
- [M1.1 文件扫描器决策记录](./docs/decisions/0003-m1-file-scanner.md)
- [M1.2 文件元数据批量持久化决策记录](./docs/decisions/0004-m1-file-metadata-batch-persistence.md)
- [M1.3 手动重扫决策记录](./docs/decisions/0005-m1-manual-rescan.md)
- [M1.4 可取消重扫、进度事件和桌面界面决策记录](./docs/decisions/0006-m1-rescan-control-and-ui.md)
- [M1 本地文件扫描验收记录](./docs/acceptance/0001-m1-file-scanner.md)
- [M2.0 Document 模型决策记录](./docs/decisions/0007-m2-document-model.md)
- [M2.0 Document 模型验收记录](./docs/acceptance/0002-m2-document-model.md)
- [M2.1 本地文本解析决策记录](./docs/decisions/0008-m2-content-parser.md)
- [M2.1 本地文本解析验收记录](./docs/acceptance/0003-m2-content-parser.md)
- [M2.2 JSON 解析决策记录](./docs/decisions/0009-m2-json-parser.md)
- [M2.2 JSON 解析验收记录](./docs/acceptance/0004-m2-json-parser.md)
- [M2.3a HTML 解析决策记录](./docs/decisions/0010-m2-html-parser.md)
- [M2.3a HTML 解析验收记录](./docs/acceptance/0005-m2-html-parser.md)
- [M2.3b DOCX 解析决策记录](./docs/decisions/0011-m2-docx-parser.md)
- [M2.3b DOCX 解析验收记录](./docs/acceptance/0006-m2-docx-parser.md)
- [M2.3c PDF 解析决策记录](./docs/decisions/0012-m2-pdf-parser.md)
- [M2.3c PDF 解析验收记录](./docs/acceptance/0007-m2-pdf-parser.md)
- [M3.0 搜索数据模型决策记录](./docs/decisions/0013-m3-search-data-model.md)
- [M3.0 搜索数据模型验收记录](./docs/acceptance/0009-m3-search-data-model.md)
- [M3.1 FTS5 基础索引决策记录](./docs/decisions/0014-m3-fts5-index.md)
- [M3.1 FTS5 基础索引验收记录](./docs/acceptance/0010-m3-fts5-index.md)
- [M3.2 查询语法和过滤器决策记录](./docs/decisions/0015-m3-query-syntax-and-filters.md)
- [M3.2 查询语法和过滤器验收记录](./docs/acceptance/0011-m3-query-syntax-and-filters.md)
- [M3.3 ranking 和 snippet 决策记录](./docs/decisions/0016-m3-ranking-and-snippet.md)
- [M3.3 ranking 和 snippet 验收记录](./docs/acceptance/0012-m3-ranking-and-snippet.md)
- [M3.4 搜索 UI 决策记录](./docs/decisions/0017-m3-search-ui.md)
- [M3.4 搜索 UI 验收记录](./docs/acceptance/0013-m3-search-ui.md)
- [M3.5 搜索质量评估决策记录](./docs/decisions/0018-m3-search-quality.md)
- [M3.5 搜索质量评估验收记录](./docs/acceptance/0014-m3-search-quality.md)
- [M3.6 初始正文索引决策记录](./docs/decisions/0019-m3-initial-content-index.md)
- [M3.6 整体验收记录](./docs/acceptance/0015-m3-acceptance.md)
- [M4.0 文件变化判定决策记录](./docs/decisions/0020-m4-change-detection.md)
- [M4.0 文件变化判定验收记录](./docs/acceptance/0016-m4-change-detection.md)
- [M4.1 文件事件来源决策记录](./docs/decisions/0021-m4-file-event-source.md)
- [M4.1 文件事件来源验收记录](./docs/acceptance/0017-m4-file-event-source.md)
- [M4.2 事件归并与批量写入决策记录](./docs/decisions/0022-m4-event-coalescing-and-batch-write.md)
- [M4.2 事件归并与批量写入验收记录](./docs/acceptance/0018-m4-event-coalescing-and-batch-write.md)
- [M4.3 后台监听任务与关闭决策记录](./docs/decisions/0023-m4-background-watch-task.md)
- [M4.3 后台监听任务与关闭验收记录](./docs/acceptance/0019-m4-background-watch-task.md)
- [M4.4 增量索引最终验收决策记录](./docs/decisions/0024-m4-acceptance.md)
- [M4.4 增量索引最终验收记录](./docs/acceptance/0020-m4-acceptance.md)
- [M5 本地 embedding 与混合检索决策记录](./docs/decisions/0025-m5-local-embedding-baseline.md)
- [M5 本地向量混合检索验收记录](./docs/acceptance/0021-m5-hybrid-search-baseline.md)

## 开发约束

每个交付单元都按“调查 → 计划 → 实现 → 测试 → Review → 文档”推进。当前已完成
M4 全部单元和 M5.0–M5.4 本地向量混合检索基线；预训练模型选择、云端同步、多用户、认证、
M6 问答和 Agent 系统仍未开始。
