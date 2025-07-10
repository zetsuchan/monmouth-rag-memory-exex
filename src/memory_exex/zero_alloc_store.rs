use eyre::Result;
use alloy_primitives::{Address, B256};
use std::collections::HashMap;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Zero-allocation memory store optimized for performance
/// 
/// Based on the original design notes, this implementation minimizes heap allocations
/// by reusing buffers for consecutive updates on the same intent_id.

#[derive(Debug)]
pub struct ZeroAllocMemoryEntry {
    pub data: Vec<u8>,
    pub last_block: u64,
    pub access_count: u64,
    /// Pre-allocated capacity for buffer reuse
    pub reserved_capacity: usize,
}

#[derive(Debug)]
pub struct ZeroAllocMemoryStore {
    /// Agent -> (IntentId -> MemoryEntry)
    entries: DashMap<Address, HashMap<B256, ZeroAllocMemoryEntry>>,
    /// Agent -> Current memory root (intent_id)
    current_root: DashMap<Address, B256>,
    /// TTL in blocks
    ttl_blocks: u64,
    /// Buffer pool for common sizes
    buffer_pool: Arc<BufferPool>,
}

#[derive(Debug)]
pub struct BufferPool {
    small_buffers: RwLock<Vec<Vec<u8>>>,  // 1KB buffers
    medium_buffers: RwLock<Vec<Vec<u8>>>, // 16KB buffers
    large_buffers: RwLock<Vec<Vec<u8>>>,  // 256KB buffers
}

impl BufferPool {
    const SMALL_SIZE: usize = 1024;
    const MEDIUM_SIZE: usize = 16 * 1024;
    const LARGE_SIZE: usize = 256 * 1024;
    
    pub fn new() -> Self {
        Self {
            small_buffers: RwLock::new(Vec::with_capacity(100)),
            medium_buffers: RwLock::new(Vec::with_capacity(50)),
            large_buffers: RwLock::new(Vec::with_capacity(10)),
        }
    }
    
    pub fn acquire(&self, size: usize) -> Vec<u8> {
        if size <= Self::SMALL_SIZE {
            if let Some(buffer) = self.small_buffers.write().pop() {
                return buffer;
            }
            Vec::with_capacity(Self::SMALL_SIZE)
        } else if size <= Self::MEDIUM_SIZE {
            if let Some(buffer) = self.medium_buffers.write().pop() {
                return buffer;
            }
            Vec::with_capacity(Self::MEDIUM_SIZE)
        } else if size <= Self::LARGE_SIZE {
            if let Some(buffer) = self.large_buffers.write().pop() {
                return buffer;
            }
            Vec::with_capacity(Self::LARGE_SIZE)
        } else {
            Vec::with_capacity(size)
        }
    }
    
    pub fn release(&self, mut buffer: Vec<u8>) {
        buffer.clear();
        let capacity = buffer.capacity();
        
        if capacity <= Self::SMALL_SIZE {
            let mut small = self.small_buffers.write();
            if small.len() < 100 {
                small.push(buffer);
            }
        } else if capacity <= Self::MEDIUM_SIZE {
            let mut medium = self.medium_buffers.write();
            if medium.len() < 50 {
                medium.push(buffer);
            }
        } else if capacity <= Self::LARGE_SIZE {
            let mut large = self.large_buffers.write();
            if large.len() < 10 {
                large.push(buffer);
            }
        }
    }
}

impl ZeroAllocMemoryStore {
    pub fn new(ttl_blocks: u64) -> Self {
        Self {
            entries: DashMap::new(),
            current_root: DashMap::new(),
            ttl_blocks,
            buffer_pool: Arc::new(BufferPool::new()),
        }
    }
    
    /// Insert or update with zero allocation when possible
    pub fn insert_or_update(
        &self,
        agent: Address,
        intent_id: B256,
        data: &[u8],
        current_block: u64,
    ) -> Result<B256> {
        let mut agent_entries = self.entries.entry(agent).or_insert_with(HashMap::new);
        
        if let Some(entry) = agent_entries.get_mut(&intent_id) {
            // Zero-allocation update path
            entry.data.clear();
            
            // If existing capacity is sufficient, reuse it
            if entry.data.capacity() >= data.len() {
                entry.data.extend_from_slice(data);
            } else {
                // Need more capacity, but try to minimize reallocation
                entry.data.reserve(data.len() - entry.data.capacity());
                entry.data.extend_from_slice(data);
                entry.reserved_capacity = entry.data.capacity();
            }
            
            entry.last_block = current_block;
            entry.access_count += 1;
        } else {
            // New entry - acquire buffer from pool
            let mut buffer = self.buffer_pool.acquire(data.len());
            buffer.extend_from_slice(data);
            
            agent_entries.insert(intent_id, ZeroAllocMemoryEntry {
                data: buffer,
                last_block: current_block,
                access_count: 1,
                reserved_capacity: data.len(),
            });
        }
        
        // Update current root
        self.current_root.insert(agent, intent_id);
        
        Ok(intent_id)
    }
    
