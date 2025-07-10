use alloy::primitives::{Address, B256};
use dashmap::DashMap;
use eyre::Result;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub max_size_mb: usize,
    pub default_ttl: Duration,
    pub cleanup_interval: Duration,
    pub enable_compression: bool,
    pub compression_threshold: usize,
    pub enable_persistence: bool,
    pub persistence_path: Option<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 256,
            default_ttl: Duration::from_secs(300),
            cleanup_interval: Duration::from_secs(60),
            enable_compression: true,
            compression_threshold: 1024,
            enable_persistence: false,
            persistence_path: None,
        }
    }
}

#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    created_at: Instant,
    last_accessed: Instant,
    ttl: Duration,
    access_count: u64,
    size_bytes: usize,
    is_compressed: bool,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Duration, size_bytes: usize) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            ttl,
            access_count: 0,
            size_bytes,
            is_compressed: false,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }

    fn touch(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }
}

#[derive(Debug)]
pub struct MultiLevelCache<K, V> 
where 
    K: Clone + Eq + Hash + Send + Sync,
    V: Clone + Send + Sync,
{
    config: CacheConfig,
    l1_cache: Arc<RwLock<LruCache<K, CacheEntry<V>>>>,
    l2_cache: Arc<DashMap<K, CacheEntry<V>>>,
    hot_keys: Arc<RwLock<HashMap<K, u64>>>,
    cache_stats: Arc<RwLock<CacheStats>>,
    total_size_bytes: Arc<RwLock<usize>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub l1_hits: u64,
    pub l1_misses: u64,
    pub l2_hits: u64,
    pub l2_misses: u64,
    pub evictions: u64,
    pub compressions: u64,
    pub total_size_bytes: usize,
    pub entry_count: usize,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total_hits = self.l1_hits + self.l2_hits;
        let total_requests = total_hits + self.l1_misses + self.l2_misses;
        
        if total_requests > 0 {
            total_hits as f64 / total_requests as f64
        } else {
            0.0
        }
    }

    pub fn l1_hit_rate(&self) -> f64 {
        let total_l1_requests = self.l1_hits + self.l1_misses;
        if total_l1_requests > 0 {
            self.l1_hits as f64 / total_l1_requests as f64
        } else {
            0.0
        }
    }

    pub fn compression_ratio(&self) -> f64 {
        if self.entry_count > 0 {
            self.compressions as f64 / self.entry_count as f64
        } else {
            0.0
        }
    }
}

