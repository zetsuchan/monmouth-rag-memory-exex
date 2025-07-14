//! Enhanced main ExEx implementation with Future trait and proper state management
//! 
//! This module provides the main entry point for the RAG-Memory ExEx with:
//! - Proper Future trait implementation for Reth integration
//! - FinishedHeight event handling for safe pruning
//! - State checkpointing for reorg handling
//! - Inter-ExEx communication with SVM ExEx

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use alloy::primitives::{BlockNumber, B256};
use dashmap::DashMap;
use eyre::Result;
use futures_util::{FutureExt, StreamExt};
use reth_exex::{ExExContext, ExExEvent, ExExNotification};
use reth_execution_types::Chain;
use reth_node_api::FullNodeComponents;
use reth_primitives::SealedBlockWithSenders;
use reth_tracing::tracing::{debug, error, info, warn};
use tokio::sync::{mpsc, RwLock};

use crate::{
    config::{ExExConfiguration, ExExType},
    inter_exex::{InterExExCoordinator, MessageBusConfig, ExExMessage, MessageType, MessagePayload},
    memory_exex::MemoryExEx,
    rag_exex::RagExEx,
    shared::Metrics,
    sync::{ExExSyncCoordinator, MemorySyncHandle, RagSyncHandle},
    RagEvent, MemoryEvent,
};

/// Maximum number of blocks to process before sending FinishedHeight
const MAX_BLOCKS_BEFORE_COMMIT: u64 = 100;

/// Maximum time to wait before sending FinishedHeight
const MAX_TIME_BEFORE_COMMIT: Duration = Duration::from_secs(30);

/// ExEx processing state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExExState {
    /// Initial state
    Initializing,
    /// Processing blocks normally
    Processing,
    /// Handling a reorg
    Reorging,
    /// Recovering from an error
    Recovering,
    /// Shutting down
    ShuttingDown,
}

/// State checkpoint for handling reorgs
#[derive(Debug, Clone)]
struct StateCheckpoint {
    block_number: BlockNumber,
    block_hash: B256,
    rag_snapshot: Vec<u8>,
    memory_snapshot: Vec<u8>,
    timestamp: Instant,
}

/// Enhanced RAG-Memory ExEx with Future implementation
pub struct EnhancedRagMemoryExEx<Node: FullNodeComponents> {
    /// ExEx context for notifications
    ctx: ExExContext<Node>,
    
    /// Configuration
    config: ExExConfiguration,
    
    /// Current processing state
    state: ExExState,
    
    /// RAG ExEx instance
    rag_exex: Arc<RwLock<RagExEx<Node>>>,
    
    /// Memory ExEx instance
    memory_exex: Arc<RwLock<MemoryExEx<Node>>>,
    
    /// Inter-ExEx coordinator
    coordinator: Arc<InterExExCoordinator>,
    
    /// Synchronization coordinator
    sync_coordinator: Arc<ExExSyncCoordinator>,
    
    /// Memory sync handle
    memory_sync: MemorySyncHandle,
    
    /// RAG sync handle
    rag_sync: RagSyncHandle,
    
    /// Metrics collector
    metrics: Arc<Metrics>,
    
    /// State checkpoints for reorg handling
    checkpoints: Arc<RwLock<Vec<StateCheckpoint>>>,
    
    /// Last committed block height
    last_committed_height: BlockNumber,
    
    /// Time of last commit
    last_commit_time: Instant,
    
    /// Pending blocks to be committed
    pending_blocks: Vec<BlockNumber>,
    
    /// Channel for sending FinishedHeight events
    event_sender: mpsc::UnboundedSender<ExExEvent>,
    
    /// Channels for internal communication
    rag_channel: (mpsc::Sender<RagEvent>, mpsc::Receiver<RagEvent>),
    memory_channel: (mpsc::Sender<MemoryEvent>, mpsc::Receiver<MemoryEvent>),
}

