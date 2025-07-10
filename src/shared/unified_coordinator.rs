use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use reth_primitives::TransactionSigned;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::interval;

use super::agent_standard::{AgentCapability, AgentRegistry};
use super::agent_state_manager::{AgentStateManager, LifecycleState, PriorityLevel, StateEvent};
use super::ai_agent::{AIAgent, RoutingDecision, TransactionAnalysis};
use super::ai_agent_v2::{EnhancedAIDecisionEngine, EnsembleConfig};
use super::communication::{CrossExExCoordinator, CrossExExMessage, RoutingDecision as CommRoutingDecision};
use super::coordination::{BLSCoordinator, CoordinationMessage};
use super::types::AgentContext;
use crate::context::preprocessing::{ContextPreprocessor, PreprocessedContext};
use crate::memory_exex::agent_integration::AgentMemoryIntegration;
use crate::rag_exex::agent_context::UnifiedContextManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationConfig {
    pub consensus_threshold: f64,
    pub max_pending_transactions: usize,
    pub batch_size: usize,
    pub coordination_timeout_ms: u64,
    pub retry_attempts: u32,
    pub enable_parallel_execution: bool,
}

impl Default for CoordinationConfig {
    fn default() -> Self {
        Self {
            consensus_threshold: 0.67,
            max_pending_transactions: 10000,
            batch_size: 100,
            coordination_timeout_ms: 5000,
            retry_attempts: 3,
            enable_parallel_execution: true,
        }
    }
}

#[derive(Debug)]
pub struct UnifiedCoordinator {
    config: CoordinationConfig,
    ai_engine: Arc<EnhancedAIDecisionEngine>,
    state_manager: Arc<AgentStateManager>,
    agent_registry: Arc<AgentRegistry>,
    bls_coordinator: Arc<BLSCoordinator>,
    cross_exex_coordinator: Arc<CrossExExCoordinator>,
    context_preprocessor: Arc<ContextPreprocessor>,
    context_manager: Arc<UnifiedContextManager>,
    memory_integration: Arc<AgentMemoryIntegration>,
    transaction_queue: Arc<RwLock<VecDeque<PendingTransaction>>>,
    execution_semaphore: Arc<Semaphore>,
    metrics: Arc<RwLock<CoordinationMetrics>>,
    event_tx: mpsc::Sender<CoordinationEvent>,
}

#[derive(Debug, Clone)]
struct PendingTransaction {
    tx: TransactionSigned,
    analysis: TransactionAnalysis,
    routing_decision: RoutingDecision,
    agent_assignment: Option<Address>,
    submitted_at: Instant,
    attempts: u32,
}

#[derive(Debug, Clone)]
pub enum CoordinationEvent {
    TransactionReceived { hash: B256 },
    TransactionRouted { hash: B256, decision: RoutingDecision, agent: Option<Address> },
    ConsensusReached { message_id: String, decision: String },
    ExecutionCompleted { hash: B256, success: bool, duration_ms: u64 },
    AgentAssigned { transaction: B256, agent: Address },
    BatchProcessed { count: usize, duration_ms: u64 },
}

#[derive(Debug, Default)]
struct CoordinationMetrics {
    total_transactions: u64,
    successful_executions: u64,
    failed_executions: u64,
    average_routing_time_ms: f64,
    average_execution_time_ms: f64,
    consensus_rounds: u64,
    agent_assignments: HashMap<Address, u64>,
}

