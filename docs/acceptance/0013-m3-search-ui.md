# M3.4 搜索 UI 验收记录

## 目标

把 M3.2/M3.3 的本地全文查询接入桌面应用，提供可理解的查询输入、结果展示、状态反馈和
原始文件定位，同时不让 React 直接拥有数据库或索引逻辑。

## 本单元范围

- 默认全文搜索页面和侧栏页面切换。
- 查询提交、loading、空结果、错误和 UI 取消状态。
- 显示标题、路径、文件类型、修改日期、relevance 和 snippet。
- 通过文档 ID 定位原始文件。
- 保留 M1 文件索引和手动重扫闭环。

明确不包含：分页、GUI filter chips、输入即搜索、可取消 SQLite 线程、搜索质量评估、
增量索引、语义搜索、LLM 和云端服务。

## 验收证据

| 检查项 | 证据 |
| --- | --- |
| 默认显示全文搜索 | `SearchView` 空闲状态测试和 `App` 初始界面测试 |
| M1 页面仍可进入 | `App` 重扫测试通过侧栏切换到“文件索引” |
| 提交查询并展示结果 | `提交查询并显示本地结果和匹配片段` |
| 结果包含可追溯字段 | 测试断言路径、类型、日期、相关性和 snippet |
| 空结果和安全错误 | `显示空结果和后端安全错误` |
| 取消后忽略晚到结果 | `取消查询后忽略晚到的结果` |
| 原始文件定位命令参数正确 | `通过文档 ID 请求定位原始文件` |
| 后端 DTO 不复制正文 | `maps_search_results_without_copying_full_document_body` |
| 无新增第三方依赖 | `nexus-db` 仍只使用 `rusqlite`；桌面仅增加 workspace 内部 `nexus-db` 依赖 |

## 结果

M3.4 已实现本地全文搜索桌面入口，搜索结果和失败状态均保持可追溯、可解释和本地化。
M1 文件索引页面仍可从侧栏访问。下一步为 M3.5 搜索质量评估。

## 实际执行的检查

- `pnpm format`：通过（Prettier 和 `cargo fmt --check`）。
- `pnpm lint`：通过（ESLint 和 workspace Clippy，`-D warnings`）。
- `pnpm typecheck`：通过（前端 `tsc -b` 和 workspace `cargo check`）。
- `pnpm test`：通过（前端 11 项，Rust workspace 67 项）。
- `cargo test --workspace --locked`：通过（core 36 项、db 26 项、desktop 5 项）。
- `pnpm build`：通过（Vite 前端和 workspace Rust 构建）。
- `git diff --check`：通过；仅有 Git 关于工作区 LF/CRLF 转换的提示。

构建期间 Windows Rust 增量编译目录曾报告一次 `拒绝访问` 清理提示，但构建以退出码 0
完成；该提示属于本机 `target` 目录复用状态，不是应用代码错误。
