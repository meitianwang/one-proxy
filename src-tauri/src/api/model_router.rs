// Model router module for aggregation mode
// Handles automatic provider selection based on model name and quota availability

use crate::config::{get_config, ProviderPriority};
use std::collections::HashMap;

/// Known models and which providers support them
/// Format: (model_pattern, vec![provider_names])
static MODEL_PROVIDER_MAP: &[(&str, &[&str])] = &[
    // Claude models
    ("claude-sonnet-4-5", &["kiro", "antigravity", "claude"]),
    ("claude-sonnet-4", &["kiro", "antigravity", "claude"]),
    ("claude-opus-4", &["kiro", "antigravity", "claude"]),
    ("claude-3-5-sonnet", &["kiro", "antigravity", "claude"]),
    ("claude-3-opus", &["kiro", "antigravity", "claude"]),
    ("claude-3-sonnet", &["kiro", "antigravity", "claude"]),
    ("claude-3-haiku", &["kiro", "antigravity", "claude"]),
    // Gemini models
    ("gemini-2.5-pro", &["gemini", "antigravity"]),
    ("gemini-2.5-flash", &["gemini", "antigravity"]),
    ("gemini-2.0-flash", &["gemini", "antigravity"]),
    ("gemini-2.0-pro", &["gemini", "antigravity"]),
    ("gemini-3-pro", &["gemini", "antigravity"]),
    ("gemini-3-flash", &["gemini", "antigravity"]),
    // OpenAI/Codex models
    ("gpt-4o", &["codex"]),
    ("gpt-4.5", &["codex"]),
    ("gpt-5", &["codex"]),
    ("o3", &["codex"]),
    ("o4-mini", &["codex"]),
];

/// Model name aliases: maps (normalized_name, provider) -> actual_model_name
/// This is used to convert normalized names back to provider-specific names
/// Format: (normalized_name, provider, actual_name)
static MODEL_NAME_ALIASES: &[(&str, &str, &str)] = &[
    // Claude Sonnet 4.5 - different providers use different names
    ("claude-sonnet-4-5", "kiro", "claude-sonnet-4.5"),
    ("claude-sonnet-4-5", "antigravity", "claude-sonnet-4-5"),
    ("claude-sonnet-4-5", "claude", "claude-sonnet-4-5-20250514"),
    // Claude Sonnet 4
    ("claude-sonnet-4", "kiro", "claude-sonnet-4"),
    ("claude-sonnet-4", "antigravity", "claude-sonnet-4"),
    ("claude-sonnet-4", "claude", "claude-sonnet-4-20250514"),
    // Claude Opus 4
    ("claude-opus-4", "kiro", "claude-opus-4"),
    ("claude-opus-4", "antigravity", "claude-opus-4"),
    ("claude-opus-4", "claude", "claude-opus-4-20250514"),
    // Claude Haiku 4.5
    ("claude-haiku-4-5", "kiro", "claude-haiku-4.5"),
    ("claude-haiku-4-5", "antigravity", "claude-haiku-4-5"),
    ("claude-haiku-4-5", "claude", "claude-haiku-4-5-20250514"),
    // Gemini models - both use the same name
    ("gemini-2-5-pro", "gemini", "gemini-2.5-pro"),
    ("gemini-2-5-pro", "antigravity", "gemini-2.5-pro"),
    ("gemini-2-5-flash", "gemini", "gemini-2.5-flash"),
    ("gemini-2-5-flash", "antigravity", "gemini-2.5-flash"),
];

/// Result of model resolution
#[derive(Debug, Clone)]
pub enum ResolvedModel {
    /// Model with explicit provider prefix
    Explicit {
        provider: String,
        model: String,
    },
    /// Model resolved through aggregation (no prefix, found provider)
    Aggregated {
        provider: String,
        model: String,
        /// Fallback providers to try if primary fails
        fallbacks: Vec<String>,
    },
    /// No provider found (in provider mode without prefix)
    NoProvider {
        model: String,
    },
}

/// Normalize model names to unify different naming conventions
/// e.g., "claude-sonnet-4.5" and "claude-sonnet-4-5" are the same model
fn normalize_model_name(name: &str) -> String {
    let normalized = name.to_lowercase();
    
    // Normalize version separators: 4.5 -> 4-5, 4_5 -> 4-5
    normalized
        .replace(".", "-")
        .replace("_", "-")
}

