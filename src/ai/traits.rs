//! AI Decision Engine Traits - Re-exported from SVM ExEx
//! 
//! This module re-exports the AI traits from the SVM ExEx to ensure compatibility.
//! Since we cannot directly import from the SVM ExEx crate, we duplicate the trait
//! definitions here. These must be kept in sync with the SVM ExEx versions.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use eyre::Result;

/// Core trait for AI decision engines
#[async_trait]
pub trait AIDecisionEngine: Send + Sync {
    /// Analyze a transaction and make a routing decision
    async fn analyze_transaction(&self, context: TransactionContext) -> Result<RoutingDecision>;
    
    /// Update the engine with feedback from executed transactions
    async fn update_with_feedback(&self, feedback: ExecutionFeedback) -> Result<()>;
    
    /// Get current confidence metrics
    async fn get_confidence_metrics(&self) -> Result<ConfidenceMetrics>;
    
    /// Export the current state for synchronization
    async fn export_state(&self) -> Result<AIEngineState>;
    
    /// Import state from another engine
    async fn import_state(&self, state: AIEngineState) -> Result<()>;
    
    /// Get engine capabilities
    fn capabilities(&self) -> EngineCapabilities;
}

/// Extended trait for engines with RAG capabilities
#[async_trait]
pub trait RAGEnabledEngine: AIDecisionEngine {
    /// Store context for future retrieval
    async fn store_context(&self, key: String, context: ContextData) -> Result<()>;
    
    /// Retrieve relevant context for a query
    async fn retrieve_context(&self, query: &str, limit: usize) -> Result<Vec<ContextData>>;
    
    /// Update embeddings with new data
    async fn update_embeddings(&self, data: Vec<EmbeddingData>) -> Result<()>;
}

/// Extended trait for engines with cross-ExEx coordination
#[async_trait]
pub trait CrossExExCoordinator: AIDecisionEngine {
    /// Coordinate decision with other ExEx instances
    async fn coordinate_decision(&self, proposals: Vec<DecisionProposal>) -> Result<CoordinatedDecision>;
    
    /// Share learned patterns with other engines
    async fn share_patterns(&self) -> Result<Vec<LearnedPattern>>;
    
    /// Integrate patterns from other engines
    async fn integrate_patterns(&self, patterns: Vec<LearnedPattern>) -> Result<()>;
}

/// Transaction context for AI analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionContext {
    /// Transaction hash
    pub tx_hash: [u8; 32],
    /// Sender address
    pub sender: [u8; 20],
    /// Transaction data
    pub data: Vec<u8>,
    /// Gas price
    pub gas_price: u64,
    /// Current network congestion level
    pub congestion_level: f32,
    /// Historical data about the sender
    pub sender_history: Option<SenderHistory>,
    /// Related transactions (for batching)
    pub related_txs: Vec<[u8; 32]>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Routing decision made by the AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// The decision type
    pub decision: DecisionType,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// Reasoning for the decision
    pub reasoning: String,
    /// Suggested priority
    pub priority: TransactionPriority,
    /// Alternative decisions considered
    pub alternatives: Vec<AlternativeDecision>,
}

/// Types of routing decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionType {
    /// Route to SVM for execution
    RouteToSVM,
    /// Keep in EVM
    KeepInEVM,
    /// Defer execution
    Defer,
    /// Reject transaction
    Reject,
    /// Split execution between VMs
    Split,
}

/// Transaction priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TransactionPriority {
    Critical,
    High,
    Normal,
    Low,
    Deferred,
}

/// Alternative decision that was considered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeDecision {
    pub decision: DecisionType,
    pub confidence: f64,
    pub reason_not_chosen: String,
}

/// Feedback from transaction execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionFeedback {
    /// Original transaction hash
    pub tx_hash: [u8; 32],
    /// The decision that was made
    pub decision_made: RoutingDecision,
    /// Execution result
    pub result: ExecutionResult,
    /// Performance metrics
    pub metrics: ExecutionMetrics,
    /// Timestamp
    pub timestamp: u64,
}

/// Result of transaction execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionResult {
    /// Successful execution
    Success {
        gas_used: u64,
        output: Vec<u8>,
    },
    /// Failed execution
    Failed {
        reason: String,
        gas_used: u64,
    },
    /// Execution was reverted
    Reverted {
        reason: String,
    },
}