impl UnifiedCoordinator {
    pub async fn new(
        config: CoordinationConfig,
        ai_engine: Arc<EnhancedAIDecisionEngine>,
        state_manager: Arc<AgentStateManager>,
        agent_registry: Arc<AgentRegistry>,
        bls_coordinator: Arc<BLSCoordinator>,
        cross_exex_coordinator: Arc<CrossExExCoordinator>,
        context_preprocessor: Arc<ContextPreprocessor>,
        context_manager: Arc<UnifiedContextManager>,
        memory_integration: Arc<AgentMemoryIntegration>,
    ) -> (Self, mpsc::Receiver<CoordinationEvent>) {
        let (event_tx, event_rx) = mpsc::channel(1000);

        let coordinator = Self {
            config: config.clone(),
            ai_engine,
            state_manager,
            agent_registry,
            bls_coordinator,
            cross_exex_coordinator,
            context_preprocessor,
            context_manager,
            memory_integration,
            transaction_queue: Arc::new(RwLock::new(VecDeque::with_capacity(config.max_pending_transactions))),
            execution_semaphore: Arc::new(Semaphore::new(config.batch_size)),
            metrics: Arc::new(RwLock::new(CoordinationMetrics::default())),
            event_tx,
        };

        // Start background processors
        coordinator.start_transaction_processor();
        coordinator.start_batch_processor();
        coordinator.start_consensus_monitor();

        (coordinator, event_rx)
    }

    pub async fn process_transaction(&self, tx: TransactionSigned) -> Result<()> {
        let start = Instant::now();
        let tx_hash = tx.hash();

        // Send event
        let _ = self.event_tx.send(CoordinationEvent::TransactionReceived { hash: tx_hash }).await;

        // AI analysis and routing decision
        let analysis = self.ai_engine.analyze_transaction(&tx).await?;
        let routing_decision = self.ai_engine.make_routing_decision(&tx).await?;

        // Update routing metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.total_transactions += 1;
            let routing_time = start.elapsed().as_millis() as f64;
            metrics.average_routing_time_ms = 
                (metrics.average_routing_time_ms * (metrics.total_transactions - 1) as f64 + routing_time) 
                / metrics.total_transactions as f64;
        }

        // Process based on routing decision
        match &routing_decision {
            RoutingDecision::Skip => {
                return Ok(());
            }
            RoutingDecision::ProcessWithRAG | 
            RoutingDecision::ProcessWithMemory | 
            RoutingDecision::ProcessWithBoth => {
                self.route_to_exex(&tx, &analysis, &routing_decision).await?;
            }
            RoutingDecision::ProcessWithSVM => {
                self.route_to_agent(&tx, &analysis, &routing_decision).await?;
            }
        }

