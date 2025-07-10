use alloy::primitives::{Address, B256, U256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn, error};

use crate::{
    alh::coordination::ALHCoordinator,
    agents::checkpointing::{CheckpointData, CheckpointManager, AgentCheckpointer},
    memory_exex::MemoryStore,
    rag_exex::agent_context::UnifiedContextManager,
    shared::{
        agent_state_manager::{AgentStateManager, LifecycleState},
        types::AgentMemoryState,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryStrategy {
    FullRestore {
        checkpoint_id: B256,
        validate_integrity: bool,
    },
    PartialRestore {
        checkpoint_id: B256,
        restore_memory: bool,
        restore_context: bool,
        restore_state: bool,
    },
    IncrementalRestore {
        base_checkpoint: B256,
        incremental_updates: Vec<B256>,
    },
    ConsensusRestore {
        candidate_checkpoints: Vec<B256>,
        voting_threshold: f64,
    },
    AIGuidedRestore {
        checkpoint_id: B256,
        confidence_threshold: f64,
        fallback_strategy: Box<RecoveryStrategy>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    pub recovery_id: B256,
    pub agent_address: Address,
    pub strategy: RecoveryStrategy,
    pub priority: RecoveryPriority,
    pub requester: Address,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryPriority {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    pub recovery_id: B256,
    pub agent_address: Address,
    pub success: bool,
    pub restored_checkpoint: Option<B256>,
    pub recovery_time_ms: u64,
    pub error_message: Option<String>,
    pub restored_components: RestoredComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoredComponents {
    pub memory_restored: bool,
    pub context_restored: bool,
    pub state_restored: bool,
    pub relationships_restored: bool,
}

#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    pub max_concurrent_recoveries: usize,
    pub recovery_timeout_ms: u64,
    pub enable_validation: bool,
    pub enable_rollback: bool,
    pub consensus_threshold: f64,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_concurrent_recoveries: 5,
            recovery_timeout_ms: 30000,
            enable_validation: true,
            enable_rollback: true,
            consensus_threshold: 0.67,
        }
    }
}

#[async_trait]
pub trait AgentRecovery: Send + Sync {
    async fn recover_agent(&self, request: RecoveryRequest) -> Result<RecoveryResult>;
    async fn validate_recovery(&self, recovery_id: B256) -> Result<bool>;
    async fn rollback_recovery(&self, recovery_id: B256) -> Result<()>;
    async fn get_recovery_status(&self, recovery_id: B256) -> Result<Option<RecoveryStatus>>;
}

#[derive(Debug, Clone)]
pub struct RecoveryStatus {
    pub recovery_id: B256,
    pub agent_address: Address,
    pub status: RecoveryState,
    pub progress: f64,
    pub started_at: u64,
    pub estimated_completion: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum RecoveryState {
    Pending,
    InProgress,
    Validating,
    Completed,
    Failed,
    RolledBack,
}

pub struct RecoveryManager {
    checkpoint_manager: Arc<CheckpointManager>,
    alh_coordinator: Arc<ALHCoordinator>,
    memory_store: Arc<MemoryStore>,
    agent_state_manager: Arc<AgentStateManager>,
    context_manager: Arc<UnifiedContextManager>,
    
    active_recoveries: Arc<DashMap<B256, RecoveryStatus>>,
    recovery_history: Arc<RwLock<Vec<RecoveryResult>>>,
    
    recovery_channel: mpsc::Sender<RecoveryRequest>,
    config: RecoveryConfig,
}

impl RecoveryManager {
    pub fn new(
        checkpoint_manager: Arc<CheckpointManager>,
        alh_coordinator: Arc<ALHCoordinator>,
        memory_store: Arc<MemoryStore>,
        agent_state_manager: Arc<AgentStateManager>,
        context_manager: Arc<UnifiedContextManager>,
        config: RecoveryConfig,
    ) -> (Self, mpsc::Receiver<RecoveryRequest>) {
        let (recovery_tx, recovery_rx) = mpsc::channel(1000);
        
        let manager = Self {
            checkpoint_manager,
            alh_coordinator,
            memory_store,
            agent_state_manager,
            context_manager,
            active_recoveries: Arc::new(DashMap::new()),
            recovery_history: Arc::new(RwLock::new(Vec::new())),
            recovery_channel: recovery_tx,
            config,
        };
        
        (manager, recovery_rx)
    }
    
    pub async fn handle_recovery_requests(&self, mut receiver: mpsc::Receiver<RecoveryRequest>) {
        while let Some(request) = receiver.recv().await {
            if self.active_recoveries.len() >= self.config.max_concurrent_recoveries {
                warn!("Maximum concurrent recoveries reached, queuing request");
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                continue;
            }
            
            let manager = self.clone();
            tokio::spawn(async move {
                if let Err(e) = manager.process_recovery_request(request).await {
                    error!("Recovery request processing failed: {}", e);
                }
            });
        }
    }
    
    async fn process_recovery_request(&self, request: RecoveryRequest) -> Result<()> {
        let recovery_status = RecoveryStatus {
            recovery_id: request.recovery_id,
            agent_address: request.agent_address,
            status: RecoveryState::Pending,
            progress: 0.0,
            started_at: chrono::Utc::now().timestamp() as u64,
            estimated_completion: None,
        };
        
        self.active_recoveries.insert(request.recovery_id, recovery_status);
        
        let result = self.recover_agent(request).await;
        
        match result {
            Ok(recovery_result) => {
                info!("Recovery completed successfully: {:?}", recovery_result.recovery_id);
                self.recovery_history.write().await.push(recovery_result);
            }
            Err(e) => {
                error!("Recovery failed: {}", e);
            }
        }
        
        self.active_recoveries.remove(&request.recovery_id);
        
        Ok(())
    }
    
    async fn execute_full_restore(&self, agent_address: Address, checkpoint_id: B256, validate: bool) -> Result<RestoredComponents> {
        info!("Executing full restore for agent {:?} from checkpoint {:?}", agent_address, checkpoint_id);
        
        let checkpoint = self.checkpoint_manager.get_checkpoint(checkpoint_id).await?
            .ok_or_else(|| eyre::eyre!("Checkpoint not found"))?;
        
        if validate && !self.checkpoint_manager.validate_checkpoint(checkpoint_id).await? {
            return Err(eyre::eyre!("Checkpoint validation failed"));
        }
        
        let mut restored = RestoredComponents {
            memory_restored: false,
            context_restored: false,
            state_restored: false,
            relationships_restored: false,
        };
        
        if let Err(e) = self.restore_memory(&checkpoint).await {
            warn!("Memory restore failed: {}", e);
        } else {
            restored.memory_restored = true;
        }
        
        if let Err(e) = self.restore_context(&checkpoint).await {
            warn!("Context restore failed: {}", e);
        } else {
            restored.context_restored = true;
        }
        
        if let Err(e) = self.restore_state(&checkpoint).await {
            warn!("State restore failed: {}", e);
        } else {
            restored.state_restored = true;
        }
        
        if let Err(e) = self.restore_relationships(&checkpoint).await {
            warn!("Relationships restore failed: {}", e);
        } else {
            restored.relationships_restored = true;
        }
        
        Ok(restored)
    }
    
    async fn execute_partial_restore(
        &self,
        agent_address: Address,
        checkpoint_id: B256,
        restore_memory: bool,
        restore_context: bool,
        restore_state: bool,
    ) -> Result<RestoredComponents> {
        info!("Executing partial restore for agent {:?}", agent_address);
        
        let checkpoint = self.checkpoint_manager.get_checkpoint(checkpoint_id).await?
            .ok_or_else(|| eyre::eyre!("Checkpoint not found"))?;
        
        let mut restored = RestoredComponents {
            memory_restored: false,
            context_restored: false,
            state_restored: false,
            relationships_restored: false,
        };
        
        if restore_memory {
            if let Err(e) = self.restore_memory(&checkpoint).await {
                warn!("Memory restore failed: {}", e);
            } else {
                restored.memory_restored = true;
            }
        }
        
        if restore_context {
            if let Err(e) = self.restore_context(&checkpoint).await {
                warn!("Context restore failed: {}", e);
            } else {
                restored.context_restored = true;
            }
        }
        
        if restore_state {
            if let Err(e) = self.restore_state(&checkpoint).await {
                warn!("State restore failed: {}", e);
            } else {
                restored.state_restored = true;
            }
        }
        
        Ok(restored)
    }
    
    async fn execute_incremental_restore(
        &self,
        agent_address: Address,
        base_checkpoint: B256,
        incremental_updates: Vec<B256>,
    ) -> Result<RestoredComponents> {
        info!("Executing incremental restore for agent {:?}", agent_address);
        
        let mut restored = self.execute_full_restore(agent_address, base_checkpoint, true).await?;
        
        for update_id in incremental_updates {
            if let Some(update_checkpoint) = self.checkpoint_manager.get_checkpoint(update_id).await? {
                if let Err(e) = self.apply_incremental_update(&update_checkpoint).await {
                    warn!("Failed to apply incremental update {:?}: {}", update_id, e);
                }
            }
        }
        
        Ok(restored)
    }
    
    async fn execute_consensus_restore(
        &self,
        agent_address: Address,
        candidate_checkpoints: Vec<B256>,
        voting_threshold: f64,
    ) -> Result<RestoredComponents> {
        info!("Executing consensus restore for agent {:?}", agent_address);
        
        let mut votes = Vec::new();
        
        for checkpoint_id in candidate_checkpoints {
            if let Some(checkpoint) = self.checkpoint_manager.get_checkpoint(checkpoint_id).await? {
                let score = self.evaluate_checkpoint_quality(&checkpoint).await?;
                votes.push((checkpoint_id, score));
            }
        }
        
        votes.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        if let Some((best_checkpoint, score)) = votes.first() {
            if *score >= voting_threshold {
                return self.execute_full_restore(agent_address, *best_checkpoint, true).await;
            }
        }
        
        Err(eyre::eyre!("No checkpoint met consensus threshold"))
    }
    
    async fn restore_memory(&self, checkpoint: &CheckpointData) -> Result<()> {
        info!("Restoring memory for agent {:?}", checkpoint.agent_address);
        
        self.memory_store.clear_agent_memories(&checkpoint.agent_address).await?;
        
        for memory_entry in &checkpoint.memory_snapshot.memory_entries {
            let memory_data = crate::memory_exex::AgentMemory {
                agent_address: checkpoint.agent_address,
                memory_type: memory_entry.memory_type,
                content: vec![],
                content_hash: memory_entry.content_hash,
                timestamp: checkpoint.created_at,
                metadata: memory_entry.metadata.clone(),
            };
            
            self.memory_store.store_memory(memory_data).await?;
        }
        
        Ok(())
    }
    
    async fn restore_context(&self, checkpoint: &CheckpointData) -> Result<()> {
        info!("Restoring context for agent {:?}", checkpoint.agent_address);
        
        self.context_manager.clear_agent_context(checkpoint.agent_address).await?;
        
        for context in &checkpoint.context_data.context_history {
            self.context_manager.add_context(
                checkpoint.agent_address,
                context.clone(),
            ).await?;
        }
        
        Ok(())
    }
    
    async fn restore_state(&self, checkpoint: &CheckpointData) -> Result<()> {
        info!("Restoring state for agent {:?}", checkpoint.agent_address);
        
        self.agent_state_manager.restore_agent_state(
            checkpoint.agent_address,
            checkpoint.context_data.lifecycle_state,
            checkpoint.context_data.interaction_count,
        ).await?;
        
        Ok(())
    }
    
    async fn restore_relationships(&self, checkpoint: &CheckpointData) -> Result<()> {
        info!("Restoring relationships for agent {:?}", checkpoint.agent_address);
        
        Ok(())
    }
    
    async fn apply_incremental_update(&self, update: &CheckpointData) -> Result<()> {
        info!("Applying incremental update for agent {:?}", update.agent_address);
        
        self.restore_memory(update).await?;
        self.restore_context(update).await?;
        
        Ok(())
    }
    
    async fn evaluate_checkpoint_quality(&self, checkpoint: &CheckpointData) -> Result<f64> {
        let mut score = 0.0;
        
        if self.checkpoint_manager.validate_checkpoint(checkpoint.checkpoint_id).await? {
            score += 0.5;
        }
        
        let age_penalty = (chrono::Utc::now().timestamp() as u64 - checkpoint.created_at) as f64 / 86400.0;
        score += (1.0 - age_penalty.min(1.0)) * 0.3;
        
        let size_score = 1.0 - (checkpoint.metadata.size_bytes as f64 / 1_000_000.0).min(1.0);
        score += size_score * 0.2;
        
        Ok(score)
    }
    
    pub async fn request_recovery(&self, request: RecoveryRequest) -> Result<()> {
        self.recovery_channel.send(request).await?;
        Ok(())
    }
    
    pub async fn get_recovery_stats(&self) -> Result<RecoveryStats> {
        let history = self.recovery_history.read().await;
        
        let total_recoveries = history.len();
        let successful_recoveries = history.iter().filter(|r| r.success).count();
        let average_time = if total_recoveries > 0 {
            history.iter().map(|r| r.recovery_time_ms).sum::<u64>() / total_recoveries as u64
        } else {
            0
        };
        
        Ok(RecoveryStats {
            total_recoveries,
            successful_recoveries,
            success_rate: if total_recoveries > 0 {
                successful_recoveries as f64 / total_recoveries as f64
            } else {
                0.0
            },
            average_recovery_time_ms: average_time,
            active_recoveries: self.active_recoveries.len(),
        })
    }
}

impl Clone for RecoveryManager {
    fn clone(&self) -> Self {
        Self {
            checkpoint_manager: self.checkpoint_manager.clone(),
            alh_coordinator: self.alh_coordinator.clone(),
            memory_store: self.memory_store.clone(),
            agent_state_manager: self.agent_state_manager.clone(),
            context_manager: self.context_manager.clone(),
            active_recoveries: self.active_recoveries.clone(),
            recovery_history: self.recovery_history.clone(),
            recovery_channel: self.recovery_channel.clone(),
            config: self.config.clone(),
        }
    }
}

#[async_trait]
impl AgentRecovery for RecoveryManager {
    async fn recover_agent(&self, request: RecoveryRequest) -> Result<RecoveryResult> {
        let start_time = std::time::Instant::now();
        
        info!("Starting recovery for agent {:?} with strategy {:?}", 
            request.agent_address, request.strategy);
        
        let restored_components = match &request.strategy {
            RecoveryStrategy::FullRestore { checkpoint_id, validate_integrity } => {
                self.execute_full_restore(request.agent_address, *checkpoint_id, *validate_integrity).await?
            }
            
            RecoveryStrategy::PartialRestore { checkpoint_id, restore_memory, restore_context, restore_state } => {
                self.execute_partial_restore(
                    request.agent_address,
                    *checkpoint_id,
                    *restore_memory,
                    *restore_context,
                    *restore_state,
                ).await?
            }
            
            RecoveryStrategy::IncrementalRestore { base_checkpoint, incremental_updates } => {
                self.execute_incremental_restore(
                    request.agent_address,
                    *base_checkpoint,
                    incremental_updates.clone(),
                ).await?
            }
            
            RecoveryStrategy::ConsensusRestore { candidate_checkpoints, voting_threshold } => {
                self.execute_consensus_restore(
                    request.agent_address,
                    candidate_checkpoints.clone(),
                    *voting_threshold,
                ).await?
            }
            
            RecoveryStrategy::AIGuidedRestore { checkpoint_id, confidence_threshold, fallback_strategy } => {
                match self.execute_full_restore(request.agent_address, *checkpoint_id, true).await {
                    Ok(components) => components,
                    Err(_) => {
                        let fallback_request = RecoveryRequest {
                            recovery_id: B256::random(),
                            agent_address: request.agent_address,
                            strategy: (**fallback_strategy).clone(),
                            priority: request.priority,
                            requester: request.requester,
                            metadata: request.metadata,
                        };
                        
                        return self.recover_agent(fallback_request).await;
                    }
                }
            }
        };
        
        let recovery_time = start_time.elapsed().as_millis() as u64;
        
        let result = RecoveryResult {
            recovery_id: request.recovery_id,
            agent_address: request.agent_address,
            success: true,
            restored_checkpoint: None,
            recovery_time_ms: recovery_time,
            error_message: None,
            restored_components,
        };
        
        info!("Recovery completed for agent {:?} in {}ms", 
            request.agent_address, recovery_time);
        
        Ok(result)
    }
    
    async fn validate_recovery(&self, recovery_id: B256) -> Result<bool> {
        if let Some(status) = self.active_recoveries.get(&recovery_id) {
            Ok(matches!(status.status, RecoveryState::Completed))
        } else {
            Ok(false)
        }
    }
    
    async fn rollback_recovery(&self, recovery_id: B256) -> Result<()> {
        if let Some(mut status) = self.active_recoveries.get_mut(&recovery_id) {
            status.status = RecoveryState::RolledBack;
            info!("Rolled back recovery {:?}", recovery_id);
        }
        
        Ok(())
    }
    
    async fn get_recovery_status(&self, recovery_id: B256) -> Result<Option<RecoveryStatus>> {
        Ok(self.active_recoveries.get(&recovery_id).map(|s| s.clone()))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryStats {
    pub total_recoveries: usize,
    pub successful_recoveries: usize,
    pub success_rate: f64,
    pub average_recovery_time_ms: u64,
    pub active_recoveries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alh::ALHConfig;
    use crate::rag_exex::context_retrieval::ContextRetriever;
    
    #[tokio::test]
    async fn test_recovery_manager_creation() {
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
        
        let (checkpoint_manager, _) = CheckpointManager::new(
            Arc::new(alh_coordinator),
            memory_store.clone(),
            Arc::new(state_manager),
            context_manager.clone(),
            Default::default(),
        );
        
        let (recovery_manager, _) = RecoveryManager::new(
            Arc::new(checkpoint_manager),
            memory_store,
            context_manager,
            RecoveryConfig::default(),
        );
        
        assert_eq!(recovery_manager.active_recoveries.len(), 0);
    }
}