impl<Node: FullNodeComponents> EnhancedRagMemoryExEx<Node> {
    /// Create a new enhanced ExEx instance
    pub async fn new(mut ctx: ExExContext<Node>, config: ExExConfiguration) -> Result<Self> {
        info!("Initializing Enhanced RAG-Memory ExEx");
        
        // Initialize metrics
        let metrics = Arc::new(Metrics::new()?);
        
        // Initialize synchronization coordinator
        let sync_coordinator = Arc::new(ExExSyncCoordinator::new(
            config.performance.max_concurrent_processing
        ));
        let memory_sync = MemorySyncHandle::new(sync_coordinator.clone());
        let rag_sync = RagSyncHandle::new(sync_coordinator.clone());
        
        // Initialize sub-ExEx instances
        let rag_exex = Arc::new(RwLock::new(
            RagExEx::new(ctx.clone(), metrics.clone()).await?
        ));
        let memory_exex = Arc::new(RwLock::new(
            MemoryExEx::new(ctx.clone(), metrics.clone()).await?
        ));
        
        // Initialize inter-ExEx coordinator
        let bus_config = MessageBusConfig {
            channel_buffer_size: config.shared.communication.message_bus.channel_buffer_size,
            max_message_size: config.shared.communication.message_bus.max_message_size,
            bind_address: config.individual.network.bind_address.clone(),
            discovery_method: crate::inter_exex::bus::DiscoveryMethod::Multicast,
            heartbeat_interval_secs: config.shared.communication.heartbeat_interval_secs,
        };
        
        let coordinator = Arc::new(InterExExCoordinator::new(
            bus_config,
            config.individual.instance_id.clone(),
        )?);
        
        // Start the coordinator
        coordinator.start().await?;
        
        // Update node info
        coordinator.update_node_info(|info| {
            info.node_type = crate::inter_exex::messages::NodeType::RAG;
            info.status = crate::inter_exex::messages::NodeStatus::Ready;
            info.capabilities = vec![
                "rag_context".to_string(),
                "memory_management".to_string(),
                "agent_coordination".to_string(),
            ];
        }).await?;
        
        // Create event channel
        let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
        
        // Forward events to ExEx context
        let ctx_events = ctx.events.clone();
        tokio::spawn(async move {
            while let Some(event) = event_receiver.recv().await {
                let _ = ctx_events.send(event).await;
            }
        });
        
        // Create internal channels
        let rag_channel = mpsc::channel(1000);
        let memory_channel = mpsc::channel(1000);
        
        Ok(Self {
            ctx,
            config,
            state: ExExState::Initializing,
            rag_exex,
            memory_exex,
            coordinator,
            sync_coordinator,
            memory_sync,
            rag_sync,
            metrics,
            checkpoints: Arc::new(RwLock::new(Vec::new())),
            last_committed_height: 0,
            last_commit_time: Instant::now(),
            pending_blocks: Vec::new(),
            event_sender,
            rag_channel,
            memory_channel,
        })
    }
    
    /// Handle a new chain notification
    async fn handle_notification(&mut self, notification: ExExNotification) -> Result<()> {
        match &notification {
            ExExNotification::ChainCommitted { new } => {
                info!("Processing committed chain");
                self.handle_chain_committed(new).await?;
            }
            ExExNotification::ChainReorged { old, new } => {
                warn!("Handling reorg");
                self.handle_chain_reorged(old, new).await?;
            }
            ExExNotification::ChainReverted { old } => {
                warn!("Handling chain reversion");
                self.handle_chain_reverted(old).await?;
            }
        }
        
        // Check if we should send FinishedHeight
        self.maybe_send_finished_height().await?;
        
        Ok(())
    }
    
    /// Handle committed chain
    async fn handle_chain_committed(&mut self, chain: &Chain) -> Result<()> {
        self.state = ExExState::Processing;
        
        for block in chain.blocks() {
            let block_number = block.number();
            debug!("Processing block {}", block_number);
            
            // Acquire processing permit for rate limiting
            let _permit = self.sync_coordinator.acquire_processing_permit().await?;
            
            // Process through Memory ExEx first
            {
                let mut memory = self.memory_exex.write().await;
                memory.process_block(block).await?;
            }
            
            // Signal that memory has committed this block
            self.memory_sync.commit_block(block_number).await?;
            
            // Process through RAG ExEx after memory commit
            {
                // Wait for memory to be committed before RAG processing
                self.rag_sync.wait_for_block(block_number).await?;
                
                let mut rag = self.rag_exex.write().await;
                rag.process_block(block).await?;
            }
            
            // Send inter-ExEx update
            self.send_state_update(block_number).await?;
            
            // Track pending block
            self.pending_blocks.push(block_number);
            
            // Update metrics
            self.metrics.record_block_processed();
            
            // Cleanup old notifications periodically
            if block_number % 1000 == 0 {
                let cleanup_threshold = block_number.saturating_sub(2000);
                self.sync_coordinator.cleanup_old_notifications(cleanup_threshold).await;
            }
        }
        
        Ok(())
    }
    
    /// Handle chain reorg
    async fn handle_chain_reorged(
        &mut self, 
        old: &Chain,
        new: &Chain,
    ) -> Result<()> {
        self.state = ExExState::Reorging;
        
        // Find fork point
        let fork_block = old.fork_block();
        info!("Reorg detected at block {}", fork_block);
        
        // Revert to checkpoint
        self.revert_to_checkpoint(fork_block).await?;
        
        // Process new chain
        self.handle_chain_committed(new).await?;
        
        self.state = ExExState::Processing;
        Ok(())
    }
    
    /// Handle chain reversion
    async fn handle_chain_reverted(&mut self, old: &Chain) -> Result<()> {
        let fork_block = old.fork_block();
        warn!("Chain reverted to block {}", fork_block);
        
        // Revert state
        self.revert_to_checkpoint(fork_block).await?;
        
        // Remove pending blocks
        self.pending_blocks.retain(|&b| b < fork_block);
        
        Ok(())
    }
    
