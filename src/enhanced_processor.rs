use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use eyre::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use crate::{
    alh::coordination::{ALHCoordinator, ALHQueryRequest, ALHQueryType, ALHStateUpdate, UpdateType},
    memory_exex::MemoryStore,
    rag_exex::context_retrieval::ContextRetriever,
    shared::{
        ai_agent::RoutingDecision,
        unified_coordinator::UnifiedCoordinator,
    },
};

#[async_trait]
pub trait EnhancedProcessor: Send + Sync {
    async fn process_with_alh_hooks(&self, transaction: &reth_primitives::TransactionSigned) -> Result<()>;
    async fn query_alh_state(&self, request: ALHQueryRequest) -> Result<serde_json::Value>;
    async fn checkpoint_agent_state(&self, agent_address: Address) -> Result<B256>;
}

pub struct EnhancedExExProcessor {
    alh_coordinator: Arc<ALHCoordinator>,
    unified_coordinator: Arc<UnifiedCoordinator>,
    memory_store: Arc<MemoryStore>,
    context_retriever: Arc<ContextRetriever>,
    
    hook_registry: Arc<RwLock<Vec<ProcessorHook>>>,
    state_listeners: Arc<RwLock<Vec<Box<dyn StateListener>>>>,
}

#[derive(Clone)]
pub enum ProcessorHook {
    PreProcessing(Arc<dyn Fn(&reth_primitives::TransactionSigned) -> bool + Send + Sync>),
    PostProcessing(Arc<dyn Fn(&reth_primitives::TransactionSigned, &RoutingDecision) + Send + Sync>),
    StateUpdate(Arc<dyn Fn(&ALHStateUpdate) + Send + Sync>),
}

#[async_trait]
pub trait StateListener: Send + Sync {
    async fn on_state_change(&self, update: &ALHStateUpdate);
    async fn on_checkpoint(&self, agent_address: Address, checkpoint_id: B256);
    async fn on_reorg(&self, from_block: u64, to_block: u64);
}

impl EnhancedExExProcessor {
    pub fn new(
        alh_coordinator: Arc<ALHCoordinator>,
        unified_coordinator: Arc<UnifiedCoordinator>,
        memory_store: Arc<MemoryStore>,
        context_retriever: Arc<ContextRetriever>,
    ) -> Self {
        Self {
            alh_coordinator,
            unified_coordinator,
            memory_store,
            context_retriever,
            hook_registry: Arc::new(RwLock::new(Vec::new())),
            state_listeners: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub async fn register_hook(&self, hook: ProcessorHook) {
        self.hook_registry.write().await.push(hook);
    }
    
    pub async fn register_state_listener(&self, listener: Box<dyn StateListener>) {
        self.state_listeners.write().await.push(listener);
    }
    
    async fn execute_pre_hooks(&self, tx: &reth_primitives::TransactionSigned) -> bool {
        for hook in self.hook_registry.read().await.iter() {
            if let ProcessorHook::PreProcessing(func) = hook {
                if !func(tx) {
                    return false;
                }
            }
        }
        true
    }
    
    async fn execute_post_hooks(&self, tx: &reth_primitives::TransactionSigned, decision: &RoutingDecision) {
        for hook in self.hook_registry.read().await.iter() {
            if let ProcessorHook::PostProcessing(func) = hook {
                func(tx, decision);
            }
        }
    }
    
    async fn execute_state_hooks(&self, update: &ALHStateUpdate) {
        for hook in self.hook_registry.read().await.iter() {
            if let ProcessorHook::StateUpdate(func) = hook {
                func(update);
            }
        }
        
        for listener in self.state_listeners.read().await.iter() {
            listener.on_state_change(update).await;
        }
    }
}

#[async_trait]
impl EnhancedProcessor for EnhancedExExProcessor {
    async fn process_with_alh_hooks(&self, transaction: &reth_primitives::TransactionSigned) -> Result<()> {
        if !self.execute_pre_hooks(transaction).await {
            info!("Transaction rejected by pre-processing hooks");
            return Ok(());
        }
        
        let decision = self.unified_coordinator.process_transaction(transaction.clone()).await?;
        
        self.execute_post_hooks(transaction, &decision).await;
        
        if let Some(agent) = decision.assigned_agent {
            let update = ALHStateUpdate {
                update_type: UpdateType::MemoryAdded {
                    memory_type: crate::memory_exex::MemoryType::Working,
                    hash: transaction.hash(),
                },
                agent_address: agent,
                new_state_hash: B256::random(),
                block_number: chrono::Utc::now().timestamp() as u64 / 12,
                proof: None,
            };
            
            self.alh_coordinator.notify_state_update(update.clone()).await?;
            self.execute_state_hooks(&update).await;
        }
        
        Ok(())
    }
    
    async fn query_alh_state(&self, request: ALHQueryRequest) -> Result<serde_json::Value> {
        let response = self.alh_coordinator.process_query(request).await?;
        Ok(serde_json::to_value(response)?)
    }
    
    async fn checkpoint_agent_state(&self, agent_address: Address) -> Result<B256> {
        let request = ALHQueryRequest {
            query_id: B256::random(),
            requester: agent_address,
            query_type: ALHQueryType::CheckpointRequest {
                agent_address,
                checkpoint_id: None,
            },
            timestamp: chrono::Utc::now().timestamp() as u64,
            priority: 1,
            metadata: None,
        };
        
        let response = self.alh_coordinator.process_query(request).await?;
        
        match response.result {
            crate::alh::coordination::ALHQueryResult::CheckpointData { checkpoint_id, .. } => {
                for listener in self.state_listeners.read().await.iter() {
                    listener.on_checkpoint(agent_address, checkpoint_id).await;
                }
                Ok(checkpoint_id)
            }
            _ => Err(eyre::eyre!("Unexpected response from checkpoint request")),
        }
    }
}

pub struct LoggingStateListener;

#[async_trait]
impl StateListener for LoggingStateListener {
    async fn on_state_change(&self, update: &ALHStateUpdate) {
        info!("State change: {:?} for agent {:?}", update.update_type, update.agent_address);
    }
    
    async fn on_checkpoint(&self, agent_address: Address, checkpoint_id: B256) {
        info!("Checkpoint created: {:?} for agent {:?}", checkpoint_id, agent_address);
    }
    
    async fn on_reorg(&self, from_block: u64, to_block: u64) {
        warn!("Reorg detected: {} -> {}", from_block, to_block);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alh::ALHConfig;
    
    #[tokio::test]
    async fn test_enhanced_processor_creation() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        
        let (alh_coordinator, _) = ALHCoordinator::new(
            memory_store.clone(),
            context_retriever.clone(),
            ALHConfig::default(),
        );
        
        let processor = EnhancedExExProcessor::new(
            Arc::new(alh_coordinator),
            Arc::new(UnifiedCoordinator::new(Default::default(), Default::default(), Default::default(), Default::default(), Default::default(), Default::default(), Default::default(), Default::default(), Default::default()).await.0),
            memory_store,
            context_retriever,
        );
        
        processor.register_state_listener(Box::new(LoggingStateListener)).await;
    }
}