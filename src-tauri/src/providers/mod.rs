//! 平台适配器注册表。
//!
//! 新增一个平台 = 在这里加一个 `Provider` 实现 + 在 `all()` 里登记，
//! 其余各层（账号池、调度、UI）不需要改动。

pub mod miaoshou;
pub mod minimax;
pub mod qoder;
pub mod workbuddy;

use std::sync::Arc;

use crate::core::provider::Provider;

/// 全部已注册的平台。
#[allow(dead_code)] // 调度器/命令层可按 id 动态取适配器
pub fn all() -> Vec<Arc<dyn Provider>> {
    vec![
        Arc::new(minimax::MiniMaxProvider::new()),
        Arc::new(qoder::QoderProvider::new()),
        Arc::new(miaoshou::MiaoshouProvider::new()),
        Arc::new(workbuddy::WorkBuddyProvider::new()),
    ]
}

/// 按 id 取平台。
#[allow(dead_code)] // 调度器/命令层可按 id 动态取适配器
pub fn find(id: &str) -> Option<Arc<dyn Provider>> {
    all().into_iter().find(|p| p.id() == id)
}
