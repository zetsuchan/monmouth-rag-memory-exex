use crate::memory_exex::{memory_store::MemoryStore, Memory};
use eyre::Result;
use std::sync::Arc;
use dashmap::DashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub agent_id: String,
    pub timestamp: std::time::SystemTime,
    pub memory_count: usize,
    pub memory_hash: [u8; 32],
    pub metadata: CheckpointMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    pub block_number: Option<u64>,
    pub importance_threshold: f64,
    pub compressed_size: usize,
}

#[derive(Debug)]
pub struct CheckpointManager {
    memory_store: Arc<MemoryStore>,
    checkpoints: Arc<DashMap<String, Vec<Checkpoint>>>,
    config: CheckpointConfig,
}

#[derive(Debug, Clone)]
pub struct CheckpointConfig {
    pub max_checkpoints_per_agent: usize,
    pub compression_enabled: bool,
    pub importance_threshold: f64,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            max_checkpoints_per_agent: 10,
            compression_enabled: true,
            importance_threshold: 0.5,
        }
    }
}

impl CheckpointManager {
    pub fn new(memory_store: Arc<MemoryStore>) -> Self {
        Self {
            memory_store,
            checkpoints: Arc::new(DashMap::new()),
            config: CheckpointConfig::default(),
        }
    }
    
    pub async fn create_checkpoint(&self, agent_id: &str) -> Result<Checkpoint> {
        let memories = self.memory_store.get_all_memories(agent_id).await?;
        
        let filtered_memories: Vec<Memory> = memories.into_iter()
            .filter(|m| m.importance >= self.config.importance_threshold)
            .collect();
        
        let memory_hash = self.calculate_checkpoint_hash(&filtered_memories)?;
        
        let checkpoint_data = self.serialize_memories(&filtered_memories)?;
        let compressed_size = if self.config.compression_enabled {
            self.compress_data(&checkpoint_data)?.len()
        } else {
            checkpoint_data.len()
        };
        
        let checkpoint = Checkpoint {
            id: format!("cp_{}_{}", agent_id, chrono::Utc::now().timestamp()),
            agent_id: agent_id.to_string(),
            timestamp: std::time::SystemTime::now(),
            memory_count: filtered_memories.len(),
            memory_hash,
            metadata: CheckpointMetadata {
                block_number: None,
                importance_threshold: self.config.importance_threshold,
                compressed_size,
            },
        };
        
        self.save_checkpoint(&checkpoint)?;
        
        self.manage_checkpoint_history(agent_id).await?;
        
        Ok(checkpoint)
    }
    
    pub async fn restore_from_checkpoint(&self, checkpoint_id: &str) -> Result<Vec<Memory>> {
        let checkpoint = self.get_checkpoint(checkpoint_id)?;
        
        Ok(vec![])
    }
    
    pub async fn list_checkpoints(&self, agent_id: &str) -> Vec<Checkpoint> {
        self.checkpoints
            .get(agent_id)
            .map(|checkpoints| checkpoints.clone())
            .unwrap_or_default()
    }
    
    pub async fn delete_checkpoint(&self, checkpoint_id: &str) -> Result<()> {
        for mut agent_checkpoints in self.checkpoints.iter_mut() {
            agent_checkpoints.retain(|cp| cp.id != checkpoint_id);
        }
        Ok(())
    }
    
    pub async fn create_differential_checkpoint(
        &self,
        agent_id: &str,
        base_checkpoint_id: &str,
    ) -> Result<Checkpoint> {
        let base_checkpoint = self.get_checkpoint(base_checkpoint_id)?;
        let current_memories = self.memory_store.get_all_memories(agent_id).await?;
        
        let new_memories: Vec<Memory> = current_memories.into_iter()
            .filter(|m| {
                m.timestamp > std::time::Instant::now() - std::time::Duration::from_secs(
                    base_checkpoint.timestamp.elapsed().unwrap().as_secs()
                )
            })
            .collect();
        
        let memory_hash = self.calculate_checkpoint_hash(&new_memories)?;
        
        let checkpoint = Checkpoint {
            id: format!("cp_diff_{}_{}", agent_id, chrono::Utc::now().timestamp()),
            agent_id: agent_id.to_string(),
            timestamp: std::time::SystemTime::now(),
            memory_count: new_memories.len(),
            memory_hash,
            metadata: CheckpointMetadata {
                block_number: None,
                importance_threshold: self.config.importance_threshold,
                compressed_size: 0,
            },
        };
        
        self.save_checkpoint(&checkpoint)?;
        
        Ok(checkpoint)
    }
    
    fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        self.checkpoints
            .entry(checkpoint.agent_id.clone())
            .or_insert_with(Vec::new)
            .push(checkpoint.clone());
        
        Ok(())
    }
    
    fn get_checkpoint(&self, checkpoint_id: &str) -> Result<Checkpoint> {
        for checkpoints in self.checkpoints.iter() {
            if let Some(checkpoint) = checkpoints.iter().find(|cp| cp.id == checkpoint_id) {
                return Ok(checkpoint.clone());
            }
        }
        Err(eyre::eyre!("Checkpoint not found"))
    }
    
    async fn manage_checkpoint_history(&self, agent_id: &str) -> Result<()> {
        if let Some(mut checkpoints) = self.checkpoints.get_mut(agent_id) {
            if checkpoints.len() > self.config.max_checkpoints_per_agent {
                checkpoints.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
                checkpoints.drain(0..checkpoints.len() - self.config.max_checkpoints_per_agent);
            }
        }
        Ok(())
    }
    
    fn calculate_checkpoint_hash(&self, memories: &[Memory]) -> Result<[u8; 32]> {
        use sha3::{Sha3_256, Digest};
        
        let mut hasher = Sha3_256::new();
        for memory in memories {
            hasher.update(&memory.id.as_bytes());
            hasher.update(&memory.content);
        }
        
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        
        Ok(hash)
    }
    
    fn serialize_memories(&self, memories: &[Memory]) -> Result<Vec<u8>> {
        Ok(bincode::serialize(memories)?)
    }
    
    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        Ok(encoder.finish()?)
    }
}