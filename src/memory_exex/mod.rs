pub mod memory_store;
pub mod ephemeral_zones;
pub mod memory_hash;
pub mod checkpointing;
pub mod portability;

use crate::{MemoryEvent, shared::Metrics};
use eyre::Result;
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_node_api::FullNodeComponents;
use reth_primitives::{SealedBlockWithSenders, TransactionSigned};
use reth_tracing::tracing::{info, debug, error};
use tokio::sync::mpsc;
use std::sync::Arc;
use dashmap::DashMap;

use self::{
    memory_store::MemoryStore,
    ephemeral_zones::EphemeralZoneManager,
    memory_hash::MemoryLatticeHash,
    checkpointing::CheckpointManager,
    portability::MemoryPortability,
};

#[derive(Debug)]
pub struct MemoryExEx<Node: FullNodeComponents> {
    ctx: ExExContext<Node>,
    metrics: Metrics,
    memory_store: Arc<MemoryStore>,
    ephemeral_zones: Arc<EphemeralZoneManager>,
    memory_hash: Arc<MemoryLatticeHash>,
    checkpoint_manager: Arc<CheckpointManager>,
    portability: Arc<MemoryPortability>,
    active_agents: Arc<DashMap<String, AgentMemoryState>>,
}

#[derive(Debug, Clone)]
pub struct AgentMemoryState {
    pub agent_id: String,
    pub memory_hash: [u8; 32],
    pub total_memories: u64,
    pub last_checkpoint: Option<std::time::Instant>,
    pub active_session: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub id: String,
    pub agent_id: String,
    pub memory_type: MemoryType,
    pub content: Vec<u8>,
    pub embedding: Option<Vec<f32>>,
    pub timestamp: std::time::Instant,
    pub importance: f64,
    pub access_count: u64,
}

#[derive(Debug, Clone)]
pub enum MemoryType {
    ShortTerm,
    LongTerm,
    Working,
    Episodic,
    Semantic,
}

impl<Node: FullNodeComponents> MemoryExEx<Node> {
    pub async fn new(ctx: ExExContext<Node>, metrics: Metrics) -> Result<Self> {
        info!("Initializing Memory ExEx");
        
        let memory_store = Arc::new(MemoryStore::new().await?);
        let ephemeral_zones = Arc::new(EphemeralZoneManager::new());
        let memory_hash = Arc::new(MemoryLatticeHash::new());
        let checkpoint_manager = Arc::new(CheckpointManager::new(memory_store.clone()));
        let portability = Arc::new(MemoryPortability::new());
        let active_agents = Arc::new(DashMap::new());
        
        Ok(Self {
            ctx,
            metrics,
            memory_store,
            ephemeral_zones,
            memory_hash,
            checkpoint_manager,
            portability,
            active_agents,
        })
    }
    
    pub async fn run(&mut self, event_tx: mpsc::Sender<MemoryEvent>) -> Result<()> {
        info!("Starting Memory ExEx");
        
        let checkpoint_handle = self.start_checkpoint_service();
        let cleanup_handle = self.start_cleanup_service();
        
        while let Some(notification) = self.ctx.notifications.recv().await {
            match notification {
                ExExNotification::ChainCommitted { new } => {
                    for block in new.blocks() {
                        self.process_block(&block, &event_tx).await?;
                    }
                }
                ExExNotification::ChainReorged { old, new } => {
                    info!("Chain reorg detected, adjusting memory states");
                    for block in old.blocks() {
                        self.revert_block_memories(&block).await?;
                    }
                    for block in new.blocks() {
                        self.process_block(&block, &event_tx).await?;
                    }
                }
                ExExNotification::ChainReverted { old } => {
                    info!("Chain reverted, removing memories");
                    for block in old.blocks() {
                        self.revert_block_memories(&block).await?;
                    }
                }
            }
            
            self.ctx.events.send(ExExEvent::FinishedHeight(
                self.ctx.notifications.tip().number
            ))?;
        }
        
        checkpoint_handle.abort();
        cleanup_handle.abort();
        
        Ok(())
    }
    
