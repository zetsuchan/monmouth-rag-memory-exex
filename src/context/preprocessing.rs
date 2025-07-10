use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::shared::communication::{CrossExExMessage, TransactionAnalysis};
use crate::shared::types::{AgentAction, AgentContext};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreprocessedContext {
    pub transaction_hash: B256,
    pub agent_address: Address,
    pub action_type: AgentAction,
    pub extracted_features: HashMap<String, f64>,
    pub semantic_tags: Vec<String>,
    pub priority_score: f64,
    pub timestamp: u64,
    pub compressed_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingConfig {
    pub enable_compression: bool,
    pub compression_threshold: usize,
    pub feature_extraction_depth: u8,
    pub semantic_analysis: bool,
    pub priority_weights: HashMap<String, f64>,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        let mut priority_weights = HashMap::new();
        priority_weights.insert("swap".to_string(), 1.5);
        priority_weights.insert("liquidity".to_string(), 1.3);
        priority_weights.insert("stake".to_string(), 1.2);
        priority_weights.insert("transfer".to_string(), 1.0);

        Self {
            enable_compression: true,
            compression_threshold: 1024,
            feature_extraction_depth: 3,
            semantic_analysis: true,
            priority_weights,
        }
    }
}

pub struct ContextPreprocessor {
    config: ProcessingConfig,
    feature_extractors: Arc<RwLock<HashMap<String, Box<dyn FeatureExtractor>>>>,
}

