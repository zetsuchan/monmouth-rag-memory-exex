//! AI Module - SVM ExEx Compatible AI Traits Implementation
//! 
//! This module provides AI trait implementations that are compatible with
//! the SVM ExEx, enabling seamless integration and cross-ExEx coordination.

pub mod traits;
pub mod decision_engine;
pub mod rag_adapter;
pub mod coordinator;

pub use traits::{
    AIDecisionEngine, RAGEnabledEngine, CrossExExCoordinator,
    TransactionContext, RoutingDecision, DecisionType, TransactionPriority,
    ExecutionFeedback, ConfidenceMetrics, AIEngineState,
    ContextData, EmbeddingData, LearnedPattern, DecisionProposal,
    CoordinatedDecision, SenderHistory, ExecutionResult,
    AlternativeDecision, EngineCapabilities,
};

pub use decision_engine::RagMemoryDecisionEngine;
pub use rag_adapter::RagMemoryRAGAdapter;
pub use coordinator::RagMemoryCrossExExCoordinator;

use eyre::Result;
use std::sync::Arc;

/// Main AI coordinator that implements all SVM ExEx AI traits
pub struct AICoordinator {
    /// Decision engine implementation
    pub decision_engine: Arc<RagMemoryDecisionEngine>,
    
    /// RAG adapter implementation
    pub rag_adapter: Arc<RagMemoryRAGAdapter>,
    
    /// Cross-ExEx coordinator implementation
    pub coordinator: Arc<RagMemoryCrossExExCoordinator>,
}

impl AICoordinator {
    /// Create a new AI coordinator
    pub fn new(
        rag_exex: Arc<tokio::sync::RwLock<crate::rag_exex::RagExEx<impl reth_node_api::FullNodeComponents>>>,
        memory_exex: Arc<tokio::sync::RwLock<crate::memory_exex::MemoryExEx<impl reth_node_api::FullNodeComponents>>>,
        inter_exex: Arc<crate::inter_exex::InterExExCoordinator>,
    ) -> Self {
        let decision_engine = Arc::new(RagMemoryDecisionEngine::new(
            rag_exex.clone(),
            memory_exex.clone(),
        ));
        
        let rag_adapter = Arc::new(RagMemoryRAGAdapter::new(
            rag_exex.clone(),
            memory_exex.clone(),
        ));
        
        let coordinator = Arc::new(RagMemoryCrossExExCoordinator::new(
            inter_exex,
            decision_engine.clone(),
        ));
        
        Self {
            decision_engine,
            rag_adapter,
            coordinator,
        }
    }
    
    /// Get the decision engine as a trait object
    pub fn as_decision_engine(&self) -> Arc<dyn AIDecisionEngine> {
        self.decision_engine.clone()
    }
    
    /// Get the RAG-enabled engine as a trait object
    pub fn as_rag_engine(&self) -> Arc<dyn RAGEnabledEngine> {
        self.rag_adapter.clone()
    }
    
    /// Get the cross-ExEx coordinator as a trait object
    pub fn as_coordinator(&self) -> Arc<dyn CrossExExCoordinator> {
        self.coordinator.clone()
    }
}