    async fn process_block(
        &self,
        block: &SealedBlockWithSenders,
        event_tx: &mpsc::Sender<MemoryEvent>,
    ) -> Result<()> {
        debug!("Processing block {} for memory operations", block.number);
        
        for (tx, sender) in block.transactions_with_sender() {
            self.process_transaction(tx, sender, event_tx).await?;
        }
        
        self.metrics.record_block_processed();
        Ok(())
    }
    
    async fn process_transaction(
        &self,
        tx: &TransactionSigned,
        sender: &reth_primitives::Address,
        event_tx: &mpsc::Sender<MemoryEvent>,
    ) -> Result<()> {
        if let Some((agent_id, operation)) = self.parse_memory_operation(tx) {
            match operation {
                MemoryOperation::Store { memory } => {
                    self.store_memory(&agent_id, memory, event_tx).await?;
                }
                MemoryOperation::Retrieve { memory_id } => {
                    self.retrieve_memory(&agent_id, &memory_id, event_tx).await?;
                }
                MemoryOperation::Forget { memory_id } => {
                    self.forget_memory(&agent_id, &memory_id).await?;
                }
                MemoryOperation::Export { format } => {
                    self.export_memories(&agent_id, format).await?;
                }
                MemoryOperation::StartSession => {
                    self.start_ephemeral_session(&agent_id).await?;
                }
                MemoryOperation::EndSession { session_id } => {
                    self.end_ephemeral_session(&agent_id, &session_id).await?;
                }
            }
        }
        
        Ok(())
    }
    
    async fn store_memory(
        &self,
        agent_id: &str,
        memory: Memory,
        event_tx: &mpsc::Sender<MemoryEvent>,
    ) -> Result<()> {
        let memory_id = memory.id.clone();
        
        self.memory_store.store(memory.clone()).await?;
        
        let mut agent_state = self.active_agents.entry(agent_id.to_string())
            .or_insert_with(|| AgentMemoryState {
                agent_id: agent_id.to_string(),
                memory_hash: [0u8; 32],
                total_memories: 0,
                last_checkpoint: None,
                active_session: None,
            });
        
        agent_state.total_memories += 1;
        agent_state.memory_hash = self.memory_hash.update_hash(
            &agent_state.memory_hash,
            &memory,
        ).await?;
        
        event_tx.send(MemoryEvent::MemoryStored {
            agent_id: agent_id.to_string(),
            memory_hash: hex::encode(&agent_state.memory_hash),
        }).await?;
        
        self.metrics.record_memory_stored();
        
        Ok(())
    }
    
    async fn retrieve_memory(
        &self,
        agent_id: &str,
        memory_id: &str,
        event_tx: &mpsc::Sender<MemoryEvent>,
    ) -> Result<()> {
        if let Some(memory) = self.memory_store.retrieve(agent_id, memory_id).await? {
            let memory_size = memory.content.len();
            
            event_tx.send(MemoryEvent::MemoryRetrieved {
                agent_id: agent_id.to_string(),
                memory_size,
            }).await?;
            
            self.metrics.record_memory_retrieved();
        }
        
        Ok(())
    }
    
    async fn forget_memory(&self, agent_id: &str, memory_id: &str) -> Result<()> {
        self.memory_store.forget(agent_id, memory_id).await?;
        
        if let Some(mut agent_state) = self.active_agents.get_mut(agent_id) {
            agent_state.total_memories = agent_state.total_memories.saturating_sub(1);
        }
        
        Ok(())
    }
    
    async fn export_memories(&self, agent_id: &str, format: ExportFormat) -> Result<()> {
        let memories = self.memory_store.get_all_memories(agent_id).await?;
        let package = self.portability.export_memories(agent_id, memories, format).await?;
        
        info!("Exported {} memories for agent {}", package.memory_count, agent_id);
        
        Ok(())
    }
    
