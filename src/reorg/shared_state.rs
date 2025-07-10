use alloy::primitives::{Address, B256, U256};
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub snapshot_id: B256,
    pub block_number: u64,
    pub state_root: B256,
    pub agent_states: Vec<(Address, AgentStateData)>,
    pub memory_roots: Vec<(Address, B256)>,
    pub timestamp: u64,
    pub metadata: SnapshotMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateData {
    pub state_hash: B256,
    pub memory_count: u64,
    pub last_action: B256,
    pub checkpoint_id: Option<B256>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub created_by: String,
    pub reason: String,
    pub compressed: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    pub from_block: u64,
    pub to_block: u64,
    pub added_agents: Vec<Address>,
    pub removed_agents: Vec<Address>,
    pub modified_states: Vec<(Address, B256, B256)>,
    pub gas_used: U256,
}

#[derive(Debug, Clone)]
pub struct BlockData {
    pub block_number: u64,
    pub block_hash: B256,
    pub affected_agents: Vec<Address>,
    pub transactions: Vec<B256>,
    pub state_root: B256,
}

pub struct SharedState {
    current_state: Arc<RwLock<CurrentState>>,
    block_cache: Arc<DashMap<u64, BlockData>>,
    state_history: Arc<RwLock<Vec<StateTransition>>>,
    fork_manager: Arc<RwLock<ForkManager>>,
    
    consensus_data: Arc<DashMap<B256, ConsensusInfo>>,
    sync_status: Arc<RwLock<SyncStatus>>,
}

#[derive(Debug, Clone)]
struct CurrentState {
    block_number: u64,
    block_hash: B256,
    state_root: B256,
    agent_count: usize,
    total_memory_size: u64,
}

#[derive(Debug, Clone)]
struct StateTransition {
    from_state: B256,
    to_state: B256,
    block_number: u64,
    transition_type: TransitionType,
    timestamp: u64,
}

#[derive(Debug, Clone)]
enum TransitionType {
    Normal,
    Reorg { depth: u64 },
    Fork { fork_id: B256 },
    Merge { merged_fork: B256 },
}

#[derive(Debug)]
struct ForkManager {
    active_forks: Vec<Fork>,
    fork_choice_rule: ForkChoiceRule,
}

#[derive(Debug, Clone)]
struct Fork {
    fork_id: B256,
    branched_at: u64,
    head_block: u64,
    total_work: U256,
    validators: Vec<Address>,
}

#[derive(Debug, Clone)]
enum ForkChoiceRule {
    LongestChain,
    Heaviest,
    Custom(String),
}

#[derive(Debug, Clone)]
struct ConsensusInfo {
    block_hash: B256,
    validators: Vec<Address>,
    signatures: Vec<B256>,
    finalized: bool,
}

#[derive(Debug, Clone)]
struct SyncStatus {
    is_syncing: bool,
    highest_block: u64,
    current_block: u64,
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Clone)]
struct PeerInfo {
    peer_id: B256,
    best_block: u64,
    latency_ms: u64,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            current_state: Arc::new(RwLock::new(CurrentState {
                block_number: 0,
                block_hash: B256::ZERO,
                state_root: B256::ZERO,
                agent_count: 0,
                total_memory_size: 0,
            })),
            block_cache: Arc::new(DashMap::new()),
            state_history: Arc::new(RwLock::new(Vec::new())),
            fork_manager: Arc::new(RwLock::new(ForkManager {
                active_forks: Vec::new(),
                fork_choice_rule: ForkChoiceRule::LongestChain,
            })),
            consensus_data: Arc::new(DashMap::new()),
            sync_status: Arc::new(RwLock::new(SyncStatus {
                is_syncing: false,
                highest_block: 0,
                current_block: 0,
                peers: Vec::new(),
            })),
        }
    }
    
    pub async fn get_current_state(&self) -> Result<CurrentState> {
        Ok(self.current_state.read().await.clone())
    }
    
    pub async fn get_block_hash(&self, block_number: u64) -> Result<B256> {
        if let Some(block_data) = self.block_cache.get(&block_number) {
            Ok(block_data.block_hash)
        } else {
            Err(eyre::eyre!("Block {} not found", block_number))
        }
    }
    
    pub async fn get_block_data(&self, block_number: u64) -> Result<BlockData> {
        self.block_cache.get(&block_number)
            .map(|entry| entry.clone())
            .ok_or_else(|| eyre::eyre!("Block data not found"))
    }
    
    pub async fn update_state(
        &self,
        block_number: u64,
        block_hash: B256,
        state_root: B256,
        affected_agents: Vec<Address>,
        transactions: Vec<B256>,
    ) -> Result<()> {
        let mut current = self.current_state.write().await;
        
        let transition = StateTransition {
            from_state: current.state_root,
            to_state: state_root,
            block_number,
            transition_type: TransitionType::Normal,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        current.block_number = block_number;
        current.block_hash = block_hash;
        current.state_root = state_root;
        
        drop(current);
        
        let block_data = BlockData {
            block_number,
            block_hash,
            affected_agents: affected_agents.clone(),
            transactions,
            state_root,
        };
        
        self.block_cache.insert(block_number, block_data);
        self.state_history.write().await.push(transition);
        
        if self.block_cache.len() > 1000 {
            self.prune_old_blocks().await?;
        }
        
        Ok(())
    }
    
    pub async fn create_snapshot(&self, block_number: u64) -> Result<StateSnapshot> {
        let current = self.current_state.read().await;
        
        if block_number > current.block_number {
            return Err(eyre::eyre!("Cannot snapshot future block"));
        }
        
        let mut agent_states = Vec::new();
        let mut memory_roots = Vec::new();
        
        let snapshot = StateSnapshot {
            snapshot_id: B256::random(),
            block_number,
            state_root: current.state_root,
            agent_states,
            memory_roots,
            timestamp: chrono::Utc::now().timestamp() as u64,
            metadata: SnapshotMetadata {
                created_by: "SharedState".to_string(),
                reason: "Regular snapshot".to_string(),
                compressed: false,
                size_bytes: 0,
            },
        };
        
        Ok(snapshot)
    }
    
    pub async fn restore_from_snapshot(&self, snapshot: StateSnapshot) -> Result<()> {
        info!("Restoring from snapshot {:?} at block {}", 
            snapshot.snapshot_id, snapshot.block_number);
        
        let mut current = self.current_state.write().await;
        
        current.block_number = snapshot.block_number;
        current.state_root = snapshot.state_root;
        current.agent_count = snapshot.agent_states.len();
        
        let transition = StateTransition {
            from_state: current.state_root,
            to_state: snapshot.state_root,
            block_number: snapshot.block_number,
            transition_type: TransitionType::Reorg { 
                depth: current.block_number.saturating_sub(snapshot.block_number) 
            },
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        drop(current);
        
        self.state_history.write().await.push(transition);
        
        self.clear_blocks_after(snapshot.block_number).await?;
        
        Ok(())
    }
    
    pub async fn create_fork(&self) -> Result<StateSnapshot> {
        let current = self.current_state.read().await;
        let fork_id = B256::random();
        
        let fork = Fork {
            fork_id,
            branched_at: current.block_number,
            head_block: current.block_number,
            total_work: U256::from(current.block_number),
            validators: Vec::new(),
        };
        
        self.fork_manager.write().await.active_forks.push(fork);
        
        self.create_snapshot(current.block_number).await
    }
    
    pub async fn compute_state_diff(&self, from: u64, to: u64) -> Result<StateDiff> {
        let mut added_agents = Vec::new();
        let mut removed_agents = Vec::new();
        let mut modified_states = Vec::new();
        let mut total_gas = U256::ZERO;
        
        for block_num in from..=to {
            if let Some(block_data) = self.block_cache.get(&block_num) {
                for agent in &block_data.affected_agents {
                    if !added_agents.contains(agent) && !removed_agents.contains(agent) {
                        modified_states.push((*agent, B256::ZERO, B256::random()));
                    }
                }
                total_gas += U256::from(block_data.transactions.len() * 21000);
            }
        }
        
        Ok(StateDiff {
            from_block: from,
            to_block: to,
            added_agents,
            removed_agents,
            modified_states,
            gas_used: total_gas,
        })
    }
    
    pub async fn add_consensus_info(&self, block_hash: B256, info: ConsensusInfo) {
        self.consensus_data.insert(block_hash, info);
    }
    
    pub async fn get_consensus_info(&self, block_hash: B256) -> Option<ConsensusInfo> {
        self.consensus_data.get(&block_hash).map(|entry| entry.clone())
    }
    
    pub async fn update_sync_status(&self, is_syncing: bool, current: u64, highest: u64) {
        let mut status = self.sync_status.write().await;
        status.is_syncing = is_syncing;
        status.current_block = current;
        status.highest_block = highest;
    }
    
    pub async fn add_peer(&self, peer_id: B256, best_block: u64, latency_ms: u64) {
        let mut status = self.sync_status.write().await;
        
        if let Some(peer) = status.peers.iter_mut().find(|p| p.peer_id == peer_id) {
            peer.best_block = best_block;
            peer.latency_ms = latency_ms;
        } else {
            status.peers.push(PeerInfo {
                peer_id,
                best_block,
                latency_ms,
            });
        }
    }
    
    pub async fn get_sync_status(&self) -> SyncStatus {
        self.sync_status.read().await.clone()
    }
    
    async fn prune_old_blocks(&self) -> Result<()> {
        let current_block = self.current_state.read().await.block_number;
        let cutoff = current_block.saturating_sub(1000);
        
        self.block_cache.retain(|&block_num, _| block_num > cutoff);
        
        Ok(())
    }
    
    async fn clear_blocks_after(&self, block_number: u64) -> Result<()> {
        self.block_cache.retain(|&block_num, _| block_num <= block_number);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_shared_state_creation() {
        let state = SharedState::new();
        let current = state.get_current_state().await.unwrap();
        
        assert_eq!(current.block_number, 0);
        assert_eq!(current.block_hash, B256::ZERO);
    }
    
    #[tokio::test]
    async fn test_state_update() {
        let state = SharedState::new();
        
        let block_hash = B256::random();
        let state_root = B256::random();
        
        state.update_state(
            1,
            block_hash,
            state_root,
            vec![Address::random()],
            vec![B256::random()],
        ).await.unwrap();
        
        let current = state.get_current_state().await.unwrap();
        assert_eq!(current.block_number, 1);
        assert_eq!(current.block_hash, block_hash);
        
        let retrieved_hash = state.get_block_hash(1).await.unwrap();
        assert_eq!(retrieved_hash, block_hash);
    }
    
    #[tokio::test]
    async fn test_snapshot_creation() {
        let state = SharedState::new();
        
        state.update_state(
            5,
            B256::random(),
            B256::random(),
            vec![],
            vec![],
        ).await.unwrap();
        
        let snapshot = state.create_snapshot(5).await.unwrap();
        assert_eq!(snapshot.block_number, 5);
    }
    
    #[tokio::test]
    async fn test_sync_status() {
        let state = SharedState::new();
        
        state.update_sync_status(true, 100, 200).await;
        
        let status = state.get_sync_status().await;
        assert!(status.is_syncing);
        assert_eq!(status.current_block, 100);
        assert_eq!(status.highest_block, 200);
    }
}