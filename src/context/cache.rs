use alloy::primitives::{Address, B256};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::preprocessing::PreprocessedContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub max_entries: usize,
    pub ttl_seconds: u64,
    pub enable_metrics: bool,
    pub compression_threshold: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            ttl_seconds: 300, // 5 minutes
            enable_metrics: true,
            compression_threshold: 1024,
        }
    }
}

#[derive(Debug, Clone)]
struct CachedEntry {
    context: PreprocessedContext,
    timestamp: Instant,
    access_count: u64,
}

#[derive(Debug, Default)]
pub struct CacheMetrics {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub total_size_bytes: usize,
}

pub struct ContextCache {
    config: CacheConfig,
    cache: Arc<RwLock<LruCache<CacheKey, CachedEntry>>>,
    metrics: Arc<RwLock<CacheMetrics>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    transaction_hash: B256,
    agent_address: Address,
}

impl ContextCache {
    pub fn new(config: CacheConfig) -> Self {
        let max_entries = NonZeroUsize::new(config.max_entries).unwrap_or(NonZeroUsize::new(1).unwrap());
        
        Self {
            config,
            cache: Arc::new(RwLock::new(LruCache::new(max_entries))),
            metrics: Arc::new(RwLock::new(CacheMetrics::default())),
        }
    }

    pub async fn get(&self, transaction_hash: B256, agent_address: Address) -> Option<PreprocessedContext> {
        let key = CacheKey { transaction_hash, agent_address };
        let mut cache = self.cache.write().await;
        
        if let Some(entry) = cache.get_mut(&key) {
            // Check TTL
            if entry.timestamp.elapsed() > Duration::from_secs(self.config.ttl_seconds) {
                cache.pop(&key);
                if self.config.enable_metrics {
                    let mut metrics = self.metrics.write().await;
                    metrics.misses += 1;
                    metrics.evictions += 1;
                }
                return None;
            }
            
            entry.access_count += 1;
            let context = entry.context.clone();
            
            if self.config.enable_metrics {
                let mut metrics = self.metrics.write().await;
                metrics.hits += 1;
            }
            
            Some(context)
        } else {
            if self.config.enable_metrics {
                let mut metrics = self.metrics.write().await;
                metrics.misses += 1;
            }
            None
        }
    }

    pub async fn put(&self, context: PreprocessedContext) {
        let key = CacheKey {
            transaction_hash: context.transaction_hash,
            agent_address: context.agent_address,
        };
        
        let entry = CachedEntry {
            context,
            timestamp: Instant::now(),
            access_count: 1,
        };
        
        let mut cache = self.cache.write().await;
        
        // Check if we're at capacity and will evict
        if cache.len() >= cache.cap().get() && !cache.contains(&key) {
            if self.config.enable_metrics {
                let mut metrics = self.metrics.write().await;
                metrics.evictions += 1;
            }
        }
        
        cache.put(key, entry);
        
        if self.config.enable_metrics {
            self.update_size_metrics(&cache).await;
        }
    }

    pub async fn get_batch(
        &self,
        requests: Vec<(B256, Address)>,
    ) -> Vec<Option<PreprocessedContext>> {
        let mut results = Vec::with_capacity(requests.len());
        
        for (tx_hash, agent_addr) in requests {
            results.push(self.get(tx_hash, agent_addr).await);
        }
        
        results
    }

    pub async fn put_batch(&self, contexts: Vec<PreprocessedContext>) {
        for context in contexts {
            self.put(context).await;
        }
    }

    pub async fn invalidate(&self, transaction_hash: B256, agent_address: Address) {
        let key = CacheKey { transaction_hash, agent_address };
        let mut cache = self.cache.write().await;
        cache.pop(&key);
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        
        if self.config.enable_metrics {
            let mut metrics = self.metrics.write().await;
            *metrics = CacheMetrics::default();
        }
    }

    pub async fn get_metrics(&self) -> CacheMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        let ttl = Duration::from_secs(self.config.ttl_seconds);
        let mut expired_keys = Vec::new();
        
        // Collect expired keys
        for (key, entry) in cache.iter() {
            if entry.timestamp.elapsed() > ttl {
                expired_keys.push(key.clone());
            }
        }
        
        // Remove expired entries
        for key in expired_keys {
            cache.pop(&key);
            if self.config.enable_metrics {
                let mut metrics = self.metrics.write().await;
                metrics.evictions += 1;
            }
        }
        