impl<K, V> MultiLevelCache<K, V>
where
    K: Clone + Eq + Hash + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(config: CacheConfig) -> Self {
        let l1_size = (config.max_size_mb / 4).max(1);
        let l1_capacity = NonZeroUsize::new(l1_size * 1024).unwrap();
        
        Self {
            config,
            l1_cache: Arc::new(RwLock::new(LruCache::new(l1_capacity))),
            l2_cache: Arc::new(DashMap::new()),
            hot_keys: Arc::new(RwLock::new(HashMap::new())),
            cache_stats: Arc::new(RwLock::new(CacheStats::default())),
            total_size_bytes: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        self.update_hot_keys(key).await;
        
        if let Some(mut entry) = self.l1_cache.write().await.get_mut(key) {
            entry.touch();
            self.cache_stats.write().await.l1_hits += 1;
            debug!("L1 cache hit for key");
            return Some(entry.value.clone());
        }

        self.cache_stats.write().await.l1_misses += 1;

        if let Some(mut entry) = self.l2_cache.get_mut(key) {
            if !entry.is_expired() {
                entry.touch();
                self.cache_stats.write().await.l2_hits += 1;
                
                self.promote_to_l1(key.clone(), entry.value.clone()).await;
                debug!("L2 cache hit for key");
                return Some(entry.value.clone());
            } else {
                self.l2_cache.remove(key);
                self.update_total_size(-entry.size_bytes as i64).await;
            }
        }

        self.cache_stats.write().await.l2_misses += 1;
        None
    }

    pub async fn put(&self, key: K, value: V, ttl: Option<Duration>) -> Result<()> {
        let ttl = ttl.unwrap_or(self.config.default_ttl);
        let size_bytes = self.estimate_size(&value);
        
        self.ensure_capacity(size_bytes).await?;
        
        let entry = CacheEntry::new(value.clone(), ttl, size_bytes);
        
        if self.is_hot_key(&key).await {
            self.l1_cache.write().await.put(key.clone(), entry.clone());
            debug!("Inserted hot key into L1 cache");
        } else {
            self.l2_cache.insert(key.clone(), entry);
            debug!("Inserted key into L2 cache");
        }
        
        self.update_total_size(size_bytes as i64).await;
        self.update_cache_stats().await;
        
        Ok(())
    }

    pub async fn remove(&self, key: &K) -> Option<V> {
        let mut removed_value = None;
        let mut removed_size = 0;

        if let Some(entry) = self.l1_cache.write().await.pop(key) {
            removed_value = Some(entry.value);
            removed_size = entry.size_bytes;
        } else if let Some((_, entry)) = self.l2_cache.remove(key) {
            removed_value = Some(entry.value);
            removed_size = entry.size_bytes;
        }

        if removed_value.is_some() {
            self.update_total_size(-(removed_size as i64)).await;
            self.update_cache_stats().await;
        }

        removed_value
    }

    pub async fn clear(&self) {
        self.l1_cache.write().await.clear();
        self.l2_cache.clear();
        *self.total_size_bytes.write().await = 0;
        *self.cache_stats.write().await = CacheStats::default();
        info!("Cache cleared");
    }

    pub async fn cleanup_expired(&self) -> usize {
        let mut removed_count = 0;
        let mut keys_to_remove = Vec::new();

        for entry in self.l2_cache.iter() {
            if entry.is_expired() {
                keys_to_remove.push(entry.key().clone());
            }
        }

        for key in keys_to_remove {
            if let Some((_, entry)) = self.l2_cache.remove(&key) {
                self.update_total_size(-(entry.size_bytes as i64)).await;
                removed_count += 1;
            }
        }

        if removed_count > 0 {
            info!("Cleaned up {} expired cache entries", removed_count);
            self.update_cache_stats().await;
        }

        removed_count
    }

    pub async fn get_stats(&self) -> CacheStats {
        let mut stats = self.cache_stats.read().await.clone();
        stats.total_size_bytes = *self.total_size_bytes.read().await;
        stats.entry_count = self.l1_cache.read().await.len() + self.l2_cache.len();
        stats
    }

    async fn promote_to_l1(&self, key: K, value: V) {
        let size_bytes = self.estimate_size(&value);
        let entry = CacheEntry::new(value, self.config.default_ttl, size_bytes);
        
        if let Some(evicted) = self.l1_cache.write().await.put(key, entry) {
            debug!("Evicted entry from L1 during promotion");
            self.cache_stats.write().await.evictions += 1;
        }
    }

    async fn update_hot_keys(&self, key: &K) {
        let mut hot_keys = self.hot_keys.write().await;
        *hot_keys.entry(key.clone()).or_insert(0) += 1;
        
        if hot_keys.len() > 10000 {
            hot_keys.retain(|_, count| *count > 10);
        }
    }

    async fn is_hot_key(&self, key: &K) -> bool {
        self.hot_keys.read().await.get(key).map_or(false, |&count| count > 5)
    }

    async fn ensure_capacity(&self, required_size: usize) -> Result<()> {
        let max_size = self.config.max_size_mb * 1024 * 1024;
        let current_size = *self.total_size_bytes.read().await;
        
        if current_size + required_size > max_size {
            let bytes_to_free = (current_size + required_size) - max_size + (max_size / 10);
            self.evict_lru_entries(bytes_to_free).await?;
        }
        
        Ok(())
    }

    async fn evict_lru_entries(&self, bytes_to_free: usize) -> Result<()> {
        let mut freed_bytes = 0;
        let mut eviction_count = 0;

        let mut candidates: Vec<(K, Instant, usize)> = self.l2_cache.iter()
            .map(|entry| (entry.key().clone(), entry.last_accessed, entry.size_bytes))
            .collect();
        
        candidates.sort_by_key(|(_, last_accessed, _)| *last_accessed);

        for (key, _, size) in candidates {
            if freed_bytes >= bytes_to_free {
                break;
            }
            
            if let Some((_, entry)) = self.l2_cache.remove(&key) {
                freed_bytes += entry.size_bytes;
                eviction_count += 1;
            }
        }

        if freed_bytes > 0 {
            self.update_total_size(-(freed_bytes as i64)).await;
            self.cache_stats.write().await.evictions += eviction_count;
            info!("Evicted {} entries, freed {} bytes", eviction_count, freed_bytes);
        }

        Ok(())
    }

    async fn update_total_size(&self, delta: i64) {
        let mut size = self.total_size_bytes.write().await;
        if delta < 0 && (-delta) as usize > *size {
            *size = 0;
        } else {
            *size = (*size as i64 + delta) as usize;
        }
    }

    async fn update_cache_stats(&self) {
        let mut stats = self.cache_stats.write().await;
        stats.total_size_bytes = *self.total_size_bytes.read().await;
        stats.entry_count = self.l1_cache.read().await.len() + self.l2_cache.len();
    }

    fn estimate_size(&self, _value: &V) -> usize {
        std::mem::size_of::<V>() + 64
    }

    pub async fn start_cleanup_task(&self) {
        let cache = self.clone_weak();
        let interval = self.config.cleanup_interval;
        
        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;
                if let Some(cache_ref) = cache.upgrade() {
                    cache_ref.cleanup_expired().await;
                } else {
                    break;
                }
            }
        });
    }

    fn clone_weak(&self) -> std::sync::Weak<Self> {
        std::sync::Weak::new()
    }
}

