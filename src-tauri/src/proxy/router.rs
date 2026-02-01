// Request router - selects credentials and routes requests

use anyhow::Result;
use crate::config;

pub struct CredentialSelector {
    strategy: RoutingStrategy,
    current_index: std::sync::atomic::AtomicUsize,
}

#[derive(Debug, Clone, Copy)]
pub enum RoutingStrategy {
    RoundRobin,
    FillFirst,
}

impl CredentialSelector {
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self {
            strategy,
            current_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn from_config() -> Self {
        let strategy = config::get_config()
            .map(|c| match c.routing.strategy.as_str() {
                "fill-first" => RoutingStrategy::FillFirst,
                _ => RoutingStrategy::RoundRobin,
            })
            .unwrap_or(RoutingStrategy::RoundRobin);

        Self::new(strategy)
    }
}
