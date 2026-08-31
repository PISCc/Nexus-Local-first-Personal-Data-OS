# Codex Collaboration Guide

## 核心工作方式

固定循环：

``` text
Research -> Plan -> Implement -> Test -> Review -> Document
```

不要使用"帮我实现整个 Nexus"这种大范围 Prompt。

## 每个任务的标准流程

### 1. 调查

先让 Codex 读取： - AGENTS.md - ARCHITECTURE.md - 当前 milestone spec -
相关代码和测试

示例：

``` text
Read AGENTS.md and inspect the current repository.

We are preparing to implement directory scanning.

Do NOT modify code yet.

Analyze:
1. current architecture
2. where the scanner should live
3. data structures needed
4. error cases
5. concurrency considerations
6. test strategy

Then propose an implementation plan with affected files.
```

### 2. 审批计划

你重点检查： - 是否过度设计 - 模块边界是否合理 -
有没有偷偷引入后续需求 - 数据模型是否清晰 - 错误处理是否合理 -
测试是否足够

### 3. 小步实现

示例：

``` text
Proceed with Phase 1 only.

Implement the scanner core without UI integration.

Requirements:
- recursive scanning
- configurable ignored paths
- permission failures must not abort scanning
- no panics in normal error cases
- unit tests required

Do not implement database persistence yet.

Run all relevant tests after implementation.
```

一次只做一个清晰、可验证的切片。

### 4. 强制测试

要求 Codex 明确告诉你： - 实际运行了哪些测试 - 哪些没有运行 - 为什么 -
build/lint/typecheck 状态

不能接受"应该可以工作"。

### 5. 独立 Review

实现后再让 Codex切换角色：

``` text
Review the implementation as if you were a senior engineer reviewing
someone else's pull request.

Look specifically for:
- correctness bugs
- race conditions
- unnecessary complexity
- error handling problems
- performance problems
- missing tests
- architectural violations

Do not modify code yet.

Report findings ordered by severity.
```

确认问题后：

``` text
Fix the confirmed issues from the review.

Do not perform unrelated refactors.

After fixing:
1. run tests
2. run lint
3. run relevant build checks
4. summarize changes
```

## 推荐工作颗粒度

错误： `Implement M1.`

较好： `实现 scanner core。`

更好：
`实现递归目录遍历 + ignore rules + 单元测试；暂时不要持久化数据库。`

每次改动最好做到： - 一个明确目的 - 可以单独测试 - 可以单独 review -
可以安全回滚

## 什么时候必须自己介入

出现以下情况，不要直接接受 Codex： - 修改核心数据模型 - 增加重大依赖 -
改模块边界 - 引入新的并发模型 - 修改数据库 schema - 引入网络上传 -
涉及隐私/安全 - 大规模重构 - Codex 一次准备改几十个文件

## 自我检查

每完成一个模块，你应该能不看 Codex 回答： 1. 模块负责什么？ 2.
输入输出是什么？ 3. 依赖谁？ 4. 数据在哪里？ 5. 失败怎么办？ 6.
为什么这么设计？ 7. 性能瓶颈在哪里？ 8. 测试覆盖了什么？

答不出来时，暂停加功能，先读懂代码。

## ADR

重大架构决定写入 `docs/decisions/`：

-   Context
-   Decision
-   Alternatives
-   Consequences

让未来的 Codex 能知道"为什么当时这么决定"。

## 建议的人机分工

大致把精力放在： - 30% 设计/讨论 - 50% 实现 - 20% Review/Test

目标不是让 AI 写最多代码，而是让项目在高速开发下仍保持可理解。
