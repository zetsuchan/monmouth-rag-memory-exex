//! Cross-ExEx Coordinator Implementation
//! 
//! Implements the CrossExExCoordinator trait for coordinating decisions
//! and sharing patterns between ExEx instances.

use super::{decision_engine::RagMemoryDecisionEngine, traits::*};
use crate::inter_exex::{InterExExCoordinator, ExExMessage, MessageType, MessagePayload};
use async_trait::async_trait;
use eyre::Result;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::RwLock;

/// Cross-ExEx coordinator for decision coordination and pattern sharing
pub struct RagMemoryCrossExExCoordinator {
    /// Base decision engine
    decision_engine: Arc<RagMemoryDecisionEngine>,
    
    /// Inter-ExEx communication coordinator
    inter_exex: Arc<InterExExCoordinator>,
    
    /// Shared patterns from other ExEx instances
    shared_patterns: Arc<RwLock<Vec<LearnedPattern>>>,
}

impl RagMemoryCrossExExCoordinator {
    /// Create a new cross-ExEx coordinator
    pub fn new(
        inter_exex: Arc<InterExExCoordinator>,
        decision_engine: Arc<RagMemoryDecisionEngine>,
    ) -> Self {
        Self {
            decision_engine,
            inter_exex,
            shared_patterns: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// Convert internal message to decision proposal
    fn message_to_proposal(&self, message: ExExMessage) -> Option<DecisionProposal> {
        match message.payload {
            MessagePayload::Data(data) => {
                // Try to deserialize as a decision proposal
                if let Ok(proposal) = serde_json::from_slice::<DecisionProposal>(&data) {
                    Some(proposal)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    
    /// Aggregate multiple proposals into a coordinated decision
    fn aggregate_proposals(&self, proposals: Vec<DecisionProposal>) -> CoordinatedDecision {
        if proposals.is_empty() {
            return CoordinatedDecision {
                decision: RoutingDecision {
                    decision: DecisionType::KeepInEVM,
                    confidence: 0.5,
                    reasoning: "No proposals received".to_string(),
                    priority: TransactionPriority::Normal,
                    alternatives: vec![],
                },
                consensus: 0.0,
                proposals: vec![],
                method: "default".to_string(),
            };
        }
        
        // Count votes for each decision type
        let mut decision_votes: HashMap<DecisionType, f64> = HashMap::new();
        let mut total_confidence = 0.0;
        
        for proposal in &proposals {
            let weight = proposal.decision.confidence;
            *decision_votes.entry(proposal.decision.decision).or_insert(0.0) += weight;
            total_confidence += weight;
        }
        
        // Find decision with highest weighted votes
        let (best_decision, weighted_votes) = decision_votes
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(d, v)| (*d, *v))
            .unwrap_or((DecisionType::KeepInEVM, 0.0));
        
        // Calculate consensus level
        let consensus = if total_confidence > 0.0 {
            weighted_votes / total_confidence
        } else {
            0.0
        };
        
        // Create final decision
        let final_decision = RoutingDecision {
            decision: best_decision,
            confidence: consensus,
            reasoning: format!(
                "Consensus decision from {} proposals with {:.2}% agreement",
                proposals.len(),
                consensus * 100.0
            ),
            priority: if consensus > 0.8 {
                TransactionPriority::High
            } else {
                TransactionPriority::Normal
            },
            alternatives: vec![],
        };
        
        CoordinatedDecision {
            decision: final_decision,
            consensus,
            proposals,
            method: "weighted_voting".to_string(),
        }
    }
}

// Implement AIDecisionEngine by delegating to the inner decision engine
#[async_trait]
impl AIDecisionEngine for RagMemoryCrossExExCoordinator {
    async fn analyze_transaction(&self, context: TransactionContext) -> Result<RoutingDecision> {
        self.decision_engine.analyze_transaction(context).await
    }
    
    async fn update_with_feedback(&self, feedback: ExecutionFeedback) -> Result<()> {
        self.decision_engine.update_with_feedback(feedback).await
    }
    
    async fn get_confidence_metrics(&self) -> Result<ConfidenceMetrics> {
        self.decision_engine.get_confidence_metrics().await
    }
    
    async fn export_state(&self) -> Result<AIEngineState> {
        self.decision_engine.export_state().await
    }
    
    async fn import_state(&self, state: AIEngineState) -> Result<()> {
        self.decision_engine.import_state(state).await
    }
    
    fn capabilities(&self) -> EngineCapabilities {
        let mut capabilities = self.decision_engine.capabilities();
        capabilities.cross_exex_enabled = true;
        capabilities
    }
}

// Implement CrossExExCoordinator trait
#[async_trait]
impl CrossExExCoordinator for RagMemoryCrossExExCoordinator {
    async fn coordinate_decision(&self, proposals: Vec<DecisionProposal>) -> Result<CoordinatedDecision> {
        // Broadcast our own proposal if we have one
        if let Some(our_proposal) = proposals.iter().find(|p| p.exex_id == "rag_memory_exex") {
            let message = ExExMessage::new(
                MessageType::ConsensusVote,
                "rag_memory_exex".to_string(),
                MessagePayload::Data(serde_json::to_vec(our_proposal)?),
            );
            
            self.inter_exex.broadcast(message).await?;
        }
        
        // Subscribe to consensus votes
        let mut receiver = self.inter_exex.subscribe(MessageType::ConsensusVote).await?;
        
        // Collect proposals from other ExEx instances
        let mut all_proposals = proposals;
        let timeout = tokio::time::Duration::from_millis(100);
        let start = tokio::time::Instant::now();
        
        while start.elapsed() < timeout {
            match tokio::time::timeout(
                timeout - start.elapsed(),
                receiver.recv()
            ).await {
                Ok(Some(message)) => {
                    if let Some(proposal) = self.message_to_proposal(message) {
                        // Don't add duplicates
                        if !all_proposals.iter().any(|p| p.exex_id == proposal.exex_id) {
                            all_proposals.push(proposal);
                        }
                    }
                }
                Ok(None) | Err(_) => break,
            }
        }
        
        // Aggregate all proposals
        Ok(self.aggregate_proposals(all_proposals))
    }
    
    async fn share_patterns(&self) -> Result<Vec<LearnedPattern>> {
        // Get patterns from decision engine
        let state = self.decision_engine.export_state().await?;
        let patterns = state.patterns;
        
        // Broadcast patterns to other ExEx instances
        for pattern in &patterns {
            let message = ExExMessage::new(
                MessageType::Data,
                "rag_memory_exex".to_string(),
                MessagePayload::Data(serde_json::to_vec(&pattern)?),
            );
            
            self.inter_exex.broadcast(message).await?;
        }
        
        Ok(patterns)
    }
    
    async fn integrate_patterns(&self, patterns: Vec<LearnedPattern>) -> Result<()> {
        // Store patterns from other ExEx instances
        let mut shared_patterns = self.shared_patterns.write().await;
        
        for pattern in patterns {
            // Check if pattern already exists
            if !shared_patterns.iter().any(|p| p.id == pattern.id) {
                shared_patterns.push(pattern);
            }
        }
        
        // Keep only recent patterns
        if shared_patterns.len() > 1000 {
            shared_patterns.drain(0..shared_patterns.len() - 1000);
        }
        
        // Update decision engine with new patterns
        let current_state = self.decision_engine.export_state().await?;
        let mut updated_state = current_state;
        
        // Merge shared patterns with existing patterns
        for pattern in shared_patterns.iter() {
            if !updated_state.patterns.iter().any(|p| p.id == pattern.id) {
                updated_state.patterns.push(pattern.clone());
            }
        }
        
        self.decision_engine.import_state(updated_state).await?;
        
        Ok(())
    }
}