/// Execution performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Queue wait time in milliseconds
    pub queue_time_ms: u64,
    /// Memory used in bytes
    pub memory_used: u64,
    /// Number of accounts accessed
    pub accounts_accessed: u32,
}

/// Confidence metrics for the AI engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceMetrics {
    /// Overall confidence score
    pub overall_confidence: f64,
    /// Confidence by decision type
    pub decision_confidence: HashMap<DecisionType, f64>,
    /// Recent accuracy rate
    pub recent_accuracy: f64,
    /// Number of decisions made
    pub total_decisions: u64,
    /// Learning rate
    pub learning_rate: f64,
}

/// AI engine state for synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIEngineState {
    /// Engine version
    pub version: String,
    /// Model parameters
    pub model_params: ModelParameters,
    /// Learned patterns
    pub patterns: Vec<LearnedPattern>,
    /// Performance statistics
    pub stats: PerformanceStats,
    /// Timestamp of state export
    pub timestamp: u64,
}

/// Model parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelParameters {
    /// Model weights (serialized)
    pub weights: Vec<u8>,
    /// Hyperparameters
    pub hyperparams: HashMap<String, f64>,
    /// Feature importance scores
    pub feature_importance: HashMap<String, f64>,
}

/// Learned pattern from transaction analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedPattern {
    /// Pattern identifier
    pub id: String,
    /// Pattern description
    pub description: String,
    /// Pattern matching criteria
    pub criteria: PatternCriteria,
    /// Recommended action
    pub action: DecisionType,
    /// Success rate
    pub success_rate: f64,
    /// Number of observations
    pub observations: u64,
}

/// Criteria for pattern matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCriteria {
    /// Contract address patterns
    pub contract_patterns: Vec<String>,
    /// Function selector patterns
    pub function_patterns: Vec<String>,
    /// Gas price ranges
    pub gas_price_range: Option<(u64, u64)>,
    /// Data size ranges
    pub data_size_range: Option<(usize, usize)>,
    /// Additional conditions
    pub conditions: HashMap<String, serde_json::Value>,
}

/// Performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceStats {
    /// Total transactions analyzed
    pub total_analyzed: u64,
    /// Successful predictions
    pub successful_predictions: u64,
    /// Average decision time (ms)
    pub avg_decision_time_ms: f64,
    /// Memory usage (bytes)
    pub memory_usage: u64,
}

/// Engine capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCapabilities {
    /// Supports RAG
    pub rag_enabled: bool,
    /// Supports cross-ExEx coordination
    pub cross_exex_enabled: bool,
    /// Maximum transactions per second
    pub max_tps: u32,
    /// Supported decision types
    pub supported_decisions: Vec<DecisionType>,
    /// Model type
    pub model_type: String,
}

/// Context data for RAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextData {
    /// Context identifier
    pub id: String,
    /// Content
    pub content: String,
    /// Embedding vector
    pub embedding: Vec<f32>,
    /// Metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// Timestamp
    pub timestamp: u64,
}

/// Embedding data for updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingData {
    /// Content to embed
    pub content: String,
    /// Pre-computed embedding (optional)
    pub embedding: Option<Vec<f32>>,
    /// Associated metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Sender history for context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderHistory {
    /// Total transactions
    pub total_txs: u64,
    /// Success rate
    pub success_rate: f64,
    /// Average gas price
    pub avg_gas_price: u64,
    /// Common contracts interacted with
    pub common_contracts: Vec<[u8; 20]>,
}

/// Decision proposal from an ExEx
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionProposal {
    /// ExEx that made the proposal
    pub exex_id: String,
    /// Proposed decision
    pub decision: RoutingDecision,
    /// Supporting evidence
    pub evidence: Vec<String>,
    /// Timestamp
    pub timestamp: u64,
}

/// Coordinated decision from multiple ExEx instances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatedDecision {
    /// Final decision
    pub decision: RoutingDecision,
    /// Consensus level (0.0 to 1.0)
    pub consensus: f64,
    /// Individual proposals
    pub proposals: Vec<DecisionProposal>,
    /// Aggregation method used
    pub method: String,
}