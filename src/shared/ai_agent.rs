use eyre::Result;
use reth_primitives::TransactionSigned;
use serde::{Serialize, Deserialize};
use async_trait::async_trait;
use std::time::Instant;

#[async_trait]
pub trait AIAgent: Send + Sync {
    async fn make_routing_decision(&self, tx: &TransactionSigned) -> Result<RoutingDecision>;
    async fn analyze_transaction(&self, tx: &TransactionSigned) -> Result<TransactionAnalysis>;
    async fn update_learning(&self, actual: ActualMetrics, predicted: PredictedMetrics) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingDecision {
    ProcessWithRAG,
    ProcessWithMemory,
    ProcessWithBoth,
    ProcessWithSVM,
    Skip,
}

impl RoutingDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProcessWithRAG => "rag",
            Self::ProcessWithMemory => "memory",
            Self::ProcessWithBoth => "both",
            Self::ProcessWithSVM => "svm",
            Self::Skip => "skip",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionAnalysis {
    pub complexity_score: f64,
    pub safety_score: f64,
    pub gas_estimate: u64,
    pub routing_suggestion: RoutingDecision,
    pub confidence: f64,
    pub features: TransactionFeatures,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionFeatures {
    pub is_contract_creation: bool,
    pub is_token_transfer: bool,
    pub has_complex_data: bool,
    pub value_category: ValueCategory,
    pub gas_price_category: GasPriceCategory,
    pub nonce_sequence: NonceSequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValueCategory {
    Zero,
    Low,
    Medium,
    High,
    Whale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GasPriceCategory {
    Low,
    Standard,
    Priority,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NonceSequence {
    Sequential,
    Gap,
    Duplicate,
    First,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictedMetrics {
    pub execution_time_ms: f64,
    pub memory_usage_bytes: u64,
    pub success_probability: f64,
    pub complexity_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActualMetrics {
    pub execution_time_ms: f64,
    pub memory_usage_bytes: u64,
    pub success: bool,
    pub error_type: Option<String>,
}

#[derive(Debug)]
pub struct UnifiedAIDecisionEngine {
    complexity_threshold: f64,
    safety_threshold: f64,
    learning_rate: f64,
}

impl UnifiedAIDecisionEngine {
    pub fn new() -> Result<Self> {
        Ok(Self {
            complexity_threshold: 0.7,
            safety_threshold: 0.8,
            learning_rate: 0.1,
        })
    }
}

#[async_trait]
impl AIAgent for UnifiedAIDecisionEngine {
    async fn make_routing_decision(&self, tx: &TransactionSigned) -> Result<RoutingDecision> {
        let analysis = self.analyze_transaction(tx).await?;
        
        if analysis.safety_score < self.safety_threshold {
            return Ok(RoutingDecision::Skip);
        }
        
        if analysis.features.is_contract_creation || analysis.features.has_complex_data {
            if analysis.complexity_score > self.complexity_threshold {
                return Ok(RoutingDecision::ProcessWithBoth);
            }
            return Ok(RoutingDecision::ProcessWithRAG);
        }
        
        if matches!(analysis.features.value_category, ValueCategory::High | ValueCategory::Whale) {
            return Ok(RoutingDecision::ProcessWithMemory);
        }
        
        Ok(analysis.routing_suggestion)
    }
    
    async fn analyze_transaction(&self, tx: &TransactionSigned) -> Result<TransactionAnalysis> {
        let features = self.extract_features(tx);
        let complexity_score = self.calculate_complexity(&features);
        let safety_score = self.calculate_safety(&features);
        
        let routing_suggestion = if features.has_complex_data {
            RoutingDecision::ProcessWithRAG
        } else if features.is_token_transfer {
            RoutingDecision::ProcessWithMemory
        } else {
            RoutingDecision::ProcessWithSVM
        };
        
        Ok(TransactionAnalysis {
            complexity_score,
            safety_score,
            gas_estimate: tx.gas_limit(),
            routing_suggestion,
            confidence: 0.85,
            features,
            timestamp: Instant::now(),
        })
    }
    
    async fn update_learning(&self, actual: ActualMetrics, predicted: PredictedMetrics) -> Result<()> {
        let error = (actual.execution_time_ms - predicted.execution_time_ms).abs();
        let adjustment = error * self.learning_rate;
        
        Ok(())
    }
}

impl UnifiedAIDecisionEngine {
    fn extract_features(&self, tx: &TransactionSigned) -> TransactionFeatures {
        TransactionFeatures {
            is_contract_creation: tx.to().is_none(),
            is_token_transfer: self.is_token_transfer(tx),
            has_complex_data: tx.input().len() > 100,
            value_category: self.categorize_value(tx.value()),
            gas_price_category: self.categorize_gas_price(tx.max_priority_fee_per_gas()),
            nonce_sequence: NonceSequence::Sequential,
        }
    }
    
    fn is_token_transfer(&self, tx: &TransactionSigned) -> bool {
        tx.input().len() >= 4 && &tx.input()[0..4] == &[0xa9, 0x05, 0x9c, 0xbb]
    }
    
    fn categorize_value(&self, value: reth_primitives::U256) -> ValueCategory {
        if value == reth_primitives::U256::ZERO {
            ValueCategory::Zero
        } else if value < reth_primitives::U256::from(1_000_000_000_000_000_000u64) {
            ValueCategory::Low
        } else if value < reth_primitives::U256::from(10_000_000_000_000_000_000u128) {
            ValueCategory::Medium
        } else if value < reth_primitives::U256::from(100_000_000_000_000_000_000u128) {
            ValueCategory::High
        } else {
            ValueCategory::Whale
        }
    }
    
    fn categorize_gas_price(&self, gas_price: Option<u128>) -> GasPriceCategory {
        match gas_price {
            None => GasPriceCategory::Standard,
            Some(price) if price < 10_000_000_000 => GasPriceCategory::Low,
            Some(price) if price < 50_000_000_000 => GasPriceCategory::Standard,
            Some(price) if price < 200_000_000_000 => GasPriceCategory::Priority,
            _ => GasPriceCategory::Max,
        }
    }
    
    fn calculate_complexity(&self, features: &TransactionFeatures) -> f64 {
        let mut score = 0.0;
        
        if features.is_contract_creation { score += 0.3; }
        if features.has_complex_data { score += 0.3; }
        if features.is_token_transfer { score += 0.1; }
        
        match features.value_category {
            ValueCategory::Whale => score += 0.2,
            ValueCategory::High => score += 0.1,
            _ => {}
        }
        
        score.min(1.0)
    }
    
    fn calculate_safety(&self, features: &TransactionFeatures) -> f64 {
        let mut score = 1.0;
        
        if features.is_contract_creation { score -= 0.2; }
        if matches!(features.gas_price_category, GasPriceCategory::Max) { score -= 0.1; }
        if matches!(features.nonce_sequence, NonceSequence::Gap | NonceSequence::Duplicate) { 
            score -= 0.3; 
        }
        
        score.max(0.0)
    }
}