    /// Send state update to other ExEx instances
    async fn send_state_update(&self, block_number: BlockNumber) -> Result<()> {
        let message = ExExMessage::new(
            MessageType::StateSync,
            self.config.individual.instance_id.clone(),
            MessagePayload::StateData(crate::inter_exex::messages::StateData {
                block_number,
                state_root: B256::default(), // TODO: Calculate actual state root
                alh: B256::default(), // TODO: Calculate ALH
                tx_count: 0,
                metrics: crate::inter_exex::messages::ProcessingMetrics {
                    processing_time_ms: 0,
                    successful_txs: 0,
                    failed_txs: 0,
                    gas_used: alloy::primitives::U256::ZERO,
                },
            }),
        );
        
        self.coordinator.broadcast(message).await?;
        Ok(())
    }
    
    /// Create a state checkpoint
    async fn create_checkpoint(&self, block_number: BlockNumber) -> Result<()> {
        let checkpoint = StateCheckpoint {
            block_number,
            block_hash: B256::default(), // TODO: Get actual block hash
            rag_snapshot: vec![], // TODO: Serialize RAG state
            memory_snapshot: vec![], // TODO: Serialize memory state
            timestamp: Instant::now(),
        };
        
        let mut checkpoints = self.checkpoints.write().await;
        checkpoints.push(checkpoint);
        
        // Keep only last 10 checkpoints
        if checkpoints.len() > 10 {
            checkpoints.remove(0);
        }
        
        Ok(())
    }
    
    /// Revert to a checkpoint
    async fn revert_to_checkpoint(&mut self, block_number: BlockNumber) -> Result<()> {
        let mut checkpoints = self.checkpoints.write().await;
        
        // Find and remove checkpoints after the target block
        checkpoints.retain(|c| c.block_number <= block_number);
        
        if let Some(checkpoint) = checkpoints.last() {
            info!("Reverting to checkpoint at block {}", checkpoint.block_number);
            // TODO: Restore RAG and memory state from checkpoint
            self.last_committed_height = checkpoint.block_number;
        }
        
        Ok(())
    }
    
    /// Check if we should send FinishedHeight event
    async fn maybe_send_finished_height(&mut self) -> Result<()> {
        let should_commit = self.pending_blocks.len() >= MAX_BLOCKS_BEFORE_COMMIT as usize ||
            self.last_commit_time.elapsed() >= MAX_TIME_BEFORE_COMMIT;
        
        if should_commit && !self.pending_blocks.is_empty() {
            let highest_block = *self.pending_blocks.iter().max().unwrap();
            
            if highest_block > self.last_committed_height {
                info!("Sending FinishedHeight event for block {}", highest_block);
                
                // Send the event
                self.event_sender.send(ExExEvent::FinishedHeight(highest_block))?;
                
                // Create checkpoint
                self.create_checkpoint(highest_block).await?;
                
                // Update state
                self.last_committed_height = highest_block;
                self.last_commit_time = Instant::now();
                self.pending_blocks.clear();
            }
        }
        
        Ok(())
    }
}

/// Future implementation for the ExEx
impl<Node: FullNodeComponents> Future for EnhancedRagMemoryExEx<Node> {
    type Output = eyre::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Check if we're shutting down
        if self.state == ExExState::ShuttingDown {
            return Poll::Ready(Ok(()));
        }

        // Process notifications
        loop {
            match self.ctx.notifications.poll_next_unpin(cx) {
                Poll::Ready(Some(notification)) => {
                    // Handle notification asynchronously
                    let result = futures::executor::block_on(
                        self.handle_notification(notification)
                    );
                    
                    if let Err(e) = result {
                        error!("Error handling notification: {}", e);
                        self.state = ExExState::Recovering;
                        return Poll::Ready(Err(e));
                    }
                }
                Poll::Ready(None) => {
                    info!("Notification stream ended, shutting down");
                    self.state = ExExState::ShuttingDown;
                    
                    // Shutdown coordinator
                    let _ = futures::executor::block_on(self.coordinator.shutdown());
                    
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => break,
            }
        }
        
        // Process internal events
        match self.rag_channel.1.poll_recv(cx) {
            Poll::Ready(Some(event)) => {
                debug!("Received RAG event: {:?}", event);
                match event {
                    RagEvent::ContextRetrieved { agent_id, context } => {
                        self.metrics.record_rag_query();
                    }
                    RagEvent::EmbeddingGenerated { tx_hash, embedding_size } => {
                        self.metrics.record_embedding_generated();
                    }
                }
            }
            Poll::Ready(None) => {}
            Poll::Pending => {}
        }
        
        match self.memory_channel.1.poll_recv(cx) {
            Poll::Ready(Some(event)) => {
                debug!("Received memory event: {:?}", event);
                match event {
                    MemoryEvent::MemoryStored { agent_id, memory_hash } => {
                        self.metrics.record_memory_stored();
                    }
                    MemoryEvent::MemoryRetrieved { agent_id, memory_size } => {
                        self.metrics.record_memory_retrieved();
                    }
                }
            }
            Poll::Ready(None) => {}
            Poll::Pending => {}
        }
        
        // Continue polling
        Poll::Pending
    }
}

/// Create and run the enhanced ExEx
pub async fn run<Node: FullNodeComponents>(
    ctx: ExExContext<Node>,
    config: ExExConfiguration,
) -> Result<()> {
    let exex = EnhancedRagMemoryExEx::new(ctx, config).await?;
    exex.await
}