        if self.config.enable_metrics {
            self.update_size_metrics(&cache).await;
        }
    }

    pub async fn get_hot_contexts(&self, threshold: u64) -> Vec<PreprocessedContext> {
        let cache = self.cache.read().await;
        let mut hot_contexts = Vec::new();
        
        for (_, entry) in cache.iter() {
            if entry.access_count >= threshold {
                hot_contexts.push(entry.context.clone());
            }
        }
        
        // Sort by access count descending
        hot_contexts.sort_by(|a, b| {
            let cache_read = self.cache.blocking_read();
            let key_a = CacheKey {
                transaction_hash: a.transaction_hash,
                agent_address: a.agent_address,
            };
            let key_b = CacheKey {
                transaction_hash: b.transaction_hash,
                agent_address: b.agent_address,
            };
            
            let count_a = cache_read.peek(&key_a).map(|e| e.access_count).unwrap_or(0);
            let count_b = cache_read.peek(&key_b).map(|e| e.access_count).unwrap_or(0);
            
            count_b.cmp(&count_a)
        });
        
        hot_contexts
    }

    async fn update_size_metrics(&self, cache: &LruCache<CacheKey, CachedEntry>) {
        let mut total_size = 0;
        
        for (_, entry) in cache.iter() {
            // Estimate size based on serialized data
            total_size += std::mem::size_of_val(&entry.context.transaction_hash);
            total_size += std::mem::size_of_val(&entry.context.agent_address);
            total_size += entry.context.extracted_features.len() * 32; // Rough estimate
            total_size += entry.context.semantic_tags.iter().map(|s| s.len()).sum::<usize>();
            
            if let Some(compressed) = &entry.context.compressed_data {
                total_size += compressed.len();
            }
        }
        
        let mut metrics = self.metrics.write().await;
        metrics.total_size_bytes = total_size;
    }

    pub async fn resize(&self, new_size: usize) -> Result<(), String> {
        let new_cap = NonZeroUsize::new(new_size)
            .ok_or_else(|| "Cache size must be greater than zero".to_string())?;
        
        let mut cache = self.cache.write().await;
        cache.resize(new_cap);
        
        Ok(())
    }
}

// Background task for periodic cleanup
pub async fn start_cache_maintenance(cache: Arc<ContextCache>, interval_seconds: u64) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_seconds));
    
    loop {
        interval.tick().await;
        cache.cleanup_expired().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::types::AgentAction;
    use alloy::primitives::U256;

    #[tokio::test]
    async fn test_cache_basic_operations() {
        let config = CacheConfig {
            max_entries: 10,
            ttl_seconds: 60,
            enable_metrics: true,
            compression_threshold: 1024,
        };
        
        let cache = ContextCache::new(config);
        
        let context = PreprocessedContext {
            transaction_hash: B256::from([1u8; 32]),
            agent_address: Address::from([2u8; 20]),
            action_type: AgentAction::Transfer {
                to: Address::from([3u8; 20]),
                amount: U256::from(1000),
            },
            extracted_features: Default::default(),
            semantic_tags: vec!["test".to_string()],
            priority_score: 1.0,
            timestamp: 12345,
            compressed_data: None,
        };
        
        // Test put and get
        cache.put(context.clone()).await;
        let retrieved = cache.get(context.transaction_hash, context.agent_address).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().transaction_hash, context.transaction_hash);
        
        // Test metrics
        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.misses, 0);
    }

    #[tokio::test]
    async fn test_cache_ttl() {
        let config = CacheConfig {
            max_entries: 10,
            ttl_seconds: 0, // Immediate expiry
            enable_metrics: true,
            compression_threshold: 1024,
        };
        
        let cache = ContextCache::new(config);
        
        let context = PreprocessedContext {
            transaction_hash: B256::from([1u8; 32]),
            agent_address: Address::from([2u8; 20]),
            action_type: AgentAction::Transfer {
                to: Address::from([3u8; 20]),
                amount: U256::from(1000),
            },
            extracted_features: Default::default(),
            semantic_tags: vec!["test".to_string()],
            priority_score: 1.0,
            timestamp: 12345,
            compressed_data: None,
        };
        
        cache.put(context.clone()).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        let retrieved = cache.get(context.transaction_hash, context.agent_address).await;
        assert!(retrieved.is_none());
        
        let metrics = cache.get_metrics().await;
        assert_eq!(metrics.misses, 1);
        assert_eq!(metrics.evictions, 1);
    }
}