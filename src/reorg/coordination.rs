use alloy::primitives::{Address, B256, U256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{info, warn, error};

use crate::{
    alh::coordination::{ALHCoordinator, ALHStateUpdate},
    memory_exex::MemoryStore,
    rag_exex::context_retrieval::ContextRetriever,
    shared::{
        agent_state_manager::AgentStateManager,
        communication::{CrossExExCoordinator, CrossExExMessage},
    },
};

use super::shared_state::{SharedState, StateSnapshot, StateDiff};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorgEvent {
    pub event_id: B256,
    pub from_block: u64,
    pub to_block: u64,
    pub common_ancestor: u64,
    pub affected_agents: Vec<Address>,
    pub affected_transactions: Vec<B256>,
    pub timestamp: u64,
    pub severity: ReorgSeverity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReorgSeverity {
    Minor { depth: u64 },
    Major { depth: u64, state_conflicts: usize },
    Critical { depth: u64, consensus_break: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReorgStrategy {
    Rollback {
        to_block: u64,
        preserve_mempool: bool,
    },
    Replay {
        from_snapshot: B256,
        transactions: Vec<B256>,
    },
    Reconcile {
        merge_strategy: MergeStrategy,
        conflict_resolution: ConflictResolution,
    },
    Fork {
        maintain_both: bool,
        primary_chain: ChainSelection,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeStrategy {
    PreferCanonical,
    PreferLocal,
    ConsensusVoting,
    AIArbitration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolution {
    DropConflicting,
    ReprocessAll,
    SelectiveReprocess { threshold: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainSelection {
    LongestChain,
    MostWork,
    HighestScore,
    ConsensusChoice,
}

pub struct ReorgCoordinator {
    shared_state: Arc<SharedState>,
    alh_coordinator: Arc<ALHCoordinator>,
    agent_state_manager: Arc<AgentStateManager>,
    cross_exex_coordinator: Arc<CrossExExCoordinator>,
    memory_store: Arc<MemoryStore>,
    
    snapshot_store: Arc<DashMap<B256, StateSnapshot>>,
    reorg_history: Arc<RwLock<Vec<ReorgEvent>>>,
    active_reorgs: Arc<DashMap<B256, ActiveReorg>>,
    
    event_broadcaster: broadcast::Sender<ReorgEvent>,
    command_channel: mpsc::Sender<ReorgCommand>,
    
    config: ReorgConfig,
}

#[derive(Debug)]
struct ActiveReorg {
    event: ReorgEvent,
    strategy: ReorgStrategy,
    start_time: std::time::Instant,
    affected_states: Vec<StateDiff>,
    rollback_points: Vec<RollbackPoint>,
}

#[derive(Debug, Clone)]
struct RollbackPoint {
    block_number: u64,
    state_hash: B256,
    agent_states: Vec<(Address, B256)>,
    memory_snapshot: B256,
}

#[derive(Debug)]
enum ReorgCommand {
    InitiateReorg { event: ReorgEvent, strategy: ReorgStrategy },
    CompleteReorg { event_id: B256, success: bool },
    AbortReorg { event_id: B256, reason: String },
}

#[derive(Debug, Clone)]
pub struct ReorgConfig {
    pub max_reorg_depth: u64,
    pub snapshot_interval: u64,
    pub consensus_threshold: f64,
    pub enable_auto_recovery: bool,
    pub max_active_reorgs: usize,
    pub reorg_timeout_ms: u64,
}

impl Default for ReorgConfig {
    fn default() -> Self {
        Self {
            max_reorg_depth: 100,
            snapshot_interval: 100,
            consensus_threshold: 0.67,
            enable_auto_recovery: true,
            max_active_reorgs: 3,
            reorg_timeout_ms: 30000,
        }
    }
}

impl ReorgCoordinator {
    pub fn new(
        shared_state: Arc<SharedState>,
        alh_coordinator: Arc<ALHCoordinator>,
        agent_state_manager: Arc<AgentStateManager>,
        cross_exex_coordinator: Arc<CrossExExCoordinator>,
        memory_store: Arc<MemoryStore>,
        config: ReorgConfig,
    ) -> (Self, broadcast::Receiver<ReorgEvent>, mpsc::Receiver<ReorgCommand>) {
        let (event_tx, event_rx) = broadcast::channel(100);
        let (command_tx, command_rx) = mpsc::channel(100);
        
        let coordinator = Self {
            shared_state,
            alh_coordinator,
            agent_state_manager,
            cross_exex_coordinator,
            memory_store,
            snapshot_store: Arc::new(DashMap::new()),
            reorg_history: Arc::new(RwLock::new(Vec::new())),
            active_reorgs: Arc::new(DashMap::new()),
            event_broadcaster: event_tx,
            command_channel: command_tx,
            config,
        };
        
        (coordinator, event_rx, command_rx)
    }
    
    pub async fn detect_reorg(&self, new_block: u64, new_hash: B256) -> Result<Option<ReorgEvent>> {
        let current_state = self.shared_state.get_current_state().await?;
        
        if current_state.block_number >= new_block {
            let stored_hash = self.shared_state.get_block_hash(new_block).await?;
            
            if stored_hash != new_hash {
                info!("Reorg detected at block {}: {} != {}", new_block, stored_hash, new_hash);
                
                let common_ancestor = self.find_common_ancestor(new_block, new_hash).await?;
                let affected_data = self.analyze_affected_data(common_ancestor, current_state.block_number).await?;
                
                let severity = self.calculate_severity(
                    current_state.block_number - common_ancestor,
                    affected_data.1.len(),
                );
                
                let event = ReorgEvent {
                    event_id: B256::random(),
                    from_block: current_state.block_number,
                    to_block: new_block,
                    common_ancestor,
                    affected_agents: affected_data.0,
                    affected_transactions: affected_data.1,
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    severity,
                };
                
                return Ok(Some(event));
            }
        }
        
        Ok(None)
    }
    
    pub async fn handle_reorg(&self, event: ReorgEvent) -> Result<()> {
        if event.from_block - event.common_ancestor > self.config.max_reorg_depth {
            error!("Reorg depth {} exceeds maximum {}", 
                event.from_block - event.common_ancestor, 
                self.config.max_reorg_depth
            );
            return Err(eyre::eyre!("Reorg depth exceeds maximum allowed"));
        }
        
        let strategy = self.determine_strategy(&event).await?;
        
        info!("Handling reorg {:?} with strategy {:?}", event.event_id, strategy);
        
        self.command_channel.send(ReorgCommand::InitiateReorg {
            event: event.clone(),
            strategy: strategy.clone(),
        }).await?;
        
        let active_reorg = ActiveReorg {
            event: event.clone(),
            strategy: strategy.clone(),
            start_time: std::time::Instant::now(),
            affected_states: Vec::new(),
            rollback_points: Vec::new(),
        };
        
        self.active_reorgs.insert(event.event_id, active_reorg);
        
        let result = match strategy {
            ReorgStrategy::Rollback { to_block, preserve_mempool } => {
                self.execute_rollback(event.event_id, to_block, preserve_mempool).await
            }
            ReorgStrategy::Replay { from_snapshot, transactions } => {
                self.execute_replay(event.event_id, from_snapshot, transactions).await
            }
            ReorgStrategy::Reconcile { merge_strategy, conflict_resolution } => {
                self.execute_reconcile(event.event_id, merge_strategy, conflict_resolution).await
            }
            ReorgStrategy::Fork { maintain_both, primary_chain } => {
                self.execute_fork(event.event_id, maintain_both, primary_chain).await
            }
        };
        
        match result {
            Ok(_) => {
                self.command_channel.send(ReorgCommand::CompleteReorg {
                    event_id: event.event_id,
                    success: true,
                }).await?;
                
                self.event_broadcaster.send(event.clone())?;
                self.reorg_history.write().await.push(event);
            }
            Err(e) => {
                error!("Reorg handling failed: {}", e);
                self.command_channel.send(ReorgCommand::AbortReorg {
                    event_id: event.event_id,
                    reason: e.to_string(),
                }).await?;
            }
        }
        
        self.active_reorgs.remove(&event.event_id);
        
        Ok(())
    }
    
    async fn execute_rollback(
        &self,
        event_id: B256,
        to_block: u64,
        preserve_mempool: bool,
    ) -> Result<()> {
        info!("Executing rollback to block {}", to_block);
        
        if let Some(snapshot) = self.find_nearest_snapshot(to_block).await? {
            self.shared_state.restore_from_snapshot(snapshot.clone()).await?;
            
            let affected_agents = self.get_affected_agents(event_id).await?;
            for agent in affected_agents {
                self.agent_state_manager.reset_agent_state(agent).await?;
                
                if preserve_mempool {
                    self.preserve_agent_mempool(agent).await?;
                }
            }
            
            self.broadcast_state_update(CrossExExMessage::ReorgNotification {
                from_block: self.shared_state.get_current_state().await?.block_number,
                to_block,
                common_ancestor: to_block,
            }).await?;
            
            Ok(())
        } else {
            Err(eyre::eyre!("No snapshot found for rollback"))
        }
    }
    
    async fn execute_replay(
        &self,
        event_id: B256,
        from_snapshot: B256,
        transactions: Vec<B256>,
    ) -> Result<()> {
        info!("Executing replay from snapshot {:?}", from_snapshot);
        
        if let Some(snapshot) = self.snapshot_store.get(&from_snapshot) {
            self.shared_state.restore_from_snapshot(snapshot.clone()).await?;
            
            for tx_hash in transactions {
                if let Some(tx) = self.get_transaction(tx_hash).await? {
                    match self.replay_transaction(tx).await {
                        Ok(_) => info!("Replayed transaction {:?}", tx_hash),
                        Err(e) => warn!("Failed to replay transaction {:?}: {}", tx_hash, e),
                    }
                }
            }
            
            self.update_active_reorg_progress(event_id, 1.0).await;
            
            Ok(())
        } else {
            Err(eyre::eyre!("Snapshot not found"))
        }
    }
    
    async fn execute_reconcile(
        &self,
        event_id: B256,
        merge_strategy: MergeStrategy,
        conflict_resolution: ConflictResolution,
    ) -> Result<()> {
        info!("Executing reconciliation with strategy {:?}", merge_strategy);
        
        let conflicts = self.identify_conflicts(event_id).await?;
        
        for conflict in conflicts {
            let resolution = match &conflict_resolution {
                ConflictResolution::DropConflicting => {
                    self.drop_conflicting_state(conflict).await?
                }
                ConflictResolution::ReprocessAll => {
                    self.reprocess_conflict(conflict).await?
                }
                ConflictResolution::SelectiveReprocess { threshold } => {
                    if conflict.severity > *threshold {
                        self.reprocess_conflict(conflict).await?
                    } else {
                        self.drop_conflicting_state(conflict).await?
                    }
                }
            };
            
            self.apply_resolution(resolution).await?;
        }
        
        match merge_strategy {
            MergeStrategy::ConsensusVoting => {
                self.consensus_merge(event_id).await?;
            }
            MergeStrategy::AIArbitration => {
                self.ai_arbitrated_merge(event_id).await?;
            }
            _ => {}
        }
        
        Ok(())
    }
    
    async fn execute_fork(
        &self,
        event_id: B256,
        maintain_both: bool,
        primary_chain: ChainSelection,
    ) -> Result<()> {
        info!("Executing fork with primary chain selection {:?}", primary_chain);
        
        if maintain_both {
            let fork_state = self.shared_state.create_fork().await?;
            self.manage_dual_chains(event_id, fork_state).await?;
        } else {
            let selected_chain = match primary_chain {
                ChainSelection::LongestChain => self.select_longest_chain().await?,
                ChainSelection::MostWork => self.select_most_work_chain().await?,
                ChainSelection::HighestScore => self.select_highest_score_chain().await?,
                ChainSelection::ConsensusChoice => self.select_consensus_chain().await?,
            };
            
            self.switch_to_chain(selected_chain).await?;
        }
        
        Ok(())
    }
    
    pub async fn create_snapshot(&self, block_number: u64) -> Result<B256> {
        let snapshot = self.shared_state.create_snapshot(block_number).await?;
        let snapshot_id = snapshot.snapshot_id;
        
        self.snapshot_store.insert(snapshot_id, snapshot);
        
        if self.snapshot_store.len() > 100 {
            self.prune_old_snapshots().await?;
        }
        
        Ok(snapshot_id)
    }
    
    async fn find_common_ancestor(&self, block: u64, hash: B256) -> Result<u64> {
        let mut current = block;
        
        while current > 0 {
            if let Ok(stored_hash) = self.shared_state.get_block_hash(current).await {
                if stored_hash == hash {
                    return Ok(current);
                }
            }
            current = current.saturating_sub(1);
        }
        
        Ok(0)
    }
    
    async fn analyze_affected_data(&self, from: u64, to: u64) -> Result<(Vec<Address>, Vec<B256>)> {
        let mut agents = Vec::new();
        let mut transactions = Vec::new();
        
        for block in from..=to {
            if let Ok(block_data) = self.shared_state.get_block_data(block).await {
                agents.extend(block_data.affected_agents);
                transactions.extend(block_data.transactions);
            }
        }
        
        agents.sort();
        agents.dedup();
        
        Ok((agents, transactions))
    }
    
    fn calculate_severity(&self, depth: u64, conflicts: usize) -> ReorgSeverity {
        if depth > 50 || conflicts > 100 {
            ReorgSeverity::Critical {
                depth,
                consensus_break: depth > 100,
            }
        } else if depth > 10 || conflicts > 20 {
            ReorgSeverity::Major {
                depth,
                state_conflicts: conflicts,
            }
        } else {
            ReorgSeverity::Minor { depth }
        }
    }
    
    async fn determine_strategy(&self, event: &ReorgEvent) -> Result<ReorgStrategy> {
        match &event.severity {
            ReorgSeverity::Minor { .. } => {
                Ok(ReorgStrategy::Rollback {
                    to_block: event.common_ancestor,
                    preserve_mempool: true,
                })
            }
            ReorgSeverity::Major { .. } => {
                Ok(ReorgStrategy::Reconcile {
                    merge_strategy: MergeStrategy::ConsensusVoting,
                    conflict_resolution: ConflictResolution::SelectiveReprocess { threshold: 0.5 },
                })
            }
            ReorgSeverity::Critical { consensus_break, .. } => {
                if *consensus_break {
                    Ok(ReorgStrategy::Fork {
                        maintain_both: true,
                        primary_chain: ChainSelection::ConsensusChoice,
                    })
                } else {
                    Ok(ReorgStrategy::Replay {
                        from_snapshot: self.find_best_snapshot(event.common_ancestor).await?,
                        transactions: event.affected_transactions.clone(),
                    })
                }
            }
        }
    }
    
    async fn find_nearest_snapshot(&self, block: u64) -> Result<Option<StateSnapshot>> {
        let mut best_snapshot = None;
        let mut best_distance = u64::MAX;
        
        for entry in self.snapshot_store.iter() {
            if entry.block_number <= block {
                let distance = block - entry.block_number;
                if distance < best_distance {
                    best_distance = distance;
                    best_snapshot = Some(entry.value().clone());
                }
            }
        }
        
        Ok(best_snapshot)
    }
    
    async fn find_best_snapshot(&self, block: u64) -> Result<B256> {
        self.find_nearest_snapshot(block).await?
            .map(|s| s.snapshot_id)
            .ok_or_else(|| eyre::eyre!("No snapshot found"))
    }
    
    async fn get_affected_agents(&self, event_id: B256) -> Result<Vec<Address>> {
        if let Some(active) = self.active_reorgs.get(&event_id) {
            Ok(active.event.affected_agents.clone())
        } else {
            Ok(Vec::new())
        }
    }
    
    async fn preserve_agent_mempool(&self, _agent: Address) -> Result<()> {
        Ok(())
    }
    
    async fn broadcast_state_update(&self, message: CrossExExMessage) -> Result<()> {
        self.cross_exex_coordinator.broadcast(message).await
    }
    
    async fn get_transaction(&self, _hash: B256) -> Result<Option<reth_primitives::TransactionSigned>> {
        Ok(None)
    }
    
    async fn replay_transaction(&self, _tx: reth_primitives::TransactionSigned) -> Result<()> {
        Ok(())
    }
    
    async fn update_active_reorg_progress(&self, _event_id: B256, _progress: f64) {
    }
    
    async fn identify_conflicts(&self, _event_id: B256) -> Result<Vec<StateConflict>> {
        Ok(Vec::new())
    }
    
    async fn drop_conflicting_state(&self, _conflict: StateConflict) -> Result<ConflictResolution> {
        Ok(ConflictResolution::Dropped)
    }
    
    async fn reprocess_conflict(&self, _conflict: StateConflict) -> Result<ConflictResolution> {
        Ok(ConflictResolution::Reprocessed)
    }
    
    async fn apply_resolution(&self, _resolution: ConflictResolution) -> Result<()> {
        Ok(())
    }
    
    async fn consensus_merge(&self, _event_id: B256) -> Result<()> {
        Ok(())
    }
    
    async fn ai_arbitrated_merge(&self, _event_id: B256) -> Result<()> {
        Ok(())
    }
    
    async fn manage_dual_chains(&self, _event_id: B256, _fork_state: StateSnapshot) -> Result<()> {
        Ok(())
    }
    
    async fn select_longest_chain(&self) -> Result<ChainIdentifier> {
        Ok(ChainIdentifier::default())
    }
    
    async fn select_most_work_chain(&self) -> Result<ChainIdentifier> {
        Ok(ChainIdentifier::default())
    }
    
    async fn select_highest_score_chain(&self) -> Result<ChainIdentifier> {
        Ok(ChainIdentifier::default())
    }
    
    async fn select_consensus_chain(&self) -> Result<ChainIdentifier> {
        Ok(ChainIdentifier::default())
    }
    
    async fn switch_to_chain(&self, _chain: ChainIdentifier) -> Result<()> {
        Ok(())
    }
    
    async fn prune_old_snapshots(&self) -> Result<()> {
        let current_block = self.shared_state.get_current_state().await?.block_number;
        let cutoff = current_block.saturating_sub(self.config.snapshot_interval * 10);
        
        self.snapshot_store.retain(|_, snapshot| snapshot.block_number > cutoff);
        
        Ok(())
    }
}

#[derive(Debug)]
struct StateConflict {
    agent: Address,
    local_state: B256,
    remote_state: B256,
    severity: f64,
}

#[derive(Debug)]
enum ConflictResolution {
    Dropped,
    Reprocessed,
    Merged(B256),
}

#[derive(Debug, Default)]
struct ChainIdentifier {
    chain_id: B256,
    head_block: u64,
    total_work: U256,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_reorg_detection() {
        let shared_state = Arc::new(SharedState::new());
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (alh_coordinator, _) = ALHCoordinator::new(
            memory_store.clone(),
            context_retriever,
            Default::default(),
        );
        
        let (reorg_coordinator, _, _) = ReorgCoordinator::new(
            shared_state,
            Arc::new(alh_coordinator),
            Arc::new(AgentStateManager::new(Default::default()).0),
            Arc::new(CrossExExCoordinator::new()),
            memory_store,
            ReorgConfig::default(),
        );
        
        let event = reorg_coordinator.detect_reorg(100, B256::random()).await.unwrap();
        assert!(event.is_none());
    }
    
    #[tokio::test]
    async fn test_snapshot_creation() {
        let shared_state = Arc::new(SharedState::new());
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (alh_coordinator, _) = ALHCoordinator::new(
            memory_store.clone(),
            context_retriever,
            Default::default(),
        );
        
        let (reorg_coordinator, _, _) = ReorgCoordinator::new(
            shared_state,
            Arc::new(alh_coordinator),
            Arc::new(AgentStateManager::new(Default::default()).0),
            Arc::new(CrossExExCoordinator::new()),
            memory_store,
            ReorgConfig::default(),
        );
        
        let snapshot_id = reorg_coordinator.create_snapshot(100).await.unwrap();
        assert!(reorg_coordinator.snapshot_store.contains_key(&snapshot_id));
    }
}