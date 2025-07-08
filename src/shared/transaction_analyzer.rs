use eyre::Result;
use reth_primitives::TransactionSigned;
use std::time::Instant;

#[derive(Debug)]
pub struct TransactionAnalyzer {
    complexity_calculator: ComplexityCalculator,
    safety_analyzer: SafetyAnalyzer,
    gas_estimator: GasEstimator,
}

#[derive(Debug, Clone)]
pub struct TransactionAnalysis {
    pub complexity_score: f64,
    pub safety_score: f64,
    pub gas_estimate: GasEstimate,
    pub semantic_type: SemanticType,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct GasEstimate {
    pub base_gas: u64,
    pub priority_fee: u64,
    pub max_fee: u64,
    pub estimated_actual: u64,
}

#[derive(Debug, Clone)]
pub enum SemanticType {
    SimpleTransfer,
    TokenTransfer,
    ContractCreation,
    ContractInteraction,
    MultiSigOperation,
    DeFiOperation,
    NFTOperation,
    Unknown,
}

#[derive(Debug)]
pub struct ComplexityCalculator;

#[derive(Debug)]
pub struct SafetyAnalyzer;

#[derive(Debug)]
pub struct GasEstimator;

impl TransactionAnalyzer {
    pub fn new() -> Self {
        Self {
            complexity_calculator: ComplexityCalculator,
            safety_analyzer: SafetyAnalyzer,
            gas_estimator: GasEstimator,
        }
    }
    
    pub async fn analyze(&self, tx: &TransactionSigned) -> Result<TransactionAnalysis> {
        let (complexity, safety, gas_estimate) = tokio::join!(
            self.complexity_calculator.calculate(tx),
            self.safety_analyzer.analyze(tx),
            self.gas_estimator.estimate(tx)
        );
        
        let semantic_type = self.determine_semantic_type(tx);
        
        Ok(TransactionAnalysis {
            complexity_score: complexity?,
            safety_score: safety?,
            gas_estimate: gas_estimate?,
            semantic_type,
            timestamp: Instant::now(),
        })
    }
    
    fn determine_semantic_type(&self, tx: &TransactionSigned) -> SemanticType {
        if tx.to().is_none() {
            return SemanticType::ContractCreation;
        }
        
        if tx.input().is_empty() {
            return SemanticType::SimpleTransfer;
        }
        
        if tx.input().len() >= 4 {
            let selector = &tx.input()[0..4];
            match selector {
                [0xa9, 0x05, 0x9c, 0xbb] => SemanticType::TokenTransfer,
                [0x23, 0xb8, 0x72, 0xdd] => SemanticType::TokenTransfer,
                [0x09, 0x5e, 0xa7, 0xb3] => SemanticType::DeFiOperation,
                [0x12, 0xaa, 0x3c, 0xaf] => SemanticType::DeFiOperation,
                _ => SemanticType::ContractInteraction,
            }
        } else {
            SemanticType::Unknown
        }
    }
}

impl ComplexityCalculator {
    pub async fn calculate(&self, tx: &TransactionSigned) -> Result<f64> {
        let mut score = 0.0;
        
        score += (tx.input().len() as f64) / 1000.0;
        
        if tx.to().is_none() {
            score += 0.3;
        }
        
        score += (tx.gas_limit() as f64) / 10_000_000.0;
        
        if tx.input().len() > 100 {
            score += 0.2;
        }
        
        if tx.access_list().is_some() {
            score += 0.1;
        }
        
        Ok(score.min(1.0))
    }
}

impl SafetyAnalyzer {
    pub async fn analyze(&self, tx: &TransactionSigned) -> Result<f64> {
        let mut score = 1.0;
        
        if tx.to().is_none() {
            score -= 0.2;
        }
        
        if tx.gas_limit() > 5_000_000 {
            score -= 0.1;
        }
        
        if let Some(max_fee) = tx.max_fee_per_gas() {
            if max_fee > 1000_000_000_000 {
                score -= 0.15;
            }
        }
        
        if tx.input().len() > 10000 {
            score -= 0.1;
        }
        
        Ok(score.max(0.0))
    }
}

impl GasEstimator {
    pub async fn estimate(&self, tx: &TransactionSigned) -> Result<GasEstimate> {
        let base_gas = 21000u64;
        
        let data_gas = tx.input().iter().map(|&byte| {
            if byte == 0 { 4 } else { 16 }
        }).sum::<u64>();
        
        let total_base = base_gas + data_gas;
        
        let estimated_actual = if tx.to().is_none() {
            total_base + 32000
        } else {
            total_base + (tx.input().len() as u64 * 10)
        };
        
        Ok(GasEstimate {
            base_gas: total_base,
            priority_fee: tx.max_priority_fee_per_gas().unwrap_or(1_000_000_000),
            max_fee: tx.max_fee_per_gas().unwrap_or(50_000_000_000),
            estimated_actual: estimated_actual.min(tx.gas_limit()),
        })
    }
}