    async fn start_ephemeral_session(&self, agent_id: &str) -> Result<()> {
        let session_id = self.ephemeral_zones.create_session(agent_id).await?;
        
        if let Some(mut agent_state) = self.active_agents.get_mut(agent_id) {
            agent_state.active_session = Some(session_id);
        }
        
        Ok(())
    }
    
    async fn end_ephemeral_session(&self, agent_id: &str, session_id: &str) -> Result<()> {
        let session_memories = self.ephemeral_zones.end_session(session_id).await?;
        
        for memory in session_memories {
            self.memory_store.store(memory).await?;
        }
        
        if let Some(mut agent_state) = self.active_agents.get_mut(agent_id) {
            agent_state.active_session = None;
        }
        
        Ok(())
    }
    
    async fn revert_block_memories(&self, block: &SealedBlockWithSenders) -> Result<()> {
        Ok(())
    }
    
    fn parse_memory_operation(&self, tx: &TransactionSigned) -> Option<(String, MemoryOperation)> {
        if tx.input().len() < 4 {
            return None;
        }
        
        let selector = &tx.input()[0..4];
        match selector {
            [0x01, 0x02, 0x03, 0x04] => self.parse_store_operation(tx),
            [0x05, 0x06, 0x07, 0x08] => self.parse_retrieve_operation(tx),
            [0x09, 0x0a, 0x0b, 0x0c] => self.parse_forget_operation(tx),
            [0x0d, 0x0e, 0x0f, 0x10] => self.parse_export_operation(tx),
            _ => None,
        }
    }
    
    fn parse_store_operation(&self, tx: &TransactionSigned) -> Option<(String, MemoryOperation)> {
        Some(("agent123".to_string(), MemoryOperation::Store {
            memory: Memory {
                id: "mem123".to_string(),
                agent_id: "agent123".to_string(),
                memory_type: MemoryType::LongTerm,
                content: vec![1, 2, 3],
                embedding: None,
                timestamp: std::time::Instant::now(),
                importance: 0.8,
                access_count: 0,
            }
        }))
    }
    
    fn parse_retrieve_operation(&self, tx: &TransactionSigned) -> Option<(String, MemoryOperation)> {
        Some(("agent123".to_string(), MemoryOperation::Retrieve {
            memory_id: "mem123".to_string(),
        }))
    }
    
    fn parse_forget_operation(&self, tx: &TransactionSigned) -> Option<(String, MemoryOperation)> {
        Some(("agent123".to_string(), MemoryOperation::Forget {
            memory_id: "mem123".to_string(),
        }))
    }
    
    fn parse_export_operation(&self, tx: &TransactionSigned) -> Option<(String, MemoryOperation)> {
        Some(("agent123".to_string(), MemoryOperation::Export {
            format: ExportFormat::Json,
        }))
    }
    
    fn start_checkpoint_service(&self) -> tokio::task::JoinHandle<()> {
        let checkpoint_manager = self.checkpoint_manager.clone();
        let active_agents = self.active_agents.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                
                for agent_state in active_agents.iter() {
                    if let Err(e) = checkpoint_manager.create_checkpoint(&agent_state.agent_id).await {
                        error!("Failed to create checkpoint for agent {}: {}", agent_state.agent_id, e);
                    }
                }
            }
        })
    }
    
    fn start_cleanup_service(&self) -> tokio::task::JoinHandle<()> {
        let ephemeral_zones = self.ephemeral_zones.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                interval.tick().await;
                
                if let Err(e) = ephemeral_zones.cleanup_expired_sessions().await {
                    error!("Failed to cleanup expired sessions: {}", e);
                }
            }
        })
    }
}

#[derive(Debug, Clone)]
enum MemoryOperation {
    Store { memory: Memory },
    Retrieve { memory_id: String },
    Forget { memory_id: String },
    Export { format: ExportFormat },
    StartSession,
    EndSession { session_id: String },
}

#[derive(Debug, Clone)]
pub enum ExportFormat {
    Json,
    Binary,
    Compressed,
}