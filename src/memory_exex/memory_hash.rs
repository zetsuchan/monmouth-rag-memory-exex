use crate::memory_exex::Memory;
use eyre::Result;
use sha3::{Sha3_256, Digest};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct MemoryLatticeHash {
    hasher: Sha3_256,
}

impl MemoryLatticeHash {
    pub fn new() -> Self {
        Self {
            hasher: Sha3_256::new(),
        }
    }
    
    pub async fn update_hash(&self, current_hash: &[u8; 32], memory: &Memory) -> Result<[u8; 32]> {
        let memory_hash = self.hash_memory(memory)?;
        
        let mut hasher = Sha3_256::new();
        hasher.update(current_hash);
        hasher.update(&memory_hash);
        hasher.update(&memory.timestamp.elapsed().as_secs().to_le_bytes());
        
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        
        Ok(hash)
    }
    
    pub async fn compute_agent_hash(&self, memories: &[Memory]) -> Result<[u8; 32]> {
        let mut memory_map = BTreeMap::new();
        
        for memory in memories {
            let memory_hash = self.hash_memory(memory)?;
            memory_map.insert(memory.id.clone(), memory_hash);
        }
        
        let mut hasher = Sha3_256::new();
        for (id, hash) in memory_map {
            hasher.update(id.as_bytes());
            hasher.update(&hash);
        }
        
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        
        Ok(hash)
    }
    
    pub async fn verify_memory_integrity(
        &self,
        memory: &Memory,
        expected_hash: &[u8; 32],
    ) -> Result<bool> {
        let computed_hash = self.hash_memory(memory)?;
        Ok(&computed_hash == expected_hash)
    }
    
    pub async fn compute_merkle_root(&self, memories: &[Memory]) -> Result<[u8; 32]> {
        if memories.is_empty() {
            return Ok([0u8; 32]);
        }
        
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        for memory in memories {
            hashes.push(self.hash_memory(memory)?);
        }
        
        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            
            for chunk in hashes.chunks(2) {
                let mut hasher = Sha3_256::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                
                let result = hasher.finalize();
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result);
                next_level.push(hash);
            }
            
            hashes = next_level;
        }
        
        Ok(hashes[0])
    }
    
    pub async fn generate_proof(&self, memory: &Memory, all_memories: &[Memory]) -> Result<Vec<[u8; 32]>> {
        let target_hash = self.hash_memory(memory)?;
        let mut proof = Vec::new();
        
        let mut hashes: Vec<[u8; 32]> = Vec::new();
        let mut target_index = None;
        
        for (i, mem) in all_memories.iter().enumerate() {
            let hash = self.hash_memory(mem)?;
            if hash == target_hash {
                target_index = Some(i);
            }
            hashes.push(hash);
        }
        
        let Some(mut index) = target_index else {
            return Err(eyre::eyre!("Memory not found in set"));
        };
        
        let mut level_hashes = hashes;
        while level_hashes.len() > 1 {
            let sibling_index = if index % 2 == 0 { index + 1 } else { index - 1 };
            
            if sibling_index < level_hashes.len() {
                proof.push(level_hashes[sibling_index]);
            }
            
            let mut next_level = Vec::new();
            for chunk in level_hashes.chunks(2) {
                let mut hasher = Sha3_256::new();
                hasher.update(&chunk[0]);
                if chunk.len() > 1 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                
                let result = hasher.finalize();
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result);
                next_level.push(hash);
            }
            
            level_hashes = next_level;
            index /= 2;
        }
        
        Ok(proof)
    }
    
    fn hash_memory(&self, memory: &Memory) -> Result<[u8; 32]> {
        let mut hasher = Sha3_256::new();
        
        hasher.update(memory.id.as_bytes());
        hasher.update(memory.agent_id.as_bytes());
        hasher.update(&[match &memory.memory_type {
            crate::memory_exex::MemoryType::ShortTerm => 0,
            crate::memory_exex::MemoryType::LongTerm => 1,
            crate::memory_exex::MemoryType::Working => 2,
            crate::memory_exex::MemoryType::Episodic => 3,
            crate::memory_exex::MemoryType::Semantic => 4,
        }]);
        hasher.update(&memory.content);
        hasher.update(&memory.importance.to_le_bytes());
        hasher.update(&memory.access_count.to_le_bytes());
        
        if let Some(embedding) = &memory.embedding {
            for value in embedding {
                hasher.update(&value.to_le_bytes());
            }
        }
        
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        
        Ok(hash)
    }
}