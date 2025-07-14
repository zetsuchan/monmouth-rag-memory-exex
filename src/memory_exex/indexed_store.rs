//! Enhanced memory store with indexing and batch operations
//! 
//! This module provides an optimized memory store that addresses:
//! - Block number indexing for efficient queries
//! - Batch operations to reduce I/O overhead
//! - Optimized cache eviction policies
//! - Better performance for high-throughput scenarios

use super::{Memory, MemoryType};
use alloy::primitives::BlockNumber;
use dashmap::DashMap;
use eyre::Result;
use libmdbx::WriteFlags;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use serde::{Serialize, Deserialize};

/// Index entry for block-based queries
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlockIndex {
    block_number: BlockNumber,
    memory_ids: Vec<String>,
}

/// Batch operation for efficient writes
#[derive(Debug)]
pub struct BatchOperation {
    pub agent_id: String,
    pub operations: Vec<MemoryOperation>,
}

#[derive(Debug)]
pub enum MemoryOperation {
    Store(Memory),
    Delete(String),
    Update(String, Memory),
}

/// Configuration for indexed store
#[derive(Debug, Clone)]
pub struct IndexedStoreConfig {
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,
    /// LRU cache size
    pub cache_size: usize,
    /// Enable block indexing
    pub enable_block_index: bool,
    /// Maximum concurrent operations
    pub max_concurrent_ops: usize,
}

impl Default for IndexedStoreConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            batch_timeout_ms: 100,
            cache_size: 10000,
            enable_block_index: true,
            max_concurrent_ops: 100,
        }
    }
}

/// Enhanced memory store with indexing
#[derive(Debug)]
pub struct IndexedMemoryStore {
    /// MDBX environment
    env: Arc<libmdbx::Environment<libmdbx::NoWriteMap>>,
    /// Memory cache (agent_id:memory_id -> Memory)
    cache: Arc<DashMap<String, Memory>>,
    /// LRU cache for frequently accessed memories
    lru: Arc<RwLock<LruCache<String, Memory>>>,
    /// Block index (block_number -> memory_ids)
    block_index: Arc<DashMap<BlockNumber, Vec<String>>>,
    /// Pending batch operations
    pending_batches: Arc<RwLock<Vec<BatchOperation>>>,
    /// Configuration
    config: IndexedStoreConfig,
    /// Semaphore for concurrency control
    semaphore: Arc<Semaphore>,
}