        Ok(())
    }

    async fn route_to_exex(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        routing: &RoutingDecision,
    ) -> Result<()> {
        // Create cross-ExEx message
        let message = CrossExExMessage::TransactionAnalysis {
            transaction_hash: tx.hash(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            from: Address::default(), // Would extract from tx
            to: tx.to().unwrap_or_default(),
            value: tx.value(),
            gas_used: tx.gas_limit(),
            routing_decision: match routing {
                RoutingDecision::ProcessWithRAG => CommRoutingDecision::RAG,
                RoutingDecision::ProcessWithMemory => CommRoutingDecision::Memory,
                RoutingDecision::ProcessWithBoth => CommRoutingDecision::Both,
                _ => CommRoutingDecision::SVM,
            },
            context: vec![],
        };

        // Send to appropriate ExEx
        self.cross_exex_coordinator.send_message(message).await?;

        let _ = self.event_tx.send(CoordinationEvent::TransactionRouted {
            hash: tx.hash(),
            decision: routing.clone(),
            agent: None,
        }).await;

        Ok(())
    }

    async fn route_to_agent(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        routing: &RoutingDecision,
    ) -> Result<()> {
        // Determine required capabilities
        let required_capabilities = self.determine_required_capabilities(analysis);

        // Select best agent
        let agent = self.state_manager
            .select_agent_for_task(required_capabilities, PriorityLevel::Normal)
            .await
            .ok_or_else(|| eyre::eyre!("No suitable agent available"))?;

        // Create pending transaction
        let pending = PendingTransaction {
            tx: tx.clone(),
            analysis: analysis.clone(),
            routing_decision: routing.clone(),
            agent_assignment: Some(agent),
            submitted_at: Instant::now(),
            attempts: 0,
        };

        // Add to queue
        {
            let mut queue = self.transaction_queue.write().await;
            if queue.len() >= self.config.max_pending_transactions {
                queue.pop_front(); // Drop oldest
            }
            queue.push_back(pending);
        }

        let _ = self.event_tx.send(CoordinationEvent::TransactionRouted {
            hash: tx.hash(),
            decision: routing.clone(),
            agent: Some(agent),
        }).await;

        let _ = self.event_tx.send(CoordinationEvent::AgentAssigned {
            transaction: tx.hash(),
            agent,
        }).await;

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            *metrics.agent_assignments.entry(agent).or_insert(0) += 1;
        }

        Ok(())
    }

    pub async fn coordinate_consensus(
        &self,
        message: CoordinationMessage,
    ) -> Result<bool> {
        let start = Instant::now();
        
        // Get active agents for consensus
        let active_agents = self.state_manager.get_active_agents().await;
        if active_agents.is_empty() {
            return Err(eyre::eyre!("No active agents for consensus"));
        }

        // Check if we need consensus based on message type
        let needs_consensus = match &message {
            CoordinationMessage::AgentProposal { .. } => true,
            CoordinationMessage::StateUpdate { .. } => true,
            CoordinationMessage::EmergencyAction { .. } => true,
            _ => false,
        };

        if !needs_consensus {
            return Ok(true);
        }

        // Initiate BLS consensus
        let threshold = (active_agents.len() as f64 * self.config.consensus_threshold).ceil() as usize;
        let consensus = self.bls_coordinator
            .initiate_consensus(&message, threshold)
            .await?;

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.consensus_rounds += 1;
        }

        let _ = self.event_tx.send(CoordinationEvent::ConsensusReached {
            message_id: format!("{:?}", message),
            decision: consensus.to_string(),
        }).await;

        Ok(consensus)
    }

    pub async fn execute_with_coordination(
        &self,
        transaction: PendingTransaction,
    ) -> Result<()> {
        let _permit = self.execution_semaphore.acquire().await?;
        let start = Instant::now();
        let tx_hash = transaction.tx.hash();

        // Verify agent is still active
        if let Some(agent_address) = transaction.agent_assignment {
            let agent_state = self.state_manager.get_agent_state(agent_address).await;
            if agent_state.map(|s| s.lifecycle_state != LifecycleState::Active).unwrap_or(true) {
                // Reassign to another agent
                return self.reassign_transaction(transaction).await;
            }
        }

        // Prepare context
        let context = self.prepare_execution_context(&transaction).await?;

        // Execute based on routing
        let success = match transaction.routing_decision {
            RoutingDecision::ProcessWithRAG => {
                self.execute_with_rag(&transaction, &context).await?
            }
            RoutingDecision::ProcessWithMemory => {
                self.execute_with_memory(&transaction, &context).await?
            }
            RoutingDecision::ProcessWithBoth => {
                self.execute_with_both(&transaction, &context).await?
            }
            RoutingDecision::ProcessWithSVM => {
                self.execute_with_svm(&transaction, &context).await?
            }
            _ => false,
        };

        let duration = start.elapsed().as_millis() as u64;

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            if success {
                metrics.successful_executions += 1;
            } else {
                metrics.failed_executions += 1;
            }
            metrics.average_execution_time_ms = 
                (metrics.average_execution_time_ms * (metrics.successful_executions + metrics.failed_executions - 1) as f64 + duration as f64) 
                / (metrics.successful_executions + metrics.failed_executions) as f64;
        }

        // Update agent performance
        if let Some(agent) = transaction.agent_assignment {
            self.state_manager.update_performance(agent, success, duration as f64).await?;
        }

        let _ = self.event_tx.send(CoordinationEvent::ExecutionCompleted {
            hash: tx_hash,
            success,
            duration_ms: duration,
        }).await;

        Ok(())
    }

    async fn prepare_execution_context(
        &self,
        transaction: &PendingTransaction,
    ) -> Result<ExecutionContext> {
        let from_address = Address::default(); // Would extract from transaction

        // Get agent context
        let agent_context = self.context_manager.get_or_create_context(from_address).await;

        // Preprocess transaction
        let preprocessed = self.context_preprocessor
            .preprocess_transaction(&transaction.analysis, &agent_context.current_context)
            .await?;

        // Build memory-aware context
        let memory_contexts = self.memory_integration
            .build_memory_aware_context(from_address, &preprocessed)
            .await?;

        Ok(ExecutionContext {
            transaction: transaction.clone(),
            agent_context,
            preprocessed_context: preprocessed,
            memory_contexts,
            created_at: Instant::now(),
        })
    }

    async fn execute_with_rag(
        &self,
        transaction: &PendingTransaction,
        context: &ExecutionContext,
    ) -> Result<bool> {
        // Send to RAG ExEx
        let message = CrossExExMessage::RAGQuery {
            query_id: B256::random(),
            agent_id: context.agent_context.agent_address,
            query: format!("Process transaction: {:?}", transaction.tx.hash()),
            context: vec![],
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        self.cross_exex_coordinator.send_message(message).await?;
        Ok(true)
    }

    async fn execute_with_memory(
        &self,
        transaction: &PendingTransaction,
        context: &ExecutionContext,
    ) -> Result<bool> {
        // Store in memory
        self.memory_integration
            .store_agent_memory(
                context.agent_context.agent_address,
                &context.preprocessed_context,
                crate::shared::types::MemoryType::Working,
            )
            .await?;
        
        Ok(true)
    }

    async fn execute_with_both(
        &self,
        transaction: &PendingTransaction,
        context: &ExecutionContext,
    ) -> Result<bool> {
        // Execute both in parallel
        let rag_future = self.execute_with_rag(transaction, context);
        let memory_future = self.execute_with_memory(transaction, context);

        let (rag_result, memory_result) = tokio::join!(rag_future, memory_future);
        
        Ok(rag_result.is_ok() && memory_result.is_ok())
    }

    async fn execute_with_svm(
        &self,
        transaction: &PendingTransaction,
        context: &ExecutionContext,
    ) -> Result<bool> {
        // Direct SVM execution
        // In practice, this would interface with the SVM
        Ok(true)
    }

    async fn reassign_transaction(&self, mut transaction: PendingTransaction) -> Result<()> {
        transaction.attempts += 1;
        
        if transaction.attempts > self.config.retry_attempts {
            return Err(eyre::eyre!("Max retry attempts exceeded"));
        }

        // Find new agent
        let required_capabilities = self.determine_required_capabilities(&transaction.analysis);
        let new_agent = self.state_manager
            .select_agent_for_task(required_capabilities, PriorityLevel::High)
            .await;

        if let Some(agent) = new_agent {
            transaction.agent_assignment = Some(agent);
            
            // Re-queue
            let mut queue = self.transaction_queue.write().await;
            queue.push_front(transaction);
            
            Ok(())
        } else {
            Err(eyre::eyre!("No alternative agent available"))
        }
    }

    fn determine_required_capabilities(&self, analysis: &TransactionAnalysis) -> Vec<String> {
        let mut capabilities = Vec::new();

        if analysis.features.is_contract_creation {
            capabilities.push("contract_deployment".to_string());
        }
        if analysis.features.is_token_transfer {
            capabilities.push("token_transfer".to_string());
        }
        if analysis.features.has_complex_data {
            capabilities.push("complex_execution".to_string());
        }
        if analysis.complexity_score > 0.8 {
            capabilities.push("high_complexity".to_string());
        }

        if capabilities.is_empty() {
            capabilities.push("basic_execution".to_string());
        }

        capabilities
    }

    fn start_transaction_processor(&self) {
        let queue = self.transaction_queue.clone();
        let coordinator = self.clone();

        tokio::spawn(async move {
            loop {
                let transaction = {
                    let mut q = queue.write().await;
                    q.pop_front()
                };

                if let Some(tx) = transaction {
                    if let Err(e) = coordinator.execute_with_coordination(tx).await {
                        eprintln!("Execution error: {}", e);
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        });
    }

    fn start_batch_processor(&self) {
        let queue = self.transaction_queue.clone();
        let coordinator = self.clone();
        let batch_size = self.config.batch_size;

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                let batch: Vec<_> = {
                    let mut q = queue.write().await;
                    let count = batch_size.min(q.len());
                    (0..count).filter_map(|_| q.pop_front()).collect()
                };

                if !batch.is_empty() {
                    let start = Instant::now();
                    let count = batch.len();

                    // Process batch in parallel
                    let futures: Vec<_> = batch.into_iter()
                        .map(|tx| coordinator.execute_with_coordination(tx))
                        .collect();

                    let _results = futures::future::join_all(futures).await;

                    let duration = start.elapsed().as_millis() as u64;
                    let _ = coordinator.event_tx.send(CoordinationEvent::BatchProcessed {
                        count,
                        duration_ms: duration,
                    }).await;
                }
            }
        });
    }

    fn start_consensus_monitor(&self) {
        let bls_coordinator = self.bls_coordinator.clone();
        let coordinator = self.clone();

        tokio::spawn(async move {
            // Monitor for consensus requests
            // In practice, this would listen to a channel from BLS coordinator
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                // Check for pending consensus operations
            }
        });
    }

    pub async fn get_metrics(&self) -> CoordinationMetrics {
        self.metrics.read().await.clone()
    }
}

