# M5.0–M5.4 本地向量混合检索基线验收记录

- 日期：2026-09-02
- 状态：工程基线完成
- 范围：provider、版本化存储、混合检索、索引生命周期和离线评估

## 验收结论

M5 的最小工程链路已在本地完成并通过测试：文档标题和正文可以生成确定性本地向量，
向量按模型 ID/版本保存，BM25 与向量结果可以稳定融合，初始索引和 M4 增量更新可以
维护向量，缺少向量时仍保留 lexical 搜索。

这里的默认 provider 是确定性特征向量基线，不是预训练语言模型。它证明了工程边界和
失败行为，不证明跨词汇、跨语言或复杂个人语料的通用语义理解能力。M6 尚未开始。

## 范围与验收项

| 验收项 | 结果 |
| --- | --- |
| 本地 provider 可重复生成固定维度向量 | 通过；无网络、无模型下载 |
| 向量只使用标题和正文 | 通过；路径、文件名和时间字段不进入输入 |
| 模型 ID/版本/维度登记和冲突保护 | 通过；schema 5 已加入版本化表 |
| 输入指纹和文档更新/删除清理 | 通过；旧向量不会继续对应新正文 |
| 完整 embedding 建立使用有界批次和取消 | 通过；使用文档 ID 游标 |
| M4 增量更新刷新受影响文档 | 通过；删除和解析失败保持安全降级 |
| BM25 与向量候选融合 | 通过；使用固定 RRF，lexical 始终保留 |
| 元数据过滤作用于两条检索路径 | 通过 |
| 缺失模型、缺失向量或损坏向量的回退 | 通过；不阻断 lexical 搜索 |
| 前端显示可选语义/融合信息 | 通过；不增加 UI 数据库职责 |
| reranking 是否必要 | 暂不加入；当前评估没有质量收益且增加开销 |

## 固定质量评估

评估复用 M3 的 10 条合成文档、9 个查询和人工相关性标注，不读取真实用户资料。每个
查询执行 lexical 和 hybrid 检索，比较 Recall@3 与 Top-1：

```text
macro_recall_at_3_lexical = 0.9722
macro_recall_at_3_hybrid  = 0.9722
top_1_hits_lexical        = 9/9
top_1_hits_hybrid         = 9/9
```

在当前小型 Debug 运行中，hybrid 通常比 lexical 有更高计算开销；这组结果不足以代表
中文、复杂格式、大型个人资料或真正预训练模型的效果。

## 实际验证命令

以下检查均在本地工作区执行并通过：

- `pnpm format`
- `pnpm typecheck`
- `pnpm lint`
- `pnpm test`
- `pnpm build`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo check --workspace --all-targets`
- `git diff --check`

M5 定向验证还包括：

- `cargo test -p nexus-core embedding`
- `cargo test -p nexus-core semantic`
- `cargo test -p nexus-core --test semantic_quality -- --nocapture`
- `cargo test -p nexus-db search::tests`

## 已知限制与后续门槛

- 当前 provider 不是预训练语言模型，不能宣称已经完成通用语义搜索。
- 当前质量评估使用小型合成英文语料；需要真实脱敏语料和更多标注后才能决定模型或权重。
- 向量查询当前在本地 SQLite 中读取候选，规模化性能仍需基准数据支持；不因假设提前引入
  专用向量数据库。
- 预训练模型、云端 API、M6 问答、引用生成和 Agent 层均不在本次范围内。