impl IndexedMemoryStore {
    pub async fn new(config: IndexedStoreConfig) -> Result<Self> {
        let path = "./indexed_memory_store";
        std::fs::create_dir_all(path)?;
        
        let env = libmdbx::Environment::<libmdbx::NoWriteMap>::new()
            .set_max_dbs(20) // Increased for indices
            .set_map_size(1024 * 1024 * 1024 * 50) // 50GB
            .open(path)?;
        
        // Create index databases
        let tx = env.begin_rw_txn()?;
        tx.create_db(Some("block_index"), libmdbx::libmdbx::DatabaseFlags::empty())?;
        tx.create_db(Some("type_index"), libmdbx::libmdbx::DatabaseFlags::empty())?;
        tx.create_db(Some("importance_index"), libmdbx::libmdbx::DatabaseFlags::empty())?;
        tx.commit()?;
        
        let env = Arc::new(env);
        let cache = Arc::new(DashMap::new());
        let lru = Arc::new(RwLock::new(LruCache::new(
            NonZeroUsize::new(config.cache_size).unwrap()
        )));
        let block_index = Arc::new(DashMap::new());
        let pending_batches = Arc::new(RwLock::new(Vec::new()));
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_ops));
        
        Ok(Self {
            env,
            cache,
            lru,
            block_index,
            pending_batches,
            config,
            semaphore,
        })
    }
    
    /// Store a single memory with indexing
    pub async fn store(&self, memory: Memory) -> Result<()> {
        let _permit = self.semaphore.acquire().await?;
        
        let key = format!("{}:{}", memory.agent_id, memory.id);
        
        // Update caches
        self.cache.insert(key.clone(), memory.clone());
        self.lru.write().await.put(key.clone(), memory.clone());
        
        // Update block index if applicable
        if self.config.enable_block_index {
            // For now, we'll use a fixed block number - in production
            // this would come from the transaction context
            let block_num = 0; // TODO: Get from context
            self.block_index
                .entry(block_num)
                .or_insert_with(Vec::new)
                .push(memory.id.clone());
        }
        
        // Persist to database
        let tx = self.env.begin_rw_txn()?;
        let db = tx.create_db(Some(&memory.agent_id), libmdbx::DatabaseFlags::empty())?;
        let memory_bytes = bincode::serialize(&memory)?;
        tx.put(&db, memory.id.as_bytes(), &memory_bytes, WriteFlags::empty())?;
        
        // Update indices
        if self.config.enable_block_index {
            self.update_indices(&tx, &memory)?;
        }
        
        tx.commit()?;
        
        Ok(())
    }
    
    /// Batch store multiple memories
    pub async fn batch_store(&self, memories: Vec<Memory>) -> Result<()> {
        let _permit = self.semaphore.acquire().await?;
        
        let tx = self.env.begin_rw_txn()?;
        
        for memory in &memories {
            let key = format!("{}:{}", memory.agent_id, memory.id);
            
            // Update caches
            self.cache.insert(key.clone(), memory.clone());
            
            // Create/open database for agent
            let db = tx.create_db(Some(&memory.agent_id), libmdbx::DatabaseFlags::empty())?;
            let memory_bytes = bincode::serialize(&memory)?;
            tx.put(&db, memory.id.as_bytes(), &memory_bytes, WriteFlags::empty())?;
            
            // Update indices
            if self.config.enable_block_index {
                self.update_indices(&tx, &memory)?;
            }
        }
        
        tx.commit()?;
        
        // Update LRU cache after commit
        let mut lru = self.lru.write().await;
        for memory in memories {
            let key = format!("{}:{}", memory.agent_id, memory.id);
            lru.put(key, memory);
        }
        
        Ok(())
    }
    
    /// Update database indices
    fn update_indices(&self, tx: &libmdbx::Transaction<libmdbx::RW>, memory: &Memory) -> Result<()> {
        // Block index - using fixed block for now
        let block_num = 0; // TODO: Get from context
        let block_idx_db = tx.open_db(Some("block_index"))?;
        let key = format!("block:{}", block_num);
        
        // Get existing entries
        let mut memory_ids: Vec<String> = if let Ok(Some(data)) = tx.get::<Vec<u8>>(&block_idx_db, key.as_bytes()) {
            bincode::deserialize(&data)?
        } else {
            Vec::new()
        };
        
        // Add new memory ID if not already present
        if !memory_ids.contains(&memory.id) {
            memory_ids.push(memory.id.clone());
            let data = bincode::serialize(&memory_ids)?;
            tx.put(&block_idx_db, key.as_bytes(), &data, WriteFlags::empty())?;
        }
        
        // Type index
        let type_idx_db = tx.open_db(Some("type_index"))?;
        let type_key = format!("{}:type:{:?}", memory.agent_id, memory.memory_type);
        
        let mut type_memory_ids: Vec<String> = if let Ok(Some(data)) = tx.get::<Vec<u8>>(&type_idx_db, type_key.as_bytes()) {
            bincode::deserialize(&data)?
        } else {
            Vec::new()
        };
        
        if !type_memory_ids.contains(&memory.id) {
            type_memory_ids.push(memory.id.clone());
            let data = bincode::serialize(&type_memory_ids)?;
            tx.put(&type_idx_db, type_key.as_bytes(), &data, WriteFlags::empty())?;
        }
        
        Ok(())
    }
    
    /// Query memories by block number
    pub async fn query_by_block(&self, block_number: BlockNumber) -> Result<Vec<Memory>> {
        let _permit = self.semaphore.acquire().await?;
        
        let tx = self.env.begin_ro_txn()?;
        let block_idx_db = tx.open_db(Some("block_index"))?;
        let key = format!("block:{}", block_number);
        
        if let Ok(Some(data)) = tx.get::<Vec<u8>>(&block_idx_db, key.as_bytes()) {
            let memory_ids: Vec<String> = bincode::deserialize(&data)?;
            let mut memories = Vec::new();
            
            for memory_id in memory_ids {
                // Try to find the memory across all agent databases
                // This is a simplified approach - in production, you'd want to store agent_id in the index
                if let Some(entry) = self.cache.iter().find(|e| e.value().id == memory_id) {
                    memories.push(entry.value().clone());
                }
            }
            
            Ok(memories)
        } else {
            Ok(vec![])
        }
    }
    
    /// Retrieve with optimized caching
    pub async fn retrieve(&self, agent_id: &str, memory_id: &str) -> Result<Option<Memory>> {
        let key = format!("{}:{}", agent_id, memory_id);
        
        // Check cache first
        if let Some(memory) = self.cache.get(&key) {
            return Ok(Some(memory.clone()));
        }
        
        // Check LRU
        {
            let mut lru = self.lru.write().await;
            if let Some(memory) = lru.get(&key) {
                return Ok(Some(memory.clone()));
            }
        }
        
        // Load from database
        let _permit = self.semaphore.acquire().await?;
        let tx = self.env.begin_ro_txn()?;
        let db = match tx.open_db(Some(agent_id)) {
            Ok(db) => db,
            Err(_) => return Ok(None),
        };
        
        if let Ok(Some(memory_bytes)) = tx.get::<Vec<u8>>(&db, memory_id.as_bytes()) {
            let memory: Memory = bincode::deserialize(&memory_bytes)?;
            
            // Update caches
            self.cache.insert(key.clone(), memory.clone());
            self.lru.write().await.put(key, memory.clone());
            
            Ok(Some(memory))
        } else {
            Ok(None)
        }
    }
    
    /// Add a batch operation to the queue
    pub async fn queue_batch_operation(&self, batch: BatchOperation) -> Result<()> {
        let mut batches = self.pending_batches.write().await;
        batches.push(batch);
        
        // Process if batch size exceeded
        if batches.len() >= self.config.max_batch_size {
            drop(batches);
            self.process_pending_batches().await?;
        }
        
        Ok(())
    }
    
    /// Process all pending batch operations
    pub async fn process_pending_batches(&self) -> Result<()> {
        let mut batches = self.pending_batches.write().await;
        if batches.is_empty() {
            return Ok(());
        }
        
        let current_batches = std::mem::take(&mut *batches);
        drop(batches);
        
        let _permit = self.semaphore.acquire().await?;
        let tx = self.env.begin_rw_txn()?;
        
        for batch in current_batches {
            let db = tx.create_db(Some(&batch.agent_id), libmdbx::DatabaseFlags::empty())?;
            
            for op in batch.operations {
                match op {
                    MemoryOperation::Store(memory) => {
                        let memory_bytes = bincode::serialize(&memory)?;
                        tx.put(&db, memory.id.as_bytes(), &memory_bytes, WriteFlags::empty())?;
                        
                        // Update cache
                        let key = format!("{}:{}", memory.agent_id, memory.id);
                        self.cache.insert(key, memory.clone());
                        
                        // Update indices
                        if self.config.enable_block_index {
                            self.update_indices(&tx, &memory)?;
                        }
                    }
                    MemoryOperation::Delete(memory_id) => {
                        tx.del(&db, memory_id.as_bytes(), None)?;
                        
                        // Remove from cache
                        let key = format!("{}:{}", batch.agent_id, memory_id);
                        self.cache.remove(&key);
                    }
                    MemoryOperation::Update(memory_id, memory) => {
                        let memory_bytes = bincode::serialize(&memory)?;
                        tx.put(&db, memory_id.as_bytes(), &memory_bytes, WriteFlags::empty())?;
                        
                        // Update cache
                        let key = format!("{}:{}", memory.agent_id, memory_id);
                        self.cache.insert(key, memory);
                    }
                }
            }
        }
        
        tx.commit()?;
        Ok(())
    }
    
    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> CacheStats {
        CacheStats {
            cache_size: self.cache.len(),
            lru_size: self.lru.read().await.len(),
            block_index_size: self.block_index.len(),
            pending_batches: self.pending_batches.read().await.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub cache_size: usize,
    pub lru_size: usize,
    pub block_index_size: usize,
    pub pending_batches: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
        
    #[tokio::test]
    async fn test_indexed_store() -> Result<()> {
        let config = IndexedStoreConfig::default();
        let store = IndexedMemoryStore::new(config).await?;
        
        // Create test memory
        let memory = Memory {
            id: "test1".to_string(),
            agent_id: "agent1".to_string(),
            memory_type: MemoryType::Working,
            content: "Test content".as_bytes().to_vec(),
            embedding: Some(vec![0.1; 384]),
            timestamp: std::time::Instant::now(),
            importance: 0.8,
            access_count: 0,
        };
        
        // Store memory
        store.store(memory.clone()).await?;
        
        // Retrieve by ID
        let retrieved = store.retrieve("agent1", "test1").await?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test1");
        
        // Query by block
        let block_memories = store.query_by_block(12345).await?;
        assert_eq!(block_memories.len(), 1);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_batch_operations() -> Result<()> {
        let config = IndexedStoreConfig::default();
        let store = IndexedMemoryStore::new(config).await?;
        
        let mut memories = Vec::new();
        for i in 0..10 {
            let memory = Memory {
                id: format!("test{}", i),
                agent_id: "agent1".to_string(),
                memory_type: MemoryType::LongTerm,
                content: format!("Test content {}", i).as_bytes().to_vec(),
                embedding: Some(vec![0.1; 384]),
                timestamp: std::time::Instant::now(),
                importance: 0.5,
                access_count: 0,
            };
            memories.push(memory);
        }
        
        // Batch store
        store.batch_store(memories).await?;
        
        // Verify all stored
        for i in 0..10 {
            let retrieved = store.retrieve("agent1", &format!("test{}", i)).await?;
            assert!(retrieved.is_some());
        }
        
        // Check cache stats
        let stats = store.get_cache_stats().await;
        assert_eq!(stats.cache_size, 10);
        
        Ok(())
    }
}