impl ContextPreprocessor {
    pub fn new(config: ProcessingConfig) -> Self {
        Self {
            config,
            feature_extractors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn preprocess_transaction(
        &self,
        analysis: &TransactionAnalysis,
        agent_context: &AgentContext,
    ) -> Result<PreprocessedContext, PreprocessingError> {
        let mut features = HashMap::new();
        let mut semantic_tags = Vec::new();

        // Extract basic features
        features.insert("gas_used".to_string(), analysis.gas_used as f64);
        features.insert("value".to_string(), analysis.value.to::<f64>());
        
        // Extract action-specific features
        match &agent_context.last_action {
            AgentAction::Swap { token_in, token_out, amount_in, .. } => {
                features.insert("swap_amount".to_string(), amount_in.to::<f64>());
                semantic_tags.push("swap".to_string());
                semantic_tags.push(format!("token_pair_{}_{}", token_in, token_out));
            }
            AgentAction::AddLiquidity { token_a, token_b, amount_a, amount_b, .. } => {
                features.insert("liquidity_a".to_string(), amount_a.to::<f64>());
                features.insert("liquidity_b".to_string(), amount_b.to::<f64>());
                semantic_tags.push("liquidity_add".to_string());
                semantic_tags.push(format!("pool_{}_{}", token_a, token_b));
            }
            AgentAction::RemoveLiquidity { .. } => {
                semantic_tags.push("liquidity_remove".to_string());
            }
            AgentAction::Stake { amount, .. } => {
                features.insert("stake_amount".to_string(), amount.to::<f64>());
                semantic_tags.push("staking".to_string());
            }
            AgentAction::Unstake { .. } => {
                semantic_tags.push("unstaking".to_string());
            }
            AgentAction::Transfer { to, amount } => {
                features.insert("transfer_amount".to_string(), amount.to::<f64>());
                semantic_tags.push("transfer".to_string());
                semantic_tags.push(format!("to_{}", to));
            }
        }

        // Apply custom feature extractors
        let extractors = self.feature_extractors.read().await;
        for (name, extractor) in extractors.iter() {
            if let Ok(extracted) = extractor.extract(analysis, agent_context).await {
                features.extend(extracted);
            }
        }

        // Calculate priority score
        let priority_score = self.calculate_priority(&agent_context.last_action, &features);

        // Compress if needed
        let compressed_data = if self.config.enable_compression {
            self.compress_context(analysis, &features)?
        } else {
            None
        };

        Ok(PreprocessedContext {
            transaction_hash: analysis.transaction_hash,
            agent_address: agent_context.agent_address,
            action_type: agent_context.last_action.clone(),
            extracted_features: features,
            semantic_tags,
            priority_score,
            timestamp: analysis.timestamp,
            compressed_data,
        })
    }

    pub async fn preprocess_batch(
        &self,
        analyses: Vec<TransactionAnalysis>,
        contexts: Vec<AgentContext>,
    ) -> Result<Vec<PreprocessedContext>, PreprocessingError> {
        if analyses.len() != contexts.len() {
            return Err(PreprocessingError::BatchSizeMismatch);
        }

        let mut results = Vec::with_capacity(analyses.len());
        
        // Process in parallel for efficiency
        let tasks: Vec<_> = analyses
            .iter()
            .zip(contexts.iter())
            .map(|(analysis, context)| async {
                self.preprocess_transaction(analysis, context).await
            })
            .collect();

        let processed = futures::future::join_all(tasks).await;
        
        for result in processed {
            results.push(result?);
        }

        Ok(results)
    }

    pub async fn register_feature_extractor(
        &self,
        name: String,
        extractor: Box<dyn FeatureExtractor>,
    ) {
        let mut extractors = self.feature_extractors.write().await;
        extractors.insert(name, extractor);
    }

    fn calculate_priority(
        &self,
        action: &AgentAction,
        features: &HashMap<String, f64>,
    ) -> f64 {
        let base_priority = match action {
            AgentAction::Swap { .. } => self.config.priority_weights.get("swap").unwrap_or(&1.0),
            AgentAction::AddLiquidity { .. } | AgentAction::RemoveLiquidity { .. } => {
                self.config.priority_weights.get("liquidity").unwrap_or(&1.0)
            }
            AgentAction::Stake { .. } | AgentAction::Unstake { .. } => {
                self.config.priority_weights.get("stake").unwrap_or(&1.0)
            }
            AgentAction::Transfer { .. } => {
                self.config.priority_weights.get("transfer").unwrap_or(&1.0)
            }
        };

        // Adjust based on value/amount
        let value_multiplier = features
            .values()
            .filter(|v| **v > 0.0)
            .map(|v| (v.log10() + 1.0) / 10.0)
            .fold(1.0, |acc, v| acc * (1.0 + v));

        base_priority * value_multiplier
    }

    fn compress_context(
        &self,
        analysis: &TransactionAnalysis,
        features: &HashMap<String, f64>,
    ) -> Result<Option<Vec<u8>>, PreprocessingError> {
        let data_size = bincode::serialize(analysis)
            .map(|v| v.len())
            .unwrap_or(0);

        if data_size < self.config.compression_threshold {
            return Ok(None);
        }

        // Use zstd compression for efficiency
        let serialized = bincode::serialize(&(analysis, features))
            .map_err(|e| PreprocessingError::CompressionError(e.to_string()))?;

        let compressed = zstd::bulk::compress(&serialized, 3)
            .map_err(|e| PreprocessingError::CompressionError(e.to_string()))?;

        Ok(Some(compressed))
    }

    pub fn decompress_context(
        &self,
        compressed: &[u8],
    ) -> Result<(TransactionAnalysis, HashMap<String, f64>), PreprocessingError> {
        let decompressed = zstd::bulk::decompress(compressed, 10_000_000)
            .map_err(|e| PreprocessingError::CompressionError(e.to_string()))?;

        bincode::deserialize(&decompressed)
            .map_err(|e| PreprocessingError::DeserializationError(e.to_string()))
    }
}

#[async_trait::async_trait]
pub trait FeatureExtractor: Send + Sync {
    async fn extract(
        &self,
        analysis: &TransactionAnalysis,
        context: &AgentContext,
    ) -> Result<HashMap<String, f64>, PreprocessingError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PreprocessingError {
    #[error("Batch size mismatch")]
    BatchSizeMismatch,
    #[error("Compression error: {0}")]
    CompressionError(String),
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    #[error("Feature extraction error: {0}")]
    FeatureExtractionError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::U256;

    #[tokio::test]
    async fn test_context_preprocessing() {
        let config = ProcessingConfig::default();
        let preprocessor = ContextPreprocessor::new(config);

        let analysis = TransactionAnalysis {
            transaction_hash: B256::default(),
            timestamp: 1234567890,
            from: Address::default(),
            to: Address::default(),
            value: U256::from(1000),
            gas_used: 21000,
            routing_decision: crate::shared::communication::RoutingDecision::RAG,
            context: vec![],
        };

        let agent_context = AgentContext {
            agent_address: Address::default(),
            last_action: AgentAction::Transfer {
                to: Address::default(),
                amount: U256::from(1000),
            },
            reputation_score: 100,
            total_interactions: 10,
            success_rate: 0.9,
            specialization: vec!["transfer".to_string()],
        };

        let result = preprocessor
            .preprocess_transaction(&analysis, &agent_context)
            .await
            .unwrap();

        assert_eq!(result.transaction_hash, analysis.transaction_hash);
        assert!(result.semantic_tags.contains(&"transfer".to_string()));
        assert!(result.extracted_features.contains_key("transfer_amount"));
    }
}