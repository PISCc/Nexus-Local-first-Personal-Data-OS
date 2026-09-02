# M3.6 M3 整体验收记录

## 目标

验证 M3 不只是测试数据可以搜索，而是从真实本地目录扫描、内容解析、canonical 文档写入、
FTS 同步到桌面搜索结果的最小端到端闭环已经存在；同时确认关键词、短语、过滤器、snippet、
原文件追溯和失败边界没有越过本地优先约束。

## 本单元交付

- `nexus-core::index_directory` / `index_directory_with_control` 将 M1 流式扫描、M2
  解析器和 M3 `documents`/FTS 接通。
- Tauri `start_rescan` 增加 `indexContent` 开关；桌面文件索引页默认用初始索引模式。
- 进度和完成事件增加 `documentsSucceeded`、`documentsFailed`、`documentsSkipped`。
- 支持 txt、md、py、rs、js、ts、java、cpp、json、html/htm、docx、pdf；单文件解析失败
  或不支持格式不会终止全局任务。
- 修复正文命中时未命中标题片段遮蔽正文 snippet 的问题，并加入正文-only 回归测试。

## 自动化证据

核心端到端 fixture 位于 `crates/nexus-core/src/lib.rs` 测试模块：

- 真实临时目录包含 Markdown、JSON 和不支持格式；验证正文进入 `documents`，FTS 查询
  返回结果，snippet 保留命中标记，文件元数据过滤仍可用。
- 另一个 fixture 包含有效 UTF-8 和无效 UTF-8 文本；验证元数据成功写入，单文件解析
  失败计数并继续处理后续文件。
- `nexus-db` 搜索测试验证正文-only 命中优先返回带命中标记的片段。
- Tauri/React 测试验证 `indexContent` 请求、扩展后的事件 payload、完成统计和取消状态。

## 最终检查结果

| 检查 | 结果 |
| --- | --- |
| `pnpm format` | 通过；Prettier 和 Rustfmt 均通过 |
| `pnpm lint` | 通过；ESLint 和 workspace Clippy `-D warnings` 均通过 |
| `pnpm typecheck` | 通过；TypeScript 和 workspace Cargo check 均通过 |
| `pnpm test` | 通过；前端 11 项，Rust core 40 项、db 26 项、质量评估 1 项、桌面 5 项，doctest 0 项 |
| `pnpm build` | 通过；Vite 和 workspace Cargo build 均通过 |
| `git diff --check` | 通过 |
| `cargo tree` | 未增加 M3.6 专用依赖；继续使用现有解析器和 `rusqlite` |

`pnpm build` 在 Windows 增量编译目录清理时输出过一次系统 `拒绝访问` 提示，但构建进程
最终以成功状态结束；该提示不影响产物生成，后续可由工具链自行重建增量目录。

## 查看成品

在仓库根目录执行：

```text
pnpm dev
```

桌面应用打开后：

1. 进入左侧“文件索引”；
2. 输入一个本地目录的完整路径；
3. 点击“开始索引”，等待“正文写入”统计增加；
4. 进入左侧“全文搜索”，输入正文中的关键词或双引号短语；
5. 检查结果中的 snippet、路径和“打开原文件”行为。

浏览器预览 `pnpm dev:web` 只用于查看静态界面，会诚实显示 Tauri 核心不可用，不能代替
桌面端本地数据库验收。

## 边界和错误行为

- 核心 `rescan_directory_with_control` 仍是 metadata-only；桌面 UI 显式打开 `indexContent`。
- 未支持扩展名进入 `documentsSkipped`；损坏、无权限、无效编码和资源超限进入
  `documentsFailed`，不会让其他文件停止。
- 数据库写入或连接故障返回任务级失败，不伪造完成统计。
- 取消后，已提交的单文件正文索引可能保留；未完成元数据重扫不会应用，canonical 文档
  与 FTS 仍保持单条写入的一致性。
- 不实现修改/删除/移动后的自动更新、陈旧正文清理、watcher、语义检索、分页或云端同步；
  这些属于 M4 及以后。

## M3 验收结论

M3.0–M3.6 的数据模型、FTS、查询、ranking、snippet、搜索 UI、质量评估和初始正文索引
已经形成可运行的本地闭环。M3 完成；下一步为 M4 增量索引，重点是文件变化、陈旧记录、
重复事件、取消和关闭恢复。