impl Clone for UnifiedCoordinator {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            ai_engine: self.ai_engine.clone(),
            state_manager: self.state_manager.clone(),
            agent_registry: self.agent_registry.clone(),
            bls_coordinator: self.bls_coordinator.clone(),
            cross_exex_coordinator: self.cross_exex_coordinator.clone(),
            context_preprocessor: self.context_preprocessor.clone(),
            context_manager: self.context_manager.clone(),
            memory_integration: self.memory_integration.clone(),
            transaction_queue: self.transaction_queue.clone(),
            execution_semaphore: self.execution_semaphore.clone(),
            metrics: self.metrics.clone(),
            event_tx: self.event_tx.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct ExecutionContext {
    transaction: PendingTransaction,
    agent_context: UnifiedAgentContext,
    preprocessed_context: PreprocessedContext,
    memory_contexts: Vec<B256>,
    created_at: Instant,
}

// Coordination strategies
pub trait CoordinationStrategy: Send + Sync {
    fn should_coordinate(&self, transaction: &PendingTransaction) -> bool;
    fn select_coordinators(&self, agents: &[Address]) -> Vec<Address>;
}

pub struct ThresholdCoordinationStrategy {
    complexity_threshold: f64,
    min_coordinators: usize,
}

impl CoordinationStrategy for ThresholdCoordinationStrategy {
    fn should_coordinate(&self, transaction: &PendingTransaction) -> bool {
        transaction.analysis.complexity_score > self.complexity_threshold
    }

    fn select_coordinators(&self, agents: &[Address]) -> Vec<Address> {
        agents.iter()
            .take(self.min_coordinators)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::preprocessing::ProcessingConfig;
    use crate::memory_exex::memory_store::MemoryStore;
    use crate::rag_exex::context_retrieval::ContextRetriever;

    #[tokio::test]
    async fn test_unified_coordinator_creation() {
        // This would require setting up all dependencies
        // For now, just test that the types compile correctly
    }

    #[tokio::test]
    async fn test_coordination_strategy() {
        let strategy = ThresholdCoordinationStrategy {
            complexity_threshold: 0.7,
            min_coordinators: 3,
        };

        let transaction = PendingTransaction {
            tx: TransactionSigned::default(),
            analysis: TransactionAnalysis {
                complexity_score: 0.8,
                safety_score: 0.9,
                gas_estimate: 21000,
                routing_suggestion: RoutingDecision::ProcessWithRAG,
                confidence: 0.85,
                features: Default::default(),
                timestamp: Instant::now(),
            },
            routing_decision: RoutingDecision::ProcessWithRAG,
            agent_assignment: None,
            submitted_at: Instant::now(),
            attempts: 0,
        };

        assert!(strategy.should_coordinate(&transaction));
    }
}