    /// Get by intent with minimal copying
    #[inline]
    pub fn get_by_intent(&self, agent: Address, intent_id: B256) -> Option<Vec<u8>> {
        self.entries
            .get(&agent)?
            .get(&intent_id)
            .map(|entry| {
                // Only clone if necessary - in practice, could return &[u8]
                entry.data.clone()
            })
    }
    
    /// Get current memory for agent
    #[inline]
    pub fn get_current(&self, agent: Address) -> Option<Vec<u8>> {
        let intent_id = *self.current_root.get(&agent)?;
        self.get_by_intent(agent, intent_id)
    }
    
    /// Get current memory root (intent_id) for agent
    #[inline]
    pub fn get_current_root(&self, agent: Address) -> Option<B256> {
        self.current_root.get(&agent).map(|r| *r)
    }
    
    /// Flush agent memory and return buffers to pool
    pub fn flush_agent(&self, agent: Address) {
        if let Some((_, mut agent_map)) = self.entries.remove(&agent) {
            // Return buffers to pool
            for (_, entry) in agent_map.drain() {
                self.buffer_pool.release(entry.data);
            }
        }
        self.current_root.remove(&agent);
    }
    
    /// Prune expired entries
    pub fn prune_expired(&self, current_block: u64) {
        let mut to_remove = Vec::new();
        
        for entry in self.entries.iter() {
            let agent = *entry.key();
            for (intent_id, memory_entry) in entry.value() {
                if current_block.saturating_sub(memory_entry.last_block) >= self.ttl_blocks {
                    to_remove.push((agent, *intent_id));
                }
            }
        }
        
        for (agent, intent_id) in to_remove {
            if let Some(mut agent_entries) = self.entries.get_mut(&agent) {
                if let Some(entry) = agent_entries.remove(&intent_id) {
                    self.buffer_pool.release(entry.data);
                }
                
                // Remove current root if it was pruned
                if let Some(current) = self.current_root.get(&agent) {
                    if *current == intent_id {
                        self.current_root.remove(&agent);
                    }
                }
            }
        }
    }
    
    /// Revert memory from specific block
    pub fn revert_block(&self, reverted_block: u64) {
        let mut to_remove = Vec::new();
        
        for entry in self.entries.iter() {
            let agent = *entry.key();
            for (intent_id, memory_entry) in entry.value() {
                if memory_entry.last_block == reverted_block {
                    to_remove.push((agent, *intent_id));
                }
            }
        }
        
        for (agent, intent_id) in to_remove {
            if let Some(mut agent_entries) = self.entries.get_mut(&agent) {
                if let Some(entry) = agent_entries.remove(&intent_id) {
                    self.buffer_pool.release(entry.data);
                }
                
                if let Some(current) = self.current_root.get(&agent) {
                    if *current == intent_id {
                        self.current_root.remove(&agent);
                    }
                }
            }
        }
    }
    
    /// Get memory statistics
    pub fn get_stats(&self) -> MemoryStats {
        let mut total_agents = 0;
        let mut total_entries = 0;
        let mut total_bytes = 0;
        
        for entry in self.entries.iter() {
            total_agents += 1;
            total_entries += entry.value().len();
            for (_, memory_entry) in entry.value() {
                total_bytes += memory_entry.data.len();
            }
        }
        
        MemoryStats {
            total_agents,
            total_entries,
            total_bytes,
            ttl_blocks: self.ttl_blocks,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_agents: usize,
    pub total_entries: usize,
    pub total_bytes: usize,
    pub ttl_blocks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_zero_alloc_updates() {
        let store = ZeroAllocMemoryStore::new(100);
        let agent = Address::random();
        let intent_id = B256::random();
        
        // First insert
        let data1 = b"Initial state";
        store.insert_or_update(agent, intent_id, data1, 10).unwrap();
        
        // Update with same size - should reuse buffer
        let data2 = b"Updated state";
        store.insert_or_update(agent, intent_id, data2, 11).unwrap();
        
        let retrieved = store.get_by_intent(agent, intent_id).unwrap();
        assert_eq!(retrieved, data2);
        
        // Update with larger data - will need reallocation
        let data3 = b"Much larger updated state with more content";
        store.insert_or_update(agent, intent_id, data3, 12).unwrap();
        
        let retrieved = store.get_by_intent(agent, intent_id).unwrap();
        assert_eq!(retrieved, data3);
    }
    
    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new();
        
        // Acquire and release small buffer
        let buf1 = pool.acquire(500);
        assert!(buf1.capacity() >= BufferPool::SMALL_SIZE);
        pool.release(buf1);
        
        // Should reuse the buffer
        let buf2 = pool.acquire(500);
        assert!(buf2.capacity() >= BufferPool::SMALL_SIZE);
    }
    
    #[test]
    fn test_ttl_pruning() {
        let store = ZeroAllocMemoryStore::new(10);
        let agent = Address::random();
        
        let intent1 = B256::random();
        let intent2 = B256::random();
        
        store.insert_or_update(agent, intent1, b"old", 50).unwrap();
        store.insert_or_update(agent, intent2, b"new", 55).unwrap();
        
        // Prune at block 61 - intent1 should be removed (61-50=11 > 10)
        store.prune_expired(61);
        
        assert!(store.get_by_intent(agent, intent1).is_none());
        assert!(store.get_by_intent(agent, intent2).is_some());
    }
}