// Signature Cache - 三层签名缓存系统
// 从 Antigravity-Manager 移植

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

const SIGNATURE_TTL: Duration = Duration::from_secs(2 * 60 * 60); // 2 hours
const MIN_SIGNATURE_LENGTH: usize = 50;
const TOOL_CACHE_LIMIT: usize = 500;
const FAMILY_CACHE_LIMIT: usize = 200;
const SESSION_CACHE_LIMIT: usize = 1000;

#[derive(Clone, Debug)]
struct CacheEntry<T> {
    data: T,
    timestamp: SystemTime,
}

#[derive(Clone, Debug)]
struct SessionSignatureEntry {
    signature: String,
    message_count: usize,
}

impl<T> CacheEntry<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            timestamp: SystemTime::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.timestamp.elapsed().unwrap_or(Duration::ZERO) > SIGNATURE_TTL
    }
}

/// Triple-layer signature cache
pub struct SignatureCache {
    tool_signatures: Mutex<HashMap<String, CacheEntry<String>>>,
    thinking_families: Mutex<HashMap<String, CacheEntry<String>>>,
    session_signatures: Mutex<HashMap<String, CacheEntry<SessionSignatureEntry>>>,
}

impl SignatureCache {
    fn new() -> Self {
        Self {
            tool_signatures: Mutex::new(HashMap::new()),
            thinking_families: Mutex::new(HashMap::new()),
            session_signatures: Mutex::new(HashMap::new()),
        }
    }

    pub fn global() -> &'static SignatureCache {
        static INSTANCE: OnceLock<SignatureCache> = OnceLock::new();
        INSTANCE.get_or_init(SignatureCache::new)
    }

    pub fn cache_tool_signature(&self, tool_use_id: &str, signature: String) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }
        
        if let Ok(mut cache) = self.tool_signatures.lock() {
            tracing::debug!("[SignatureCache] Caching tool signature for id: {}", tool_use_id);
            cache.insert(tool_use_id.to_string(), CacheEntry::new(signature));
            
            if cache.len() > TOOL_CACHE_LIMIT {
                cache.retain(|_, v| !v.is_expired());
            }
        }
    }

    pub fn get_tool_signature(&self, tool_use_id: &str) -> Option<String> {
        if let Ok(cache) = self.tool_signatures.lock() {
            if let Some(entry) = cache.get(tool_use_id) {
                if !entry.is_expired() {
                    return Some(entry.data.clone());
                }
            }
        }
        None
    }

    pub fn cache_thinking_family(&self, signature: String, family: String) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }

        if let Ok(mut cache) = self.thinking_families.lock() {
            cache.insert(signature, CacheEntry::new(family));
            
            if cache.len() > FAMILY_CACHE_LIMIT {
                cache.retain(|_, v| !v.is_expired());
            }
        }
    }

    pub fn get_signature_family(&self, signature: &str) -> Option<String> {
        if let Ok(cache) = self.thinking_families.lock() {
            if let Some(entry) = cache.get(signature) {
                if !entry.is_expired() {
                    return Some(entry.data.clone());
                }
            }
        }
        None
    }

    pub fn cache_session_signature(&self, session_id: &str, signature: String, message_count: usize) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }

        if let Ok(mut cache) = self.session_signatures.lock() {
            let should_store = match cache.get(session_id) {
                None => true,
                Some(existing) => {
                    if existing.is_expired() {
                        true
                    } else if message_count < existing.data.message_count {
                        true // Rewind detected
                    } else if message_count == existing.data.message_count {
                        signature.len() > existing.data.signature.len()
                    } else {
                        true
                    }
                }
            };

            if should_store {
                cache.insert(
                    session_id.to_string(), 
                    CacheEntry::new(SessionSignatureEntry { 
                        signature, 
                        message_count 
                    })
                );
            }

            if cache.len() > SESSION_CACHE_LIMIT {
                cache.retain(|_, v| !v.is_expired());
            }
        }
    }

    pub fn get_session_signature(&self, session_id: &str) -> Option<String> {
        if let Ok(cache) = self.session_signatures.lock() {
            if let Some(entry) = cache.get(session_id) {
                if !entry.is_expired() {
                    return Some(entry.data.signature.clone());
                }
            }
        }
        None
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        if let Ok(mut cache) = self.tool_signatures.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.thinking_families.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.session_signatures.lock() {
            cache.clear();
        }
    }
}