/// Get supported providers for a model name
pub fn get_providers_for_model(model: &str) -> Vec<String> {
    // Normalize the input model name first
    let model_normalized = normalize_model_name(model);
    
    // Check exact matches first
    for (pattern, providers) in MODEL_PROVIDER_MAP {
        let pattern_normalized = normalize_model_name(pattern);
        if model_normalized == pattern_normalized || model_normalized.starts_with(&format!("{}-", pattern_normalized)) {
            return providers.iter().map(|s| s.to_string()).collect();
        }
    }
    
    // Check prefix matches
    for (pattern, providers) in MODEL_PROVIDER_MAP {
        let pattern_normalized = normalize_model_name(pattern);
        if model_normalized.starts_with(&pattern_normalized) {
            return providers.iter().map(|s| s.to_string()).collect();
        }
    }
    
    // Try to infer from model name
    if model_normalized.starts_with("claude-") {
        return vec!["kiro".to_string(), "antigravity".to_string(), "claude".to_string()];
    }
    if model_normalized.starts_with("gemini-") {
        return vec!["gemini".to_string(), "antigravity".to_string()];
    }
    if model_normalized.starts_with("gpt-") || model_normalized.starts_with("o3") || model_normalized.starts_with("o4") {
        return vec!["codex".to_string()];
    }
    
    Vec::new()
}

/// Get provider priorities from config, sorted by priority (highest first)
pub fn get_sorted_priorities() -> Vec<ProviderPriority> {
    let config = get_config().unwrap_or_default();
    let mut priorities = config.model_routing.provider_priorities;
    priorities.sort_by(|a, b| b.priority.cmp(&a.priority));
    priorities
}

/// Check if we're in model aggregation mode
pub fn is_aggregation_mode() -> bool {
    let config = get_config().unwrap_or_default();
    config.model_routing.mode == "model"
}

/// Get the provider-specific model name for a normalized model name
/// Returns the provider's preferred model name, or the original name if no mapping exists
pub fn get_provider_model_name(normalized_model: &str, provider: &str) -> String {
    let normalized = normalize_model_name(normalized_model);
    
    // Look for an alias
    for (model, prov, actual) in MODEL_NAME_ALIASES {
        if normalize_model_name(model) == normalized && *prov == provider {
            return actual.to_string();
        }
    }
    
    // No alias found, return the original model name
    normalized_model.to_string()
}

/// Resolve a model name to provider and model, considering routing mode
/// 
/// In provider mode: requires explicit prefix, returns NoProvider if missing
/// In model mode: automatically finds best provider based on quota
pub fn resolve_model(raw_model: &str, explicit_provider: Option<&str>) -> ResolvedModel {
    // If explicit provider was parsed, use it
    if let Some(provider) = explicit_provider {
        return ResolvedModel::Explicit {
            provider: provider.to_string(),
            model: raw_model.to_string(),
        };
    }
    
    let config = get_config().unwrap_or_default();
    
    // In provider mode, require explicit prefix
    if config.model_routing.mode != "model" {
        return ResolvedModel::NoProvider {
            model: raw_model.to_string(),
        };
    }
    
    // In model aggregation mode, find providers
    let available_providers = get_providers_for_model(raw_model);
    if available_providers.is_empty() {
        return ResolvedModel::NoProvider {
            model: raw_model.to_string(),
        };
    }
    
    // Get priorities and filter to enabled providers that support this model
    let priorities = get_sorted_priorities();
    let mut ordered_providers: Vec<String> = Vec::new();
    
    for priority in &priorities {
        if priority.enabled && available_providers.contains(&priority.provider) {
            ordered_providers.push(priority.provider.clone());
        }
    }
    
    // Add any remaining providers not in priorities
    for provider in &available_providers {
        if !ordered_providers.contains(provider) {
            ordered_providers.push(provider.clone());
        }
    }
    
    if ordered_providers.is_empty() {
        return ResolvedModel::NoProvider {
            model: raw_model.to_string(),
        };
    }
    
    let primary = ordered_providers.remove(0);
    // Convert the normalized model name to the provider-specific name
    let actual_model = get_provider_model_name(raw_model, &primary);
    
    ResolvedModel::Aggregated {
        provider: primary,
        model: actual_model,
        fallbacks: ordered_providers,
    }
}

/// Get all unique models across all providers with their supported provider list
pub fn get_aggregated_model_list() -> HashMap<String, Vec<String>> {
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    
    for (model, providers) in MODEL_PROVIDER_MAP {
        result.insert(
            model.to_string(),
            providers.iter().map(|s| s.to_string()).collect(),
        );
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_providers_for_model() {
        let providers = get_providers_for_model("claude-sonnet-4-5");
        assert!(providers.contains(&"kiro".to_string()));
        assert!(providers.contains(&"antigravity".to_string()));
        
        let providers = get_providers_for_model("gemini-2.5-pro");
        assert!(providers.contains(&"gemini".to_string()));
        assert!(providers.contains(&"antigravity".to_string()));
    }
}
