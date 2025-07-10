use alloy::primitives::{Address, B256, U256};
use async_trait::async_trait;
use eyre::Result;
use reth_primitives::TransactionSigned;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::ai_agent::{
    AIAgent, ActualMetrics, PredictedMetrics, RoutingDecision, TransactionAnalysis,
    TransactionFeatures,
};
use crate::context::preprocessing::PreprocessedContext;
use crate::rag_exex::agent_context::UnifiedAgentContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleConfig {
    pub models: Vec<ModelConfig>,
    pub voting_strategy: VotingStrategy,
    pub confidence_threshold: f64,
    pub adaptation_rate: f64,
    pub history_window: usize,
    pub cache_ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub model_type: ModelType,
    pub weight: f64,
    pub specialization: Vec<String>,
    pub performance_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelType {
    NeuralNetwork,
    DecisionTree,
    RuleBased,
    StatisticalModel,
    HybridModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VotingStrategy {
    WeightedAverage,
    Majority,
    ConfidenceWeighted,
    Adaptive,
}

#[derive(Debug, Clone)]
pub struct ModelDecision {
    pub routing: RoutingDecision,
    pub confidence: f64,
    pub reasoning: String,
    pub features_importance: HashMap<String, f64>,
}

#[derive(Debug)]
pub struct EnhancedAIDecisionEngine {
    config: EnsembleConfig,
    models: Vec<Arc<dyn AIModel>>,
    performance_history: Arc<RwLock<PerformanceHistory>>,
    decision_cache: Arc<RwLock<lru::LruCache<B256, CachedDecision>>>,
    context_store: Arc<RwLock<HashMap<Address, VecDeque<PreprocessedContext>>>>,
}

#[derive(Debug, Clone)]
struct PerformanceHistory {
    model_accuracy: HashMap<String, ModelPerformance>,
    routing_outcomes: VecDeque<RoutingOutcome>,
    adaptation_history: VecDeque<AdaptationEvent>,
}

#[derive(Debug, Clone)]
struct ModelPerformance {
    correct_predictions: u64,
    total_predictions: u64,
    average_confidence: f64,
    specialization_accuracy: HashMap<String, f64>,
    recent_performance: VecDeque<(Instant, bool)>,
}

#[derive(Debug, Clone)]
struct RoutingOutcome {
    transaction_hash: B256,
    decision: RoutingDecision,
    actual_optimal: RoutingDecision,
    execution_metrics: ActualMetrics,
    timestamp: Instant,
}

#[derive(Debug, Clone)]
struct AdaptationEvent {
    timestamp: Instant,
    model_name: String,
    old_weight: f64,
    new_weight: f64,
    reason: String,
}

#[derive(Debug, Clone)]
struct CachedDecision {
    decision: RoutingDecision,
    confidence: f64,
    timestamp: Instant,
    context_hash: B256,
}

impl Default for EnsembleConfig {
    fn default() -> Self {
        Self {
            models: vec![
                ModelConfig {
                    name: "neural_net_v1".to_string(),
                    model_type: ModelType::NeuralNetwork,
                    weight: 0.4,
                    specialization: vec!["complex_contracts".to_string()],
                    performance_threshold: 0.8,
                },
                ModelConfig {
                    name: "decision_tree_v1".to_string(),
                    model_type: ModelType::DecisionTree,
                    weight: 0.3,
                    specialization: vec!["token_transfers".to_string()],
                    performance_threshold: 0.85,
                },
                ModelConfig {
                    name: "rule_based_v1".to_string(),
                    model_type: ModelType::RuleBased,
                    weight: 0.3,
                    specialization: vec!["simple_transfers".to_string()],
                    performance_threshold: 0.9,
                },
            ],
            voting_strategy: VotingStrategy::ConfidenceWeighted,
            confidence_threshold: 0.7,
            adaptation_rate: 0.1,
            history_window: 1000,
            cache_ttl_seconds: 60,
        }
    }
}

impl EnhancedAIDecisionEngine {
    pub fn new(config: EnsembleConfig) -> Result<Self> {
        let cache_size = std::num::NonZeroUsize::new(10000).unwrap();
        
        let models: Vec<Arc<dyn AIModel>> = config.models.iter().map(|model_config| {
            match model_config.model_type {
                ModelType::NeuralNetwork => Arc::new(NeuralNetworkModel::new(model_config.clone())) as Arc<dyn AIModel>,
                ModelType::DecisionTree => Arc::new(DecisionTreeModel::new(model_config.clone())) as Arc<dyn AIModel>,
                ModelType::RuleBased => Arc::new(RuleBasedModel::new(model_config.clone())) as Arc<dyn AIModel>,
                ModelType::StatisticalModel => Arc::new(StatisticalModel::new(model_config.clone())) as Arc<dyn AIModel>,
                ModelType::HybridModel => Arc::new(HybridModel::new(model_config.clone())) as Arc<dyn AIModel>,
            }
        }).collect();

        Ok(Self {
            config,
            models,
            performance_history: Arc::new(RwLock::new(PerformanceHistory {
                model_accuracy: HashMap::new(),
                routing_outcomes: VecDeque::with_capacity(1000),
                adaptation_history: VecDeque::with_capacity(100),
            })),
            decision_cache: Arc::new(RwLock::new(lru::LruCache::new(cache_size))),
            context_store: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn add_agent_context(
        &self,
        agent_address: Address,
        context: PreprocessedContext,
    ) {
        let mut store = self.context_store.write().await;
        let contexts = store.entry(agent_address).or_insert_with(|| VecDeque::with_capacity(100));
        
        contexts.push_back(context);
        if contexts.len() > 100 {
            contexts.pop_front();
        }
    }

    async fn get_agent_contexts(&self, agent_address: Address) -> Vec<PreprocessedContext> {
        let store = self.context_store.read().await;
        store.get(&agent_address)
            .map(|contexts| contexts.iter().cloned().collect())
            .unwrap_or_default()
    }

    async fn ensemble_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<(RoutingDecision, f64, HashMap<String, f64>)> {
        let mut model_decisions = Vec::new();
        let mut feature_importance_aggregate = HashMap::new();

        // Collect decisions from all models
        for model in &self.models {
            let decision = model.make_decision(tx, analysis, contexts).await?;
            model_decisions.push((model.get_name(), decision));
        }

        // Apply voting strategy
        let (final_decision, confidence, feature_importance) = match self.config.voting_strategy {
            VotingStrategy::WeightedAverage => self.weighted_average_voting(&model_decisions).await,
            VotingStrategy::Majority => self.majority_voting(&model_decisions).await,
            VotingStrategy::ConfidenceWeighted => self.confidence_weighted_voting(&model_decisions).await,
            VotingStrategy::Adaptive => self.adaptive_voting(&model_decisions, analysis).await,
        };

        Ok((final_decision, confidence, feature_importance))
    }

    async fn weighted_average_voting(
        &self,
        decisions: &[(String, ModelDecision)],
    ) -> (RoutingDecision, f64, HashMap<String, f64>) {
        let history = self.performance_history.read().await;
        let mut routing_scores: HashMap<RoutingDecision, f64> = HashMap::new();
        let mut total_weight = 0.0;
        let mut feature_importance = HashMap::new();

        for (model_name, decision) in decisions {
            let model_config = self.config.models.iter()
                .find(|m| &m.name == model_name)
                .unwrap();
            
            let weight = if let Some(perf) = history.model_accuracy.get(model_name) {
                let accuracy = perf.correct_predictions as f64 / perf.total_predictions.max(1) as f64;
                model_config.weight * accuracy
            } else {
                model_config.weight
            };

            *routing_scores.entry(decision.routing.clone()).or_insert(0.0) += weight * decision.confidence;
            total_weight += weight;

            // Aggregate feature importance
            for (feature, importance) in &decision.features_importance {
                *feature_importance.entry(feature.clone()).or_insert(0.0) += importance * weight;
            }
        }

        // Normalize feature importance
        for importance in feature_importance.values_mut() {
            *importance /= total_weight.max(1.0);
        }

        // Find decision with highest score
        let (final_decision, score) = routing_scores.into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((RoutingDecision::Skip, 0.0));

        let confidence = score / total_weight.max(1.0);

        (final_decision, confidence, feature_importance)
    }

    async fn majority_voting(
        &self,
        decisions: &[(String, ModelDecision)],
    ) -> (RoutingDecision, f64, HashMap<String, f64>) {
        let mut vote_counts: HashMap<RoutingDecision, usize> = HashMap::new();
        let mut feature_importance = HashMap::new();

        for (_, decision) in decisions {
            *vote_counts.entry(decision.routing.clone()).or_insert(0) += 1;
            
            for (feature, importance) in &decision.features_importance {
                *feature_importance.entry(feature.clone()).or_insert(0.0) += importance;
            }
        }

        let total_votes = decisions.len() as f64;
        for importance in feature_importance.values_mut() {
            *importance /= total_votes;
        }

        let (final_decision, votes) = vote_counts.into_iter()
            .max_by_key(|&(_, count)| count)
            .unwrap_or((RoutingDecision::Skip, 0));

        let confidence = votes as f64 / total_votes;

        (final_decision, confidence, feature_importance)
    }

    async fn confidence_weighted_voting(
        &self,
        decisions: &[(String, ModelDecision)],
    ) -> (RoutingDecision, f64, HashMap<String, f64>) {
        let mut routing_scores: HashMap<RoutingDecision, f64> = HashMap::new();
        let mut total_confidence = 0.0;
        let mut feature_importance = HashMap::new();

        for (_, decision) in decisions {
            *routing_scores.entry(decision.routing.clone()).or_insert(0.0) += decision.confidence;
            total_confidence += decision.confidence;

            for (feature, importance) in &decision.features_importance {
                *feature_importance.entry(feature.clone()).or_insert(0.0) += importance * decision.confidence;
            }
        }

        for importance in feature_importance.values_mut() {
            *importance /= total_confidence.max(1.0);
        }

        let (final_decision, score) = routing_scores.into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((RoutingDecision::Skip, 0.0));

        let confidence = score / total_confidence.max(1.0);

        (final_decision, confidence, feature_importance)
    }

    async fn adaptive_voting(
        &self,
        decisions: &[(String, ModelDecision)],
        analysis: &TransactionAnalysis,
    ) -> (RoutingDecision, f64, HashMap<String, f64>) {
        let history = self.performance_history.read().await;
        let mut routing_scores: HashMap<RoutingDecision, f64> = HashMap::new();
        let mut total_weight = 0.0;
        let mut feature_importance = HashMap::new();

        // Determine transaction type for specialization matching
        let tx_type = self.determine_transaction_type(analysis);

        for (model_name, decision) in decisions {
            let model_config = self.config.models.iter()
                .find(|m| &m.name == model_name)
                .unwrap();

            // Adaptive weight based on specialization and recent performance
            let mut weight = model_config.weight;

            // Boost weight if model specializes in this transaction type
            if model_config.specialization.contains(&tx_type) {
                weight *= 1.5;
            }

            // Adjust based on recent performance
            if let Some(perf) = history.model_accuracy.get(model_name) {
                let recent_accuracy = self.calculate_recent_accuracy(&perf.recent_performance);
                weight *= recent_accuracy;
            }

            *routing_scores.entry(decision.routing.clone()).or_insert(0.0) += weight * decision.confidence;
            total_weight += weight;

            for (feature, importance) in &decision.features_importance {
                *feature_importance.entry(feature.clone()).or_insert(0.0) += importance * weight;
            }
        }

        for importance in feature_importance.values_mut() {
            *importance /= total_weight.max(1.0);
        }

        let (final_decision, score) = routing_scores.into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((RoutingDecision::Skip, 0.0));

        let confidence = score / total_weight.max(1.0);

        (final_decision, confidence, feature_importance)
    }

    fn determine_transaction_type(&self, analysis: &TransactionAnalysis) -> String {
        if analysis.features.is_contract_creation {
            "complex_contracts".to_string()
        } else if analysis.features.is_token_transfer {
            "token_transfers".to_string()
        } else if analysis.features.has_complex_data {
            "complex_contracts".to_string()
        } else {
            "simple_transfers".to_string()
        }
    }

    fn calculate_recent_accuracy(&self, recent_performance: &VecDeque<(Instant, bool)>) -> f64 {
        if recent_performance.is_empty() {
            return 1.0;
        }

        let now = Instant::now();
        let mut weighted_sum = 0.0;
        let mut weight_total = 0.0;

        for (timestamp, success) in recent_performance {
            let age = now.duration_since(*timestamp).as_secs() as f64;
            let weight = (-age / 3600.0).exp(); // Exponential decay over hours
            
            weighted_sum += if *success { weight } else { 0.0 };
            weight_total += weight;
        }

        if weight_total > 0.0 {
            weighted_sum / weight_total
        } else {
            1.0
        }
    }

    pub async fn update_model_performance(
        &self,
        model_name: &str,
        predicted: &PredictedMetrics,
        actual: &ActualMetrics,
        routing_decision: &RoutingDecision,
    ) {
        let mut history = self.performance_history.write().await;
        
        let performance = history.model_accuracy.entry(model_name.to_string())
            .or_insert_with(|| ModelPerformance {
                correct_predictions: 0,
                total_predictions: 0,
                average_confidence: 0.0,
                specialization_accuracy: HashMap::new(),
                recent_performance: VecDeque::with_capacity(100),
            });

        performance.total_predictions += 1;
        
        // Determine if prediction was correct based on execution success and time
        let prediction_correct = actual.success && 
            (actual.execution_time_ms - predicted.execution_time_ms).abs() / predicted.execution_time_ms < 0.2;
        
        if prediction_correct {
            performance.correct_predictions += 1;
        }

        performance.recent_performance.push_back((Instant::now(), prediction_correct));
        if performance.recent_performance.len() > 100 {
            performance.recent_performance.pop_front();
        }

        // Update average confidence
        performance.average_confidence = (performance.average_confidence * (performance.total_predictions - 1) as f64 
            + predicted.success_probability) / performance.total_predictions as f64;

        // Trigger adaptation if performance drops below threshold
        let accuracy = performance.correct_predictions as f64 / performance.total_predictions as f64;
        let model_config = self.config.models.iter()
            .find(|m| m.name == model_name);

        if let Some(config) = model_config {
            if accuracy < config.performance_threshold && performance.total_predictions > 50 {
                self.trigger_adaptation(model_name, accuracy).await;
            }
        }
    }

    async fn trigger_adaptation(&self, model_name: &str, current_accuracy: f64) {
        // In a real implementation, this would retrain or adjust the model
        // For now, we'll log the adaptation event
        let mut history = self.performance_history.write().await;
        
        history.adaptation_history.push_back(AdaptationEvent {
            timestamp: Instant::now(),
            model_name: model_name.to_string(),
            old_weight: 0.0, // Would fetch actual weight
            new_weight: 0.0, // Would calculate new weight
            reason: format!("Performance dropped to {:.2}%", current_accuracy * 100.0),
        });

        if history.adaptation_history.len() > 100 {
            history.adaptation_history.pop_front();
        }
    }
}

#[async_trait]
impl AIAgent for EnhancedAIDecisionEngine {
    async fn make_routing_decision(&self, tx: &TransactionSigned) -> Result<RoutingDecision> {
        // Check cache first
        let tx_hash = tx.hash();
        
        {
            let mut cache = self.decision_cache.write().await;
            if let Some(cached) = cache.get(&tx_hash) {
                if cached.timestamp.elapsed() < Duration::from_secs(self.config.cache_ttl_seconds) {
                    return Ok(cached.decision.clone());
                }
            }
        }

        // Analyze transaction
        let analysis = self.analyze_transaction(tx).await?;
        
        // Get agent contexts if available
        let from_address = Address::default(); // Would extract from tx
        let contexts = self.get_agent_contexts(from_address).await;

        // Get ensemble decision
        let (decision, confidence, _) = self.ensemble_decision(tx, &analysis, &contexts).await?;

        // Cache decision if confidence is high
        if confidence > self.config.confidence_threshold {
            let mut cache = self.decision_cache.write().await;
            cache.put(tx_hash, CachedDecision {
                decision: decision.clone(),
                confidence,
                timestamp: Instant::now(),
                context_hash: B256::default(), // Would calculate actual hash
            });
        }

        Ok(decision)
    }

    async fn analyze_transaction(&self, tx: &TransactionSigned) -> Result<TransactionAnalysis> {
        // Use the base implementation and enhance it
        let base_engine = super::ai_agent::UnifiedAIDecisionEngine::new()?;
        let mut analysis = base_engine.analyze_transaction(tx).await?;

        // Enhance with ensemble insights
        let from_address = Address::default(); // Would extract from tx
        let contexts = self.get_agent_contexts(from_address).await;
        
        let (routing, confidence, features_importance) = self.ensemble_decision(tx, &analysis, &contexts).await?;
        
        analysis.routing_suggestion = routing;
        analysis.confidence = confidence;

        Ok(analysis)
    }

    async fn update_learning(&self, actual: ActualMetrics, predicted: PredictedMetrics) -> Result<()> {
        // This would be called after transaction execution to update model performance
        // In practice, you'd need to track which model made which prediction
        
        // For now, update all models equally (in practice, track per-model predictions)
        for model in &self.models {
            self.update_model_performance(
                &model.get_name(),
                &predicted,
                &actual,
                &RoutingDecision::ProcessWithRAG, // Would track actual decision
            ).await;
        }

        Ok(())
    }
}

// Model trait that all models must implement
#[async_trait]
trait AIModel: Send + Sync {
    async fn make_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<ModelDecision>;
    
    fn get_name(&self) -> String;
}

// Example model implementations
struct NeuralNetworkModel {
    config: ModelConfig,
}

impl NeuralNetworkModel {
    fn new(config: ModelConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AIModel for NeuralNetworkModel {
    async fn make_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<ModelDecision> {
        // Simplified neural network logic
        let mut features_importance = HashMap::new();
        features_importance.insert("complexity".to_string(), 0.8);
        features_importance.insert("gas_price".to_string(), 0.6);
        features_importance.insert("contract_creation".to_string(), 0.9);

        let routing = if analysis.complexity_score > 0.7 {
            RoutingDecision::ProcessWithBoth
        } else if analysis.features.is_contract_creation {
            RoutingDecision::ProcessWithRAG
        } else {
            RoutingDecision::ProcessWithMemory
        };

        Ok(ModelDecision {
            routing,
            confidence: 0.85,
            reasoning: "Neural network analysis based on complexity patterns".to_string(),
            features_importance,
        })
    }

    fn get_name(&self) -> String {
        self.config.name.clone()
    }
}

struct DecisionTreeModel {
    config: ModelConfig,
}

impl DecisionTreeModel {
    fn new(config: ModelConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AIModel for DecisionTreeModel {
    async fn make_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<ModelDecision> {
        let mut features_importance = HashMap::new();
        features_importance.insert("is_token_transfer".to_string(), 0.9);
        features_importance.insert("value_category".to_string(), 0.7);

        let routing = if analysis.features.is_token_transfer {
            RoutingDecision::ProcessWithMemory
        } else if analysis.features.has_complex_data {
            RoutingDecision::ProcessWithRAG
        } else {
            RoutingDecision::ProcessWithSVM
        };

        Ok(ModelDecision {
            routing,
            confidence: 0.9,
            reasoning: "Decision tree based on transaction type".to_string(),
            features_importance,
        })
    }

    fn get_name(&self) -> String {
        self.config.name.clone()
    }
}

struct RuleBasedModel {
    config: ModelConfig,
}

impl RuleBasedModel {
    fn new(config: ModelConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AIModel for RuleBasedModel {
    async fn make_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<ModelDecision> {
        let mut features_importance = HashMap::new();
        features_importance.insert("safety_score".to_string(), 1.0);
        features_importance.insert("gas_estimate".to_string(), 0.5);

        let routing = if analysis.safety_score < 0.5 {
            RoutingDecision::Skip
        } else if tx.value() > U256::from(10_000_000_000_000_000_000u128) {
            RoutingDecision::ProcessWithMemory
        } else {
            RoutingDecision::ProcessWithSVM
        };

        Ok(ModelDecision {
            routing,
            confidence: 0.95,
            reasoning: "Rule-based decision using safety thresholds".to_string(),
            features_importance,
        })
    }

    fn get_name(&self) -> String {
        self.config.name.clone()
    }
}

struct StatisticalModel {
    config: ModelConfig,
}

impl StatisticalModel {
    fn new(config: ModelConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AIModel for StatisticalModel {
    async fn make_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<ModelDecision> {
        let mut features_importance = HashMap::new();
        
        // Statistical analysis based on historical patterns
        let pattern_score = contexts.len() as f64 / 100.0;
        features_importance.insert("historical_pattern".to_string(), pattern_score);
        features_importance.insert("gas_price_percentile".to_string(), 0.6);

        let routing = if pattern_score > 0.5 {
            RoutingDecision::ProcessWithBoth
        } else {
            RoutingDecision::ProcessWithRAG
        };

        Ok(ModelDecision {
            routing,
            confidence: 0.75 + pattern_score * 0.2,
            reasoning: "Statistical model based on historical patterns".to_string(),
            features_importance,
        })
    }

    fn get_name(&self) -> String {
        self.config.name.clone()
    }
}

struct HybridModel {
    config: ModelConfig,
}

impl HybridModel {
    fn new(config: ModelConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl AIModel for HybridModel {
    async fn make_decision(
        &self,
        tx: &TransactionSigned,
        analysis: &TransactionAnalysis,
        contexts: &[PreprocessedContext],
    ) -> Result<ModelDecision> {
        // Combine multiple approaches
        let mut features_importance = HashMap::new();
        features_importance.insert("hybrid_score".to_string(), 0.85);
        features_importance.insert("context_relevance".to_string(), 0.7);

        let context_score = contexts.iter()
            .map(|c| c.priority_score)
            .sum::<f64>() / contexts.len().max(1) as f64;

        let routing = if analysis.complexity_score > 0.8 && context_score > 0.7 {
            RoutingDecision::ProcessWithBoth
        } else if analysis.features.is_contract_creation || context_score > 0.8 {
            RoutingDecision::ProcessWithRAG
        } else {
            RoutingDecision::ProcessWithMemory
        };

        Ok(ModelDecision {
            routing,
            confidence: 0.8,
            reasoning: "Hybrid model combining neural, statistical, and rule-based approaches".to_string(),
            features_importance,
        })
    }

    fn get_name(&self) -> String {
        self.config.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_primitives::{TxEip1559, TxKind};

    #[tokio::test]
    async fn test_enhanced_ai_decision_engine() {
        let config = EnsembleConfig::default();
        let engine = EnhancedAIDecisionEngine::new(config).unwrap();

        // Create a test transaction
        let tx = TransactionSigned::from_transaction_and_signature(
            reth_primitives::Transaction::Eip1559(TxEip1559 {
                chain_id: 1,
                nonce: 0,
                gas_limit: 21000,
                max_fee_per_gas: 20_000_000_000,
                max_priority_fee_per_gas: 1_000_000_000,
                to: TxKind::Call(Address::default()),
                value: U256::from(1_000_000_000_000_000_000u64),
                input: Default::default(),
                access_list: Default::default(),
            }),
            reth_primitives::Signature::default(),
        );

        let decision = engine.make_routing_decision(&tx).await.unwrap();
        assert!(!matches!(decision, RoutingDecision::Skip));
    }

    #[tokio::test]
    async fn test_ensemble_voting() {
        let config = EnsembleConfig::default();
        let engine = EnhancedAIDecisionEngine::new(config).unwrap();

        let decisions = vec![
            ("model1".to_string(), ModelDecision {
                routing: RoutingDecision::ProcessWithRAG,
                confidence: 0.9,
                reasoning: "test".to_string(),
                features_importance: HashMap::new(),
            }),
            ("model2".to_string(), ModelDecision {
                routing: RoutingDecision::ProcessWithRAG,
                confidence: 0.8,
                reasoning: "test".to_string(),
                features_importance: HashMap::new(),
            }),
            ("model3".to_string(), ModelDecision {
                routing: RoutingDecision::ProcessWithMemory,
                confidence: 0.7,
                reasoning: "test".to_string(),
                features_importance: HashMap::new(),
            }),
        ];

        let (decision, confidence, _) = engine.confidence_weighted_voting(&decisions).await;
        assert!(matches!(decision, RoutingDecision::ProcessWithRAG));
        assert!(confidence > 0.5);
    }
}