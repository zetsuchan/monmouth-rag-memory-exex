use crate::memory_exex::{Memory, MemoryType};
use eyre::Result;
use libmdbx::WriteFlags;
use std::sync::Arc;
use tokio::sync::RwLock;
use dashmap::DashMap;
use lru::LruCache;
use std::num::NonZeroUsize;

#[derive(Debug)]
pub struct MemoryStore {
    env: Arc<libmdbx::Environment<libmdbx::NoWriteMap>>,
    cache: Arc<DashMap<String, Memory>>,
    lru: Arc<RwLock<LruCache<String, Memory>>>,
}

impl MemoryStore {
    pub async fn new() -> Result<Self> {
        let path = "./memory_store";
        std::fs::create_dir_all(path)?;
        
        let env = libmdbx::Environment::<libmdbx::NoWriteMap>::new()
            .set_max_dbs(10)
            .set_map_size(1024 * 1024 * 1024 * 10)
            .open(path)?;
        let env = Arc::new(env);
        let cache = Arc::new(DashMap::new());
        let lru = Arc::new(RwLock::new(LruCache::new(NonZeroUsize::new(1000).unwrap())));
        
        Ok(Self { env, cache, lru })
    }
    
    pub async fn store(&self, memory: Memory) -> Result<()> {
        let key = format!("{}:{}", memory.agent_id, memory.id);
        
        self.cache.insert(key.clone(), memory.clone());
        
        let tx = self.env.begin_rw_txn()?;
        let db = tx.create_db(Some(&memory.agent_id), libmdbx::DatabaseFlags::empty())?;
        
        let memory_bytes = bincode::serialize(&memory)?;
        tx.put(&db, memory.id.as_bytes(), &memory_bytes, WriteFlags::empty())?;
        tx.commit()?;
        
        let mut lru = self.lru.write().await;
        lru.put(key, memory);
        
        Ok(())
    }
    
    pub async fn retrieve(&self, agent_id: &str, memory_id: &str) -> Result<Option<Memory>> {
        let key = format!("{}:{}", agent_id, memory_id);
        
        if let Some(memory) = self.cache.get(&key) {
            return Ok(Some(memory.clone()));
        }
        
        let mut lru = self.lru.write().await;
        if let Some(memory) = lru.get(&key) {
            return Ok(Some(memory.clone()));
        }
        drop(lru);
        
        let tx = self.env.begin_ro_txn()?;
        let db = match tx.open_db(Some(agent_id)) {
            Ok(db) => db,
            Err(_) => return Ok(None),
        };
        
        if let Ok(Some(memory_bytes)) = tx.get::<Vec<u8>>(&db, memory_id.as_bytes()) {
            let memory: Memory = bincode::deserialize(&memory_bytes)?;
            
            self.cache.insert(key.clone(), memory.clone());
            
            let mut lru = self.lru.write().await;
            lru.put(key, memory.clone());
            
            Ok(Some(memory))
        } else {
            Ok(None)
        }
    }
    
    pub async fn forget(&self, agent_id: &str, memory_id: &str) -> Result<()> {
        let key = format!("{}:{}", agent_id, memory_id);
        
        self.cache.remove(&key);
        
        let mut lru = self.lru.write().await;
        lru.pop(&key);
        drop(lru);
        
        let tx = self.env.begin_rw_txn()?;
        if let Ok(db) = tx.open_db(Some(agent_id)) {
            tx.del(&db, memory_id.as_bytes(), None)?;
            tx.commit()?;
        }
        
        Ok(())
    }
    
    pub async fn get_all_memories(&self, agent_id: &str) -> Result<Vec<Memory>> {
        let tx = self.env.begin_ro_txn()?;
        let db = match tx.open_db(Some(agent_id)) {
            Ok(db) => db,
            Err(_) => return Ok(vec![]),
        };
        
        let mut memories = Vec::new();
        let mut cursor = tx.cursor(&db)?;
        
        while let Ok(Some((_, value))) = cursor.next::<Vec<u8>, Vec<u8>>() {
            if let Ok(memory) = bincode::deserialize::<Memory>(&value) {
                memories.push(memory);
            }
        }
        
        Ok(memories)
    }
    
    pub async fn query_by_type(&self, agent_id: &str, memory_type: MemoryType) -> Result<Vec<Memory>> {
        let all_memories = self.get_all_memories(agent_id).await?;
        Ok(all_memories.into_iter()
            .filter(|m| std::mem::discriminant(&m.memory_type) == std::mem::discriminant(&memory_type))
            .collect())
    }
    
    pub async fn query_by_importance(&self, agent_id: &str, min_importance: f64) -> Result<Vec<Memory>> {
        let all_memories = self.get_all_memories(agent_id).await?;
        Ok(all_memories.into_iter()
            .filter(|m| m.importance >= min_importance)
            .collect())
    }
    
    pub async fn get_recent_memories(&self, agent_id: &str, limit: usize) -> Result<Vec<Memory>> {
        let mut all_memories = self.get_all_memories(agent_id).await?;
        all_memories.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        all_memories.truncate(limit);
        Ok(all_memories)
    }
    
    pub async fn update_access_count(&self, agent_id: &str, memory_id: &str) -> Result<()> {
        if let Some(mut memory) = self.retrieve(agent_id, memory_id).await? {
            memory.access_count += 1;
            self.store(memory).await?;
        }
        Ok(())
    }
}