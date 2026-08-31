# ADR-0001：M0 工程基础

- 状态：已接受
- 日期：2026-08-31
- 范围：Nexus M0 工程基础

## 背景

Nexus 的长期目标是本地优先的个人数据操作系统。M0 需要先建立可重复安装、
可测试、能够解释失败的桌面和 Rust 基础，同时避免提前实现文件扫描、正文解析、
搜索或人工智能功能。

## 决策

### 1. 使用 pnpm workspace 和 Cargo workspace

前端放在 `apps/desktop`，Rust 工作区包含 Tauri 桌面壳层、`nexus-core` 和
`nexus-db`。依赖方向固定为：

```text
nexus-desktop → nexus-core → nexus-db
```

核心层和数据库层不依赖 Tauri，界面不直接承担数据库或索引业务逻辑。

### 2. 使用 Tauri 2、React、TypeScript 和 Rust

Tauri 提供桌面壳层，React + TypeScript 提供界面，Rust 承担本地核心和数据库
边界。Tauri CLI 作为前端开发依赖进入锁文件，开发者不需要全局安装 CLI。

### 3. 使用 rusqlite bundled 提供 SQLite

`nexus-db` 使用 `rusqlite` 的 `bundled` 功能。数据库初始化由调用方传入路径，
通过 `PRAGMA user_version` 和事务迁移管理 schema。当前只创建
`nexus_metadata` 表，不提前创建文件索引或正文模型。

### 4. 使用应用数据目录保存本地数据库

Tauri 桌面壳层负责解析应用数据目录、创建目录，并将数据库文件命名为
`nexus.sqlite3`。数据库层不绑定平台路径，便于隔离测试和后续平台适配。

### 5. 使用 stderr 日志和安全错误边界

启动日志使用 `tracing` 输出到 stderr，不写持久化日志。日志只包含固定消息和
错误分类；核心错误保留进程内错误链，但不会把完整用户路径、文件名、内容或
搜索词传入日志或界面。数据库初始化失败时，应用进入 `degraded` 状态，并通过
`get_startup_status` 命令返回非敏感中文说明。

### 6. 将 CI 按平台职责拆分

GitHub Actions 使用三个检查面：

- Ubuntu 执行前端格式、lint、类型检查、测试和构建。
- Ubuntu 执行 `nexus-core` 与 `nexus-db` 的 Rust 格式、Clippy、测试和构建。
- Windows 执行前端构建，并至少编译一次 Tauri 桌面 crate。

这样可以避免 Linux CI 被 Windows 桌面运行时依赖阻塞，同时保留 Windows Tauri
编译门槛。

## 备选方案

- 不采用 `sqlx + Tokio`：M0 只需要同步的本地初始化，异步运行时会扩大依赖和
  并发边界。
- 不采用持久化日志：M0 先保证失败可观察，避免在没有诊断需求前管理日志文件
  生命周期和隐私清理。
- 不在 M0 创建 parser、index、search 空 crate：等真实需求出现后再建立边界。
- 不在 Ubuntu CI 编译 Tauri 桌面 crate：桌面编译由 Windows 专项任务负责。

## 后果

正面结果是依赖关系清晰、核心可在无 UI 环境下测试、数据库错误可分类、开发者
不依赖全局 Tauri 工具。代价是 M0 暂不持有长期数据库运行时连接，后续 M1 在
需要扫描和持久化时必须单独设计连接生命周期、批处理和取消策略，并通过新的
局部决策记录确认。

## 复审条件

如果后续需要持久化日志、异步数据库访问、跨平台桌面发布、远程同步或 AI 上传，
必须重新评估本 ADR，并单独记录隐私、依赖和运行时影响。
