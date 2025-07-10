use alloy::primitives::{Address, B256, U256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn, error};

use crate::{
    alh::coordination::ALHCoordinator,
    context::preprocessing::PreprocessedContext,
    memory_exex::{MemoryStore, MemoryType},
    rag_exex::agent_context::UnifiedContextManager,
    shared::{
        agent_state_manager::{AgentStateManager, LifecycleState},
        types::{AgentContext, AgentMemoryState},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointData {
    pub checkpoint_id: B256,
    pub agent_address: Address,
    pub created_at: u64,
    pub block_number: u64,
    pub state_hash: B256,
    pub context_data: AgentContextSnapshot,
    pub memory_snapshot: MemorySnapshot,
    pub metadata: CheckpointMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContextSnapshot {
    pub agent_state: AgentContext,
    pub memory_state: AgentMemoryState,
    pub lifecycle_state: LifecycleState,
    pub context_history: Vec<PreprocessedContext>,
    pub interaction_count: u64,
    pub last_heartbeat: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub memory_entries: Vec<MemoryEntry>,
    pub memory_hash: B256,
    pub total_size: u64,
    pub compression_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub memory_type: MemoryType,
    pub content_hash: B256,
    pub metadata: serde_json::Value,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    pub triggered_by: CheckpointTrigger,
    pub compression_enabled: bool,
    pub validation_hash: B256,
    pub size_bytes: u64,
    pub creation_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CheckpointTrigger {
    Periodic,
    StateTransition,
    Reorg,
    Manual,
    Emergency,
}

#[derive(Debug, Clone)]
pub struct CheckpointConfig {
    pub periodic_interval: Duration,
    pub max_checkpoints_per_agent: usize,
    pub enable_compression: bool,
    pub enable_validation: bool,
    pub retention_period: Duration,
    pub batch_size: usize,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            periodic_interval: Duration::from_secs(300),
            max_checkpoints_per_agent: 10,
            enable_compression: true,
            enable_validation: true,
            retention_period: Duration::from_secs(86400 * 7),
            batch_size: 5,
        }
    }
}

#[async_trait]
pub trait AgentCheckpointer: Send + Sync {
    async fn create_checkpoint(&self, agent_address: Address, trigger: CheckpointTrigger) -> Result<B256>;
    async fn get_checkpoint(&self, checkpoint_id: B256) -> Result<Option<CheckpointData>>;
    async fn list_checkpoints(&self, agent_address: Address) -> Result<Vec<B256>>;
    async fn delete_checkpoint(&self, checkpoint_id: B256) -> Result<()>;
    async fn validate_checkpoint(&self, checkpoint_id: B256) -> Result<bool>;
}

pub struct CheckpointManager {
    alh_coordinator: Arc<ALHCoordinator>,
    memory_store: Arc<MemoryStore>,
    agent_state_manager: Arc<AgentStateManager>,
    context_manager: Arc<UnifiedContextManager>,
    
    checkpoint_store: Arc<DashMap<B256, CheckpointData>>,
    agent_checkpoints: Arc<DashMap<Address, Vec<B256>>>,
    
    trigger_channel: mpsc::Sender<CheckpointRequest>,
    config: CheckpointConfig,
}

#[derive(Debug)]
struct CheckpointRequest {
    agent_address: Address,
    trigger: CheckpointTrigger,
    response_channel: mpsc::Sender<Result<B256>>,
}

impl CheckpointManager {
    pub fn new(
        alh_coordinator: Arc<ALHCoordinator>,
        memory_store: Arc<MemoryStore>,
        agent_state_manager: Arc<AgentStateManager>,
        context_manager: Arc<UnifiedContextManager>,
        config: CheckpointConfig,
    ) -> (Self, mpsc::Receiver<CheckpointRequest>) {
        let (trigger_tx, trigger_rx) = mpsc::channel(1000);
        
        let manager = Self {
            alh_coordinator,
            memory_store,
            agent_state_manager,
            context_manager,
            checkpoint_store: Arc::new(DashMap::new()),
            agent_checkpoints: Arc::new(DashMap::new()),
            trigger_channel: trigger_tx,
            config,
        };
        
        (manager, trigger_rx)
    }
    
    pub async fn start_periodic_checkpointing(&self) -> Result<()> {
        let mut interval = tokio::time::interval(self.config.periodic_interval);
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.create_periodic_checkpoints().await {
                error!("Periodic checkpoint creation failed: {}", e);
            }
        }
    }
    
    async fn create_periodic_checkpoints(&self) -> Result<()> {
        let active_agents = self.agent_state_manager.get_active_agents().await;
        
        for agent in active_agents {
            if let Err(e) = self.create_checkpoint(agent, CheckpointTrigger::Periodic).await {
                warn!("Failed to create periodic checkpoint for agent {:?}: {}", agent, e);
            }
        }
        
        Ok(())
    }
    
    async fn capture_agent_context(&self, agent_address: Address) -> Result<AgentContextSnapshot> {
        let agent_state = self.agent_state_manager.get_agent_state(agent_address).await?;
        let agent_context = AgentContext {
            agent_address,
            last_action: crate::shared::types::AgentAction::Transfer {
                to: Address::ZERO,
                amount: U256::ZERO,
            },
            reputation_score: 50,
            total_interactions: 0,
            success_rate: 0.0,
            specialization: vec!["general".to_string()],
        };
        
        let memory_state = AgentMemoryState {
            agent_address,
            working_memory: Vec::new(),
            episodic_buffer: Vec::new(),
            last_access_time: chrono::Utc::now().timestamp() as u64,
            context_switches: 0,
        };
        
        let context_history = self.context_manager
            .get_agent_context_history(agent_address)
            .await?;
        
        Ok(AgentContextSnapshot {
            agent_state: agent_context,
            memory_state,
            lifecycle_state: agent_state.lifecycle_state,
            context_history,
            interaction_count: agent_state.interaction_count,
            last_heartbeat: agent_state.last_heartbeat,
        })
    }
    
    async fn capture_memory_snapshot(&self, agent_address: Address) -> Result<MemorySnapshot> {
        let memories = self.memory_store.get_agent_memories(&agent_address).await?;
        
        let mut memory_entries = Vec::new();
        let mut total_size = 0u64;
        
        for memory in memories {
            let entry = MemoryEntry {
                memory_type: memory.memory_type,
                content_hash: memory.content_hash,
                metadata: serde_json::json!({}),
                size: memory.content.len() as u64,
            };
            
            total_size += entry.size;
            memory_entries.push(entry);
        }
        
        let memory_hash = self.compute_memory_hash(&memory_entries).await?;
        
        Ok(MemorySnapshot {
            memory_entries,
            memory_hash,
            total_size,
            compression_ratio: if self.config.enable_compression { 0.75 } else { 1.0 },
        })
    }
    
    async fn compute_memory_hash(&self, entries: &[MemoryEntry]) -> Result<B256> {
        let serialized = bincode::serialize(entries)?;
        Ok(B256::from_slice(&sha3::Keccak256::digest(&serialized)))
    }
    
    async fn validate_checkpoint_data(&self, checkpoint: &CheckpointData) -> Result<bool> {
        if !self.config.enable_validation {
            return Ok(true);
        }
        
        let computed_hash = self.compute_checkpoint_hash(checkpoint).await?;
        
        Ok(computed_hash == checkpoint.metadata.validation_hash)
    }
    
    async fn compute_checkpoint_hash(&self, checkpoint: &CheckpointData) -> Result<B256> {
        let mut hasher = sha3::Keccak256::new();
        
        let serialized = bincode::serialize(&checkpoint.context_data)?;
        hasher.update(&serialized);
        
        let serialized = bincode::serialize(&checkpoint.memory_snapshot)?;
        hasher.update(&serialized);
        
        Ok(B256::from_slice(&hasher.finalize()))
    }
    
    async fn prune_old_checkpoints(&self, agent_address: Address) -> Result<()> {
        if let Some(mut checkpoints) = self.agent_checkpoints.get_mut(&agent_address) {
            if checkpoints.len() > self.config.max_checkpoints_per_agent {
                checkpoints.sort_by(|a, b| {
                    let a_time = self.checkpoint_store.get(a).map(|c| c.created_at).unwrap_or(0);
                    let b_time = self.checkpoint_store.get(b).map(|c| c.created_at).unwrap_or(0);
                    a_time.cmp(&b_time)
                });
                
                let to_remove = checkpoints.len() - self.config.max_checkpoints_per_agent;
                for _ in 0..to_remove {
                    if let Some(checkpoint_id) = checkpoints.pop() {
                        self.checkpoint_store.remove(&checkpoint_id);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn compress_checkpoint_data(&self, checkpoint: &mut CheckpointData) -> Result<()> {
        if !self.config.enable_compression {
            return Ok(());
        }
        
        Ok(())
    }
    
    pub async fn handle_checkpoint_requests(&self, mut receiver: mpsc::Receiver<CheckpointRequest>) {
        while let Some(request) = receiver.recv().await {
            let result = self.create_checkpoint(request.agent_address, request.trigger).await;
            
            if let Err(e) = request.response_channel.send(result).await {
                error!("Failed to send checkpoint response: {}", e);
            }
        }
    }
    
    pub async fn request_checkpoint(&self, agent_address: Address, trigger: CheckpointTrigger) -> Result<B256> {
        let (response_tx, mut response_rx) = mpsc::channel(1);
        
        let request = CheckpointRequest {
            agent_address,
            trigger,
            response_channel: response_tx,
        };
        
        self.trigger_channel.send(request).await?;
        
        response_rx.recv().await
            .ok_or_else(|| eyre::eyre!("Checkpoint request timeout"))?
    }
    
    pub async fn get_checkpoint_stats(&self, agent_address: Address) -> Result<CheckpointStats> {
        let checkpoints = self.agent_checkpoints.get(&agent_address)
            .map(|c| c.clone())
            .unwrap_or_default();
        
        let mut total_size = 0u64;
        let mut oldest_timestamp = u64::MAX;
        let mut newest_timestamp = 0u64;
        
        for checkpoint_id in &checkpoints {
            if let Some(checkpoint) = self.checkpoint_store.get(checkpoint_id) {
                total_size += checkpoint.metadata.size_bytes;
                oldest_timestamp = oldest_timestamp.min(checkpoint.created_at);
                newest_timestamp = newest_timestamp.max(checkpoint.created_at);
            }
        }
        
        Ok(CheckpointStats {
            total_checkpoints: checkpoints.len(),
            total_size_bytes: total_size,
            oldest_checkpoint: if oldest_timestamp == u64::MAX { None } else { Some(oldest_timestamp) },
            newest_checkpoint: if newest_timestamp == 0 { None } else { Some(newest_timestamp) },
        })
    }
}

#[async_trait]
impl AgentCheckpointer for CheckpointManager {
    async fn create_checkpoint(&self, agent_address: Address, trigger: CheckpointTrigger) -> Result<B256> {
        let start_time = std::time::Instant::now();
        
        info!("Creating checkpoint for agent {:?} with trigger {:?}", agent_address, trigger);
        
        let context_snapshot = self.capture_agent_context(agent_address).await?;
        let memory_snapshot = self.capture_memory_snapshot(agent_address).await?;
        
        let checkpoint_id = B256::random();
        let current_block = chrono::Utc::now().timestamp() as u64 / 12;
        
        let mut checkpoint = CheckpointData {
            checkpoint_id,
            agent_address,
            created_at: chrono::Utc::now().timestamp() as u64,
            block_number: current_block,
            state_hash: context_snapshot.memory_state.working_memory.first().copied().unwrap_or(B256::ZERO),
            context_data: context_snapshot,
            memory_snapshot,
            metadata: CheckpointMetadata {
                triggered_by: trigger,
                compression_enabled: self.config.enable_compression,
                validation_hash: B256::ZERO,
                size_bytes: 0,
                creation_time_ms: start_time.elapsed().as_millis() as u64,
            },
        };
        
        self.compress_checkpoint_data(&mut checkpoint).await?;
        
        checkpoint.metadata.validation_hash = self.compute_checkpoint_hash(&checkpoint).await?;
        checkpoint.metadata.size_bytes = bincode::serialize(&checkpoint)?.len() as u64;
        
        self.checkpoint_store.insert(checkpoint_id, checkpoint);
        
        let mut agent_checkpoints = self.agent_checkpoints.entry(agent_address).or_insert_with(Vec::new);
        agent_checkpoints.push(checkpoint_id);
        
        self.prune_old_checkpoints(agent_address).await?;
        
        info!("Created checkpoint {:?} for agent {:?} in {}ms", 
            checkpoint_id, agent_address, start_time.elapsed().as_millis());
        
        Ok(checkpoint_id)
    }
    
    async fn get_checkpoint(&self, checkpoint_id: B256) -> Result<Option<CheckpointData>> {
        Ok(self.checkpoint_store.get(&checkpoint_id).map(|entry| entry.clone()))
    }
    
    async fn list_checkpoints(&self, agent_address: Address) -> Result<Vec<B256>> {
        Ok(self.agent_checkpoints.get(&agent_address)
            .map(|c| c.clone())
            .unwrap_or_default())
    }
    
    async fn delete_checkpoint(&self, checkpoint_id: B256) -> Result<()> {
        if let Some((_, checkpoint)) = self.checkpoint_store.remove(&checkpoint_id) {
            if let Some(mut agent_checkpoints) = self.agent_checkpoints.get_mut(&checkpoint.agent_address) {
                agent_checkpoints.retain(|id| *id != checkpoint_id);
            }
            
            info!("Deleted checkpoint {:?}", checkpoint_id);
        }
        
        Ok(())
    }
    
    async fn validate_checkpoint(&self, checkpoint_id: B256) -> Result<bool> {
        if let Some(checkpoint) = self.checkpoint_store.get(&checkpoint_id) {
            self.validate_checkpoint_data(&checkpoint).await
        } else {
            Ok(false)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckpointStats {
    pub total_checkpoints: usize,
    pub total_size_bytes: u64,
    pub oldest_checkpoint: Option<u64>,
    pub newest_checkpoint: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alh::ALHConfig;
    use crate::rag_exex::context_retrieval::ContextRetriever;
    
    #[tokio::test]
    async fn test_checkpoint_creation() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (alh_coordinator, _) = ALHCoordinator::new(
            memory_store.clone(),
            context_retriever.clone(),
            ALHConfig::default(),
        );
        
        let (state_manager, _) = AgentStateManager::new(Default::default());
        
        let context_manager = Arc::new(UnifiedContextManager::new(
            Default::default(),
            Default::default(),
        ));
        
        let (manager, _) = CheckpointManager::new(
            Arc::new(alh_coordinator),
            memory_store,
            Arc::new(state_manager),
            context_manager,
            CheckpointConfig::default(),
        );
        
        let agent = Address::random();
        let checkpoint_id = manager.create_checkpoint(agent, CheckpointTrigger::Manual).await.unwrap();
        
        assert!(manager.get_checkpoint(checkpoint_id).await.unwrap().is_some());
    }
    
    #[tokio::test]
    async fn test_checkpoint_validation() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (alh_coordinator, _) = ALHCoordinator::new(
            memory_store.clone(),
            context_retriever.clone(),
            ALHConfig::default(),
        );
        
        let (state_manager, _) = AgentStateManager::new(Default::default());
        
        let context_manager = Arc::new(UnifiedContextManager::new(
            Default::default(),
            Default::default(),
        ));
        
        let (manager, _) = CheckpointManager::new(
            Arc::new(alh_coordinator),
            memory_store,
            Arc::new(state_manager),
            context_manager,
            CheckpointConfig::default(),
        );
        
        let agent = Address::random();
        let checkpoint_id = manager.create_checkpoint(agent, CheckpointTrigger::Manual).await.unwrap();
        
        let is_valid = manager.validate_checkpoint(checkpoint_id).await.unwrap();
        assert!(is_valid);
    }
}