use alloy::primitives::{Address, B256, U256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn, error};

use crate::{
    memory_exex::{memory_hash::MemoryLatticeHash, MemoryStore, MemoryType},
    rag_exex::context_retrieval::ContextRetriever,
    shared::{
        ai_agent::RoutingDecision,
        types::{AgentContext, AgentMemoryState},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALHQueryRequest {
    pub query_id: B256,
    pub requester: Address,
    pub query_type: ALHQueryType,
    pub timestamp: u64,
    pub priority: u8,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ALHQueryType {
    AgentMemoryState {
        agent_address: Address,
        memory_types: Vec<MemoryType>,
        include_proofs: bool,
    },
    MemoryHashVerification {
        agent_address: Address,
        expected_hash: B256,
        block_number: Option<u64>,
    },
    ContextRetrieval {
        agent_address: Address,
        context_window: u64,
        semantic_filter: Option<String>,
    },
    StateTransition {
        from_state: B256,
        to_state: B256,
        verify_path: bool,
    },
    CrossAgentSync {
        agents: Vec<Address>,
        sync_type: SyncType,
    },
    CheckpointRequest {
        agent_address: Address,
        checkpoint_id: Option<B256>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncType {
    Full,
    Incremental { since_block: u64 },
    Selective { memory_types: Vec<MemoryType> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALHQueryResponse {
    pub query_id: B256,
    pub result: ALHQueryResult,
    pub execution_time_ms: u64,
    pub state_hash: B256,
    pub block_number: u64,
    pub gas_used: Option<U256>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ALHQueryResult {
    MemoryState {
        agent_state: AgentMemoryState,
        memory_hashes: Vec<(MemoryType, B256)>,
        proofs: Option<Vec<Vec<u8>>>,
    },
    HashVerification {
        is_valid: bool,
        current_hash: B256,
        divergence_point: Option<u64>,
    },
    ContextData {
        contexts: Vec<B256>,
        relevance_scores: Vec<f64>,
        total_matches: usize,
    },
    TransitionProof {
        is_valid: bool,
        transition_path: Vec<B256>,
        intermediate_states: Vec<StateSnapshot>,
    },
    SyncResult {
        synced_agents: Vec<Address>,
        sync_hashes: Vec<B256>,
        conflicts: Vec<SyncConflict>,
    },
    CheckpointData {
        checkpoint_id: B256,
        state_root: B256,
        memory_snapshot: MemorySnapshot,
    },
    Error {
        code: u32,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub state_hash: B256,
    pub block_number: u64,
    pub timestamp: u64,
    pub agent_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConflict {
    pub agent_address: Address,
    pub conflict_type: String,
    pub local_hash: B256,
    pub remote_hash: B256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub total_memories: u64,
    pub memory_distribution: Vec<(MemoryType, u64)>,
    pub merkle_root: B256,
    pub compression_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct ALHStateUpdate {
    pub update_type: UpdateType,
    pub agent_address: Address,
    pub new_state_hash: B256,
    pub block_number: u64,
    pub proof: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum UpdateType {
    MemoryAdded { memory_type: MemoryType, hash: B256 },
    MemoryRemoved { memory_type: MemoryType, hash: B256 },
    StateCheckpointed { checkpoint_id: B256 },
    StateRestored { checkpoint_id: B256 },
    AgentMigrated { from: Address, to: Address },
}

pub struct ALHCoordinator {
    memory_store: Arc<MemoryStore>,
    context_retriever: Arc<ContextRetriever>,
    memory_hash: Arc<RwLock<MemoryLatticeHash>>,
    
    state_cache: Arc<DashMap<Address, CachedAgentState>>,
    query_history: Arc<RwLock<Vec<QueryRecord>>>,
    update_channel: mpsc::Sender<ALHStateUpdate>,
    
    checkpoint_store: Arc<DashMap<B256, CheckpointData>>,
    transition_log: Arc<RwLock<Vec<StateTransition>>>,
    
    config: ALHConfig,
}

#[derive(Debug, Clone)]
struct CachedAgentState {
    state_hash: B256,
    last_update: u64,
    memory_count: u64,
    checkpoint_ids: Vec<B256>,
}

#[derive(Debug, Clone)]
struct QueryRecord {
    query_id: B256,
    query_type: String,
    timestamp: u64,
    execution_time_ms: u64,
    success: bool,
}

#[derive(Debug, Clone)]
struct CheckpointData {
    checkpoint_id: B256,
    agent_address: Address,
    state_hash: B256,
    memory_snapshot: MemorySnapshot,
    created_at: u64,
    metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
struct StateTransition {
    from_hash: B256,
    to_hash: B256,
    block_number: u64,
    transition_type: String,
    gas_cost: U256,
}

#[derive(Debug, Clone)]
pub struct ALHConfig {
    pub max_cache_size: usize,
    pub query_timeout_ms: u64,
    pub checkpoint_retention_blocks: u64,
    pub enable_compression: bool,
    pub proof_generation: bool,
    pub max_query_history: usize,
}

impl Default for ALHConfig {
    fn default() -> Self {
        Self {
            max_cache_size: 10000,
            query_timeout_ms: 5000,
            checkpoint_retention_blocks: 100000,
            enable_compression: true,
            proof_generation: true,
            max_query_history: 1000,
        }
    }
}

impl ALHCoordinator {
    pub fn new(
        memory_store: Arc<MemoryStore>,
        context_retriever: Arc<ContextRetriever>,
        config: ALHConfig,
    ) -> (Self, mpsc::Receiver<ALHStateUpdate>) {
        let (update_tx, update_rx) = mpsc::channel(1000);
        
        let coordinator = Self {
            memory_store,
            context_retriever,
            memory_hash: Arc::new(RwLock::new(MemoryLatticeHash::new())),
            state_cache: Arc::new(DashMap::new()),
            query_history: Arc::new(RwLock::new(Vec::new())),
            update_channel: update_tx,
            checkpoint_store: Arc::new(DashMap::new()),
            transition_log: Arc::new(RwLock::new(Vec::new())),
            config,
        };
        
        (coordinator, update_rx)
    }
    
    pub async fn process_query(&self, request: ALHQueryRequest) -> Result<ALHQueryResponse> {
        let start = std::time::Instant::now();
        let block_number = self.get_current_block().await?;
        
        info!("Processing ALH query: {:?} from {:?}", request.query_type, request.requester);
        
        let result = match &request.query_type {
            ALHQueryType::AgentMemoryState { agent_address, memory_types, include_proofs } => {
                self.query_agent_memory_state(*agent_address, memory_types, *include_proofs).await?
            }
            
            ALHQueryType::MemoryHashVerification { agent_address, expected_hash, block_number } => {
                self.verify_memory_hash(*agent_address, *expected_hash, *block_number).await?
            }
            
            ALHQueryType::ContextRetrieval { agent_address, context_window, semantic_filter } => {
                self.retrieve_context(*agent_address, *context_window, semantic_filter.as_deref()).await?
            }
            
            ALHQueryType::StateTransition { from_state, to_state, verify_path } => {
                self.verify_state_transition(*from_state, *to_state, *verify_path).await?
            }
            
            ALHQueryType::CrossAgentSync { agents, sync_type } => {
                self.sync_cross_agent_state(agents, sync_type).await?
            }
            
            ALHQueryType::CheckpointRequest { agent_address, checkpoint_id } => {
                self.handle_checkpoint_request(*agent_address, *checkpoint_id).await?
            }
        };
        
        let execution_time_ms = start.elapsed().as_millis() as u64;
        let state_hash = self.compute_current_state_hash().await?;
        
        self.record_query(&request, execution_time_ms, true).await;
        
        Ok(ALHQueryResponse {
            query_id: request.query_id,
            result,
            execution_time_ms,
            state_hash,
            block_number,
            gas_used: Some(U256::from(21000 + execution_time_ms * 100)),
        })
    }
    
    async fn query_agent_memory_state(
        &self,
        agent_address: Address,
        memory_types: &[MemoryType],
        include_proofs: bool,
    ) -> Result<ALHQueryResult> {
        let memories = self.memory_store.get_agent_memories(&agent_address).await?;
        
        let mut memory_hashes = Vec::new();
        let mut proofs = if include_proofs { Some(Vec::new()) } else { None };
        
        for memory_type in memory_types {
            let type_memories: Vec<_> = memories.iter()
                .filter(|m| &m.memory_type == memory_type)
                .collect();
            
            if !type_memories.is_empty() {
                let hash = self.memory_hash.read().await
                    .compute_agent_hash(&agent_address, &type_memories);
                memory_hashes.push((*memory_type, hash));
                
                if include_proofs {
                    let proof = self.memory_hash.read().await
                        .generate_proof(&agent_address, &type_memories)?;
                    proofs.as_mut().unwrap().push(proof);
                }
            }
        }
        
        let agent_state = AgentMemoryState {
            agent_address,
            working_memory: memories.iter()
                .filter(|m| m.memory_type == MemoryType::Working)
                .map(|m| m.content_hash)
                .collect(),
            episodic_buffer: memories.iter()
                .filter(|m| m.memory_type == MemoryType::Episodic)
                .map(|m| m.content_hash)
                .collect(),
            last_access_time: chrono::Utc::now().timestamp() as u64,
            context_switches: memories.len() as u64,
        };
        
        self.update_cache(agent_address, &agent_state).await;
        
        Ok(ALHQueryResult::MemoryState {
            agent_state,
            memory_hashes,
            proofs,
        })
    }
    
    async fn verify_memory_hash(
        &self,
        agent_address: Address,
        expected_hash: B256,
        block_number: Option<u64>,
    ) -> Result<ALHQueryResult> {
        let memories = self.memory_store.get_agent_memories(&agent_address).await?;
        let current_hash = self.memory_hash.read().await
            .compute_agent_hash(&agent_address, &memories);
        
        let is_valid = current_hash == expected_hash;
        
        let divergence_point = if !is_valid && block_number.is_some() {
            self.find_divergence_point(agent_address, expected_hash, block_number.unwrap()).await?
        } else {
            None
        };
        
        Ok(ALHQueryResult::HashVerification {
            is_valid,
            current_hash,
            divergence_point,
        })
    }
    
    async fn retrieve_context(
        &self,
        agent_address: Address,
        context_window: u64,
        semantic_filter: Option<&str>,
    ) -> Result<ALHQueryResult> {
        let contexts = self.context_retriever
            .retrieve_context(&agent_address, context_window as usize)
            .await?;
        
        let mut filtered_contexts = contexts;
        let mut relevance_scores = vec![1.0; filtered_contexts.len()];
        
        if let Some(filter) = semantic_filter {
            let (filtered, scores) = self.apply_semantic_filter(filtered_contexts, filter).await?;
            filtered_contexts = filtered;
            relevance_scores = scores;
        }
        
        Ok(ALHQueryResult::ContextData {
            total_matches: filtered_contexts.len(),
            contexts: filtered_contexts,
            relevance_scores,
        })
    }
    
    async fn verify_state_transition(
        &self,
        from_state: B256,
        to_state: B256,
        verify_path: bool,
    ) -> Result<ALHQueryResult> {
        let transitions = self.transition_log.read().await;
        
        let mut transition_path = Vec::new();
        let mut current = from_state;
        let mut found = false;
        
        if verify_path {
            for transition in transitions.iter() {
                if transition.from_hash == current {
                    transition_path.push(transition.to_hash);
                    current = transition.to_hash;
                    
                    if current == to_state {
                        found = true;
                        break;
                    }
                }
            }
        } else {
            found = transitions.iter().any(|t| 
                t.from_hash == from_state && t.to_hash == to_state
            );
        }
        
        let intermediate_states = if verify_path {
            self.get_intermediate_states(&transition_path).await?
        } else {
            Vec::new()
        };
        
        Ok(ALHQueryResult::TransitionProof {
            is_valid: found,
            transition_path,
            intermediate_states,
        })
    }
    
    async fn sync_cross_agent_state(
        &self,
        agents: &[Address],
        sync_type: &SyncType,
    ) -> Result<ALHQueryResult> {
        let mut synced_agents = Vec::new();
        let mut sync_hashes = Vec::new();
        let mut conflicts = Vec::new();
        
        for agent in agents {
            match sync_type {
                SyncType::Full => {
                    let memories = self.memory_store.get_agent_memories(agent).await?;
                    let hash = self.memory_hash.read().await
                        .compute_agent_hash(agent, &memories);
                    
                    synced_agents.push(*agent);
                    sync_hashes.push(hash);
                }
                
                SyncType::Incremental { since_block } => {
                    if let Some(cached) = self.state_cache.get(agent) {
                        if cached.last_update >= *since_block {
                            synced_agents.push(*agent);
                            sync_hashes.push(cached.state_hash);
                        }
                    }
                }
                
                SyncType::Selective { memory_types } => {
                    let memories = self.memory_store.get_agent_memories(agent).await?;
                    let filtered: Vec<_> = memories.into_iter()
                        .filter(|m| memory_types.contains(&m.memory_type))
                        .collect();
                    
                    let hash = self.memory_hash.read().await
                        .compute_agent_hash(agent, &filtered);
                    
                    synced_agents.push(*agent);
                    sync_hashes.push(hash);
                }
            }
        }
        
        Ok(ALHQueryResult::SyncResult {
            synced_agents,
            sync_hashes,
            conflicts,
        })
    }
    
    async fn handle_checkpoint_request(
        &self,
        agent_address: Address,
        checkpoint_id: Option<B256>,
    ) -> Result<ALHQueryResult> {
        if let Some(id) = checkpoint_id {
            if let Some(checkpoint) = self.checkpoint_store.get(&id) {
                return Ok(ALHQueryResult::CheckpointData {
                    checkpoint_id: id,
                    state_root: checkpoint.state_hash,
                    memory_snapshot: checkpoint.memory_snapshot.clone(),
                });
            }
        }
        
        let checkpoint = self.create_checkpoint(agent_address).await?;
        
        Ok(ALHQueryResult::CheckpointData {
            checkpoint_id: checkpoint.checkpoint_id,
            state_root: checkpoint.state_hash,
            memory_snapshot: checkpoint.memory_snapshot,
        })
    }
    
    async fn create_checkpoint(&self, agent_address: Address) -> Result<CheckpointData> {
        let memories = self.memory_store.get_agent_memories(&agent_address).await?;
        let state_hash = self.memory_hash.read().await
            .compute_agent_hash(&agent_address, &memories);
        
        let mut memory_distribution = Vec::new();
        for memory_type in &[MemoryType::ShortTerm, MemoryType::LongTerm, 
                            MemoryType::Working, MemoryType::Episodic, MemoryType::Semantic] {
            let count = memories.iter()
                .filter(|m| &m.memory_type == memory_type)
                .count() as u64;
            memory_distribution.push((*memory_type, count));
        }
        
        let merkle_root = self.memory_hash.read().await
            .compute_merkle_root(&memories);
        
        let checkpoint = CheckpointData {
            checkpoint_id: B256::random(),
            agent_address,
            state_hash,
            memory_snapshot: MemorySnapshot {
                total_memories: memories.len() as u64,
                memory_distribution,
                merkle_root,
                compression_ratio: 0.75,
            },
            created_at: chrono::Utc::now().timestamp() as u64,
            metadata: serde_json::json!({
                "version": "1.0",
                "type": "manual"
            }),
        };
        
        self.checkpoint_store.insert(checkpoint.checkpoint_id, checkpoint.clone());
        
        self.update_channel.send(ALHStateUpdate {
            update_type: UpdateType::StateCheckpointed { 
                checkpoint_id: checkpoint.checkpoint_id 
            },
            agent_address,
            new_state_hash: state_hash,
            block_number: self.get_current_block().await?,
            proof: None,
        }).await?;
        
        Ok(checkpoint)
    }
    
    pub async fn notify_state_update(&self, update: ALHStateUpdate) -> Result<()> {
        info!("ALH state update: {:?} for agent {:?}", update.update_type, update.agent_address);
        
        if let Some(mut cached) = self.state_cache.get_mut(&update.agent_address) {
            cached.state_hash = update.new_state_hash;
            cached.last_update = update.block_number;
        }
        
        let transition = StateTransition {
            from_hash: self.get_previous_state_hash(update.agent_address).await?,
            to_hash: update.new_state_hash,
            block_number: update.block_number,
            transition_type: format!("{:?}", update.update_type),
            gas_cost: U256::from(50000),
        };
        
        self.transition_log.write().await.push(transition);
        
        if self.transition_log.read().await.len() > 10000 {
            self.prune_old_transitions().await?;
        }
        
        Ok(())
    }
    
    async fn update_cache(&self, agent_address: Address, state: &AgentMemoryState) {
        let state_hash = B256::from_slice(
            &sha3::Keccak256::digest(&bincode::serialize(state).unwrap())
        );
        
        self.state_cache.insert(agent_address, CachedAgentState {
            state_hash,
            last_update: chrono::Utc::now().timestamp() as u64,
            memory_count: state.working_memory.len() as u64 + state.episodic_buffer.len() as u64,
            checkpoint_ids: Vec::new(),
        });
        
        if self.state_cache.len() > self.config.max_cache_size {
            self.evict_old_cache_entries();
        }
    }
    
    async fn record_query(&self, request: &ALHQueryRequest, execution_time_ms: u64, success: bool) {
        let record = QueryRecord {
            query_id: request.query_id,
            query_type: format!("{:?}", request.query_type),
            timestamp: request.timestamp,
            execution_time_ms,
            success,
        };
        
        let mut history = self.query_history.write().await;
        history.push(record);
        
        if history.len() > self.config.max_query_history {
            history.drain(0..100);
        }
    }
    
    async fn get_current_block(&self) -> Result<u64> {
        Ok(chrono::Utc::now().timestamp() as u64 / 12)
    }
    
    async fn find_divergence_point(
        &self,
        _agent_address: Address,
        _expected_hash: B256,
        _block_number: u64,
    ) -> Result<Option<u64>> {
        Ok(Some(0))
    }
    
    async fn apply_semantic_filter(
        &self,
        contexts: Vec<B256>,
        _filter: &str,
    ) -> Result<(Vec<B256>, Vec<f64>)> {
        let scores = vec![0.85; contexts.len()];
        Ok((contexts, scores))
    }
    
    async fn get_intermediate_states(&self, path: &[B256]) -> Result<Vec<StateSnapshot>> {
        let mut states = Vec::new();
        
        for (i, hash) in path.iter().enumerate() {
            states.push(StateSnapshot {
                state_hash: *hash,
                block_number: self.get_current_block().await? - (path.len() - i) as u64,
                timestamp: chrono::Utc::now().timestamp() as u64 - ((path.len() - i) * 12) as u64,
                agent_count: self.state_cache.len(),
            });
        }
        
        Ok(states)
    }
    
    async fn get_previous_state_hash(&self, agent_address: Address) -> Result<B256> {
        if let Some(cached) = self.state_cache.get(&agent_address) {
            Ok(cached.state_hash)
        } else {
            Ok(B256::ZERO)
        }
    }
    
    async fn prune_old_transitions(&self) -> Result<()> {
        let current_block = self.get_current_block().await?;
        let retention_threshold = current_block.saturating_sub(self.config.checkpoint_retention_blocks);
        
        self.transition_log.write().await
            .retain(|t| t.block_number > retention_threshold);
        
        Ok(())
    }
    
    fn evict_old_cache_entries(&self) {
        let mut entries: Vec<_> = self.state_cache.iter()
            .map(|e| (e.key().clone(), e.last_update))
            .collect();
        
        entries.sort_by_key(|e| e.1);
        
        let to_remove = entries.len() / 10;
        for (addr, _) in entries.into_iter().take(to_remove) {
            self.state_cache.remove(&addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_alh_coordinator_creation() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (coordinator, _rx) = ALHCoordinator::new(
            memory_store,
            context_retriever,
            ALHConfig::default(),
        );
        
        assert_eq!(coordinator.state_cache.len(), 0);
    }
    
    #[tokio::test]
    async fn test_memory_state_query() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (coordinator, _rx) = ALHCoordinator::new(
            memory_store.clone(),
            context_retriever,
            ALHConfig::default(),
        );
        
        let agent = Address::random();
        let request = ALHQueryRequest {
            query_id: B256::random(),
            requester: Address::random(),
            query_type: ALHQueryType::AgentMemoryState {
                agent_address: agent,
                memory_types: vec![MemoryType::Working, MemoryType::Episodic],
                include_proofs: false,
            },
            timestamp: chrono::Utc::now().timestamp() as u64,
            priority: 1,
            metadata: None,
        };
        
        let response = coordinator.process_query(request).await.unwrap();
        match response.result {
            ALHQueryResult::MemoryState { agent_state, .. } => {
                assert_eq!(agent_state.agent_address, agent);
            }
            _ => panic!("Unexpected result type"),
        }
    }
    
    #[tokio::test]
    async fn test_checkpoint_creation() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (coordinator, _rx) = ALHCoordinator::new(
            memory_store,
            context_retriever,
            ALHConfig::default(),
        );
        
        let agent = Address::random();
        let checkpoint = coordinator.create_checkpoint(agent).await.unwrap();
        
        assert_eq!(checkpoint.agent_address, agent);
        assert!(coordinator.checkpoint_store.contains_key(&checkpoint.checkpoint_id));
    }
}