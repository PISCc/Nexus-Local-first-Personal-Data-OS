# M3.5 搜索质量评估验收记录

## 目标

用固定、可提交、无个人数据的本地语料评估 M3.3 lexical ranking，记录查询延迟、索引
占用和 Recall，并与 M3.2 的文档 ID 顺序基线比较。

## 实现范围

- `crates/nexus-db/tests/search_quality.rs`：10 条固定 `DocumentRecord`、9 个查询和人工
  相关文档 ID 标注。
- 每个查询执行 15 次，输出中位延迟和 p95 延迟。
- 计算 Recall@3、macro Recall@3 和 Top-1 命中数。
- 记录 SQLite 文件总大小和 FTS5 segment block 字节数。
- 以同一候选集合的文档 ID 升序作为 M3.2 基线；不修改生产搜索算法。

## 实际执行结果

执行命令：

```text
cargo test -p nexus-db --test search_quality -- --nocapture
```

运行环境：Windows MSVC，Debug test profile，日期 2026-09-01。延迟是本机观测值，不是
跨机器性能承诺。

| 指标 | 当前 BM25 | M3.2 ID 顺序基线 |
| --- | ---: | ---: |
| 固定文档数 | 10 | 10 |
| 查询数 | 9 | 9 |
| Top-k | 3 | 3 |
| SQLite 文件大小 | 65,536 bytes | 65,536 bytes |
| FTS5 segment bytes | 1,675 bytes | 同一索引 |
| macro Recall@3 | 0.9722 | 0.9444 |
| Top-1 命中 | 9 / 9 | 9 / 9 |

当前实现相对基线的 macro Recall@3 提升为 `+0.0278`。测试还断言每个查询的当前 Recall@3
不低于基线；本次通过。

## 分查询结果

| 查询 | 当前 Top-3 | 基线 Top-3 | 当前 Recall@3 | 基线 Recall@3 | median µs | p95 µs |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| `plan` | `doc-release-plan`, `doc-project-plan`, `doc-travel-plan` | `doc-meeting-notes`, `doc-parser-changelog`, `doc-project-plan` | 0.7500 | 0.5000 | 797 | 1,311 |
| `project plan` | `doc-project-plan`, `doc-release-plan`, `doc-meeting-notes` | `doc-meeting-notes`, `doc-project-plan`, `doc-release-plan` | 1.0000 | 1.0000 | 704 | 1,123 |
| `"quarterly plan"` | `doc-project-plan` | `doc-project-plan` | 1.0000 | 1.0000 | 703 | 988 |
| `backup restore` | `doc-backup-runbook` | `doc-backup-runbook` | 1.0000 | 1.0000 | 732 | 1,347 |
| `local data` | `doc-privacy-notes` | `doc-privacy-notes` | 1.0000 | 1.0000 | 654 | 1,270 |
| `schema FTS` | `doc-database-migration` | `doc-database-migration` | 1.0000 | 1.0000 | 642 | 1,025 |
| `ranking recall` | `doc-search-research` | `doc-search-research` | 1.0000 | 1.0000 | 576 | 712 |
| `"meeting notes"` | `doc-meeting-notes` | `doc-meeting-notes` | 1.0000 | 1.0000 | 590 | 789 |
| `parser markdown` | `doc-code-index` | `doc-code-index` | 1.0000 | 1.0000 | 600 | 998 |

## 验收结论

- 固定语料、查询集和相关性标注已进入版本库，可重复执行。
- 当前 BM25 在该语料上没有低于 M3.2 基线，并在 `plan` 查询上改善了 Top-3 相关结果比例。
- 查询延迟和存储占用已有记录，但样本规模不足以决定 tokenizer、权重或搜索引擎迁移。
- M3.5 完成；M3.6 已在此评估基础上补齐初始正文索引和端到端验收。中文、真实脱敏
  语料和大规模性能仍需后续补充。

## 边界

本记录不代表真实用户数据的搜索质量，不采集或上传文件，不接入 UI telemetry，不实现
增量索引、语义搜索或 LLM reranking。
