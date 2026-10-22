//! 聚合签到 —— 核心领域模型。
//!
//! 分层：
//!   * `provider`  —— 平台适配器抽象（每个 AI 编程工具一个实现）
//!   * `store`     —— 账号池持久化（凭据用 Windows DPAPI 加密）
//!   * `config`    —— 应用配置（调度时间窗等）
//!   * `logging`   —— 签到流水
//!   * `scheduler` —— 时间窗 + 每账号随机分钟偏移的自动调度

pub mod config;
pub mod dpapi;
pub mod logging;
pub mod paths;
pub mod provider;
pub mod scheduler;
pub mod store;
pub mod tz;
