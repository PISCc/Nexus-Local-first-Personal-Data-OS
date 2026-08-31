//! Nexus 本地优先核心边界。
//!
//! M0.3 只固定 core 与 db 的依赖方向，不提前建立业务接口或运行时服务。
//! 后续初始化和错误传播行为在 M0.4、M0.5 中按最小单元加入。

#![forbid(unsafe_code)]

// 让 core → db 的边界在没有运行时 API 的阶段也由编译器验证。
use nexus_db as _;