impl<K, V> Clone for MultiLevelCache<K, V>
where
    K: Clone + Eq + Hash + Send + Sync,
    V: Clone + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            l1_cache: self.l1_cache.clone(),
            l2_cache: self.l2_cache.clone(),
            hot_keys: self.hot_keys.clone(),
            cache_stats: self.cache_stats.clone(),
            total_size_bytes: self.total_size_bytes.clone(),
        }
    }
}

pub type QueryCache = MultiLevelCache<String, Vec<u8>>;
pub type StateCache = MultiLevelCache<B256, serde_json::Value>;
pub type AgentCache = MultiLevelCache<Address, crate::shared::types::AgentMemoryState>;

#[derive(Debug)]
pub struct CacheManager {
    query_cache: QueryCache,
    state_cache: StateCache,
    agent_cache: AgentCache,
    config: CacheConfig,
}

impl CacheManager {
    pub fn new(config: CacheConfig) -> Self {
        let query_cache = QueryCache::new(config.clone());
        let state_cache = StateCache::new(config.clone());
        let agent_cache = AgentCache::new(config.clone());
        
        Self {
            query_cache,
            state_cache,
            agent_cache,
            config,
        }
    }

    pub async fn start_all_cleanup_tasks(&self) {
        self.query_cache.start_cleanup_task().await;
        self.state_cache.start_cleanup_task().await;
        self.agent_cache.start_cleanup_task().await;
    }

    pub async fn get_combined_stats(&self) -> CombinedCacheStats {
        let query_stats = self.query_cache.get_stats().await;
        let state_stats = self.state_cache.get_stats().await;
        let agent_stats = self.agent_cache.get_stats().await;

        CombinedCacheStats {
            query_cache: query_stats,
            state_cache: state_stats,
            agent_cache: agent_stats,
            total_memory_mb: (query_stats.total_size_bytes + 
                             state_stats.total_size_bytes + 
                             agent_stats.total_size_bytes) as f64 / (1024.0 * 1024.0),
        }
    }

    pub fn query_cache(&self) -> &QueryCache {
        &self.query_cache
    }

    pub fn state_cache(&self) -> &StateCache {
        &self.state_cache
    }

    pub fn agent_cache(&self) -> &AgentCache {
        &self.agent_cache
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombinedCacheStats {
    pub query_cache: CacheStats,
    pub state_cache: CacheStats,
    pub agent_cache: CacheStats,
    pub total_memory_mb: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_multi_level_cache_basic_operations() {
        let config = CacheConfig::default();
        let cache: MultiLevelCache<String, String> = MultiLevelCache::new(config);

        cache.put("key1".to_string(), "value1".to_string(), None).await.unwrap();
        
        let value = cache.get(&"key1".to_string()).await;
        assert_eq!(value, Some("value1".to_string()));
        
        let stats = cache.get_stats().await;
        assert_eq!(stats.entry_count, 1);
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let config = CacheConfig {
            default_ttl: Duration::from_millis(10),
            ..Default::default()
        };
        let cache: MultiLevelCache<String, String> = MultiLevelCache::new(config);

        cache.put("key1".to_string(), "value1".to_string(), None).await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(20)).await;
        
        let value = cache.get(&"key1".to_string()).await;
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let config = CacheConfig::default();
        let cache: MultiLevelCache<String, String> = MultiLevelCache::new(config);

        cache.put("key1".to_string(), "value1".to_string(), None).await.unwrap();
        cache.get(&"key1".to_string()).await;
        cache.get(&"key2".to_string()).await;
        
        let stats = cache.get_stats().await;
        assert!(stats.l1_hits > 0 || stats.l2_hits > 0);
        assert!(stats.l1_misses > 0 || stats.l2_misses > 0);
    }

    #[tokio::test]
    async fn test_cache_manager() {
        let config = CacheConfig::default();
        let manager = CacheManager::new(config);
        
        let stats = manager.get_combined_stats().await;
        assert_eq!(stats.query_cache.entry_count, 0);
        assert_eq!(stats.state_cache.entry_count, 0);
        assert_eq!(stats.agent_cache.entry_count, 0);
    }
}