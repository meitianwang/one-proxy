// Proxy Config - Thinking Budget 配置
// 从 Antigravity-Manager 移植简化版

use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};

static GLOBAL_THINKING_BUDGET_CONFIG: OnceLock<RwLock<ThinkingBudgetConfig>> = OnceLock::new();

/// 获取当前 Thinking Budget 配置
pub fn get_thinking_budget_config() -> ThinkingBudgetConfig {
    GLOBAL_THINKING_BUDGET_CONFIG
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|cfg| cfg.clone())
        .unwrap_or_default()
}

/// 更新全局 Thinking Budget 配置
pub fn update_thinking_budget_config(config: ThinkingBudgetConfig) {
    if let Some(lock) = GLOBAL_THINKING_BUDGET_CONFIG.get() {
        if let Ok(mut cfg) = lock.write() {
            *cfg = config.clone();
            tracing::info!(
                "[Thinking-Budget] Global config updated: mode={:?}, custom_value={}",
                config.mode,
                config.custom_value
            );
        }
    } else {
        let _ = GLOBAL_THINKING_BUDGET_CONFIG.set(RwLock::new(config.clone()));
        tracing::info!(
            "[Thinking-Budget] Global config initialized: mode={:?}, custom_value={}",
            config.mode,
            config.custom_value
        );
    }
}

/// Thinking Budget 模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingBudgetMode {
    /// 自动限制：对特定模型（Flash/Thinking）应用 24576 上限
    Auto,
    /// 透传：完全使用调用方传入的值
    Passthrough,
    /// 自定义：使用用户设定的固定值
    Custom,
}

impl Default for ThinkingBudgetMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// Thinking Budget 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingBudgetConfig {
    #[serde(default)]
    pub mode: ThinkingBudgetMode,
    #[serde(default = "default_thinking_budget_custom_value")]
    pub custom_value: u32,
}

impl Default for ThinkingBudgetConfig {
    fn default() -> Self {
        Self {
            mode: ThinkingBudgetMode::Auto,
            custom_value: default_thinking_budget_custom_value(),
        }
    }
}

fn default_thinking_budget_custom_value() -> u32 {
    24576
}
