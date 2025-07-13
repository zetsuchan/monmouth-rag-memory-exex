use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::rpc::types::eth::Transaction;
use async_trait::async_trait;
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::enhanced_intent_parser::{Intent, IntentType};
use super::cow_matcher::{ExecutionPlan, ExecutionStep, StepType};

/// Intent to blockchain transaction converter
pub struct IntentConverter {
    /// Protocol adapters for different DeFi protocols
    protocol_adapters: HashMap<String, Box<dyn ProtocolAdapter>>,
    /// Transaction builder
    tx_builder: TransactionBuilder,
    /// Gas estimator
    gas_estimator: GasEstimator,
    /// Safety checker
    safety_checker: SafetyChecker,
}

/// Trait for protocol-specific adapters
#[async_trait]
pub trait ProtocolAdapter: Send + Sync {
    /// Get protocol name
    fn name(&self) -> &str;
    
    /// Convert intent to protocol-specific calldata
    async fn build_calldata(
        &self,
        intent: &Intent,
        context: &ConversionContext,
    ) -> Result<Bytes>;
    
    /// Get contract address for the protocol
    fn get_contract_address(&self, chain_id: u64) -> Option<Address>;
    
    /// Estimate gas for the operation
    async fn estimate_gas(
        &self,
        intent: &Intent,
        context: &ConversionContext,
    ) -> Result<U256>;
    
    /// Validate intent compatibility with protocol
    fn validate_intent(&self, intent: &Intent) -> Result<()>;
}

/// Context for transaction conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionContext {
    /// Chain ID for the transaction
    pub chain_id: u64,
    /// Sender address
    pub sender: Address,
    /// Current nonce
    pub nonce: u64,
    /// Gas price parameters
    pub gas_params: GasParameters,
    /// Slippage settings
    pub slippage: SlippageSettings,
    /// Additional protocol-specific parameters
    pub protocol_params: HashMap<String, serde_json::Value>,
}

/// Gas parameters for transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasParameters {
    /// Max fee per gas (EIP-1559)
    pub max_fee_per_gas: U256,
    /// Max priority fee per gas (EIP-1559)
    pub max_priority_fee_per_gas: U256,
    /// Gas limit
    pub gas_limit: U256,
}

/// Slippage settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlippageSettings {
    /// Maximum allowed slippage (percentage)
    pub max_slippage: f32,
    /// Minimum output amount
    pub min_output: Option<U256>,
    /// Deadline timestamp
    pub deadline: u64,
}

/// Transaction builder for constructing blockchain transactions
pub struct TransactionBuilder {
    /// EIP-1559 support
    eip1559_enabled: bool,
    /// Default gas buffer percentage
    gas_buffer: f32,
}

/// Gas estimation service
pub struct GasEstimator {
    /// Historical gas data
    historical_data: HashMap<String, GasHistory>,
    /// Simulation service
    simulator: Box<dyn TransactionSimulator>,
}

/// Historical gas usage data
#[derive(Debug, Clone)]
pub struct GasHistory {
    pub operation_type: String,
    pub average_gas: U256,
    pub min_gas: U256,
    pub max_gas: U256,
    pub sample_count: u32,
}

/// Trait for transaction simulation
#[async_trait]
pub trait TransactionSimulator: Send + Sync {
    async fn simulate_transaction(
        &self,
        tx: &Transaction,
        block: Option<u64>,
    ) -> Result<SimulationResult>;
}

/// Result of transaction simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub success: bool,
    pub gas_used: U256,
    pub return_data: Bytes,
    pub state_changes: Vec<StateChange>,
    pub logs: Vec<SimulatedLog>,
}

/// State change from simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub address: Address,
    pub slot: B256,
    pub old_value: B256,
    pub new_value: B256,
}

/// Simulated log event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatedLog {
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Bytes,
}

/// Safety checker for transaction validation
pub struct SafetyChecker {
    /// Blacklisted addresses
    blacklist: HashMap<u64, Vec<Address>>,
    /// Known malicious patterns
    malicious_patterns: Vec<MaliciousPattern>,
    /// Risk thresholds
    risk_thresholds: RiskThresholds,
}

/// Pattern for detecting malicious transactions
#[derive(Debug, Clone)]
pub struct MaliciousPattern {
    pub name: String,
    pub pattern_type: PatternType,
    pub severity: RiskSeverity,
}

/// Types of malicious patterns
#[derive(Debug, Clone)]
pub enum PatternType {
    /// Suspicious function selector
    FunctionSelector(Vec<u8>),
    /// Unusual gas limit
    GasAnomaly { min: U256, max: U256 },
    /// Token approval to unknown address
    UnknownApproval,
    /// Reentrancy risk
    ReentrancyRisk,
}

/// Risk severity levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Risk thresholds configuration
#[derive(Debug, Clone)]
pub struct RiskThresholds {
    pub max_acceptable_risk: RiskSeverity,
    pub require_simulation: bool,
    pub max_value_at_risk: U256,
}

/// Result of intent conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    /// Generated transactions
    pub transactions: Vec<ConvertedTransaction>,
    /// Estimated total gas
    pub total_gas_estimate: U256,
    /// Risk assessment
    pub risk_assessment: RiskAssessment,
    /// Execution metadata
    pub metadata: ConversionMetadata,
}

/// Converted transaction ready for submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertedTransaction {
    /// Transaction ID
    pub id: B256,
    /// Intent ID this transaction implements
    pub intent_id: B256,
    /// Target contract address
    pub to: Address,
    /// Transaction value
    pub value: U256,
    /// Calldata
    pub data: Bytes,
    /// Gas parameters
    pub gas_params: GasParameters,
    /// Protocol used
    pub protocol: String,
    /// Execution order (if part of sequence)
    pub sequence_index: Option<usize>,
}

/// Risk assessment for converted transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub overall_risk: RiskSeverity,
    pub risk_factors: Vec<RiskFactor>,
    pub requires_confirmation: bool,
    pub simulation_results: Option<Vec<SimulationResult>>,
}

/// Individual risk factor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub factor_type: String,
    pub severity: RiskSeverity,
    pub description: String,
    pub mitigation: Option<String>,
}

/// Metadata for conversion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionMetadata {
    pub conversion_timestamp: u64,
    pub protocols_used: Vec<String>,
    pub expected_outcomes: Vec<ExpectedOutcome>,
    pub alternative_paths: Vec<AlternativePath>,
}

/// Expected outcome from transaction execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutcome {
    pub token: Address,
    pub amount: U256,
    pub recipient: Address,
    pub probability: f32,
}

/// Alternative execution path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativePath {
    pub protocol: String,
    pub gas_estimate: U256,
    pub expected_output: U256,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
}

/// Uniswap V3 protocol adapter
pub struct UniswapV3Adapter {
    router_addresses: HashMap<u64, Address>,
    quoter_addresses: HashMap<u64, Address>,
}

#[async_trait]
impl ProtocolAdapter for UniswapV3Adapter {
    fn name(&self) -> &str {
        "uniswap_v3"
    }
    
    async fn build_calldata(
        &self,
        intent: &Intent,
        context: &ConversionContext,
    ) -> Result<Bytes> {
        match &intent.intent_type {
            IntentType::Swap { token_in, token_out, amount, slippage } => {
                // Build Uniswap V3 swap calldata
                let function_selector = hex::decode("04e45aaf")?; // exactInputSingle
                
                // Encode parameters (simplified)
                let mut calldata = Vec::new();
                calldata.extend_from_slice(&function_selector);
                
                // Add encoded parameters
                // This is simplified - actual implementation would use proper ABI encoding
                
                Ok(Bytes::from(calldata))
            }
            _ => Err(eyre!("Unsupported intent type for Uniswap V3")),
        }
    }
    
    fn get_contract_address(&self, chain_id: u64) -> Option<Address> {
        self.router_addresses.get(&chain_id).copied()
    }
    
    async fn estimate_gas(
        &self,
        intent: &Intent,
        context: &ConversionContext,
    ) -> Result<U256> {
        // Simplified gas estimation
        match &intent.intent_type {
            IntentType::Swap { .. } => Ok(U256::from(200_000u64)),
            _ => Ok(U256::from(100_000u64)),
        }
    }
    
    fn validate_intent(&self, intent: &Intent) -> Result<()> {
        match &intent.intent_type {
            IntentType::Swap { .. } => Ok(()),
            _ => Err(eyre!("Intent type not supported by Uniswap V3")),
        }
    }
}

impl IntentConverter {
    /// Create new intent converter
    pub fn new() -> Self {
        let mut protocol_adapters: HashMap<String, Box<dyn ProtocolAdapter>> = HashMap::new();
        
        // Register protocol adapters
        protocol_adapters.insert(
            "uniswap_v3".to_string(),
            Box::new(UniswapV3Adapter {
                router_addresses: Self::init_uniswap_addresses(),
                quoter_addresses: HashMap::new(),
            }),
        );
        
        Self {
            protocol_adapters,
            tx_builder: TransactionBuilder::new(),
            gas_estimator: GasEstimator::new(),
            safety_checker: SafetyChecker::new(),
        }
    }
    
    fn init_uniswap_addresses() -> HashMap<u64, Address> {
        let mut addresses = HashMap::new();
        // Mainnet
        addresses.insert(1, "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45".parse().unwrap());
        // Polygon
        addresses.insert(137, "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45".parse().unwrap());
        addresses
    }
    
    /// Convert intent to blockchain transactions
    pub async fn convert_intent(
        &self,
        intent: &Intent,
        context: ConversionContext,
    ) -> Result<ConversionResult> {
        // Select appropriate protocol
        let protocol = self.select_protocol(intent)?;
        
        // Validate intent with protocol
        protocol.validate_intent(intent)?;
        
        // Build calldata
        let calldata = protocol.build_calldata(intent, &context).await?;
        
        // Get contract address
        let to = protocol.get_contract_address(context.chain_id)
            .ok_or_else(|| eyre!("Protocol not available on chain {}", context.chain_id))?;
        
        // Estimate gas
        let gas_estimate = protocol.estimate_gas(intent, &context).await?;
        let gas_params = self.tx_builder.build_gas_params(gas_estimate, &context)?;
        
        // Build transaction
        let tx = ConvertedTransaction {
            id: B256::random(),
            intent_id: intent.id,
            to,
            value: U256::ZERO, // Most DeFi operations don't require ETH
            data: calldata,
            gas_params,
            protocol: protocol.name().to_string(),
            sequence_index: None,
        };
        
        // Safety check
        let risk_assessment = self.safety_checker.assess_transaction(&tx, &context)?;
        
        // Build metadata
        let metadata = ConversionMetadata {
            conversion_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            protocols_used: vec![protocol.name().to_string()],
            expected_outcomes: self.calculate_expected_outcomes(intent, &tx)?,
            alternative_paths: self.find_alternative_paths(intent, &context).await?,
        };
        
        Ok(ConversionResult {
            transactions: vec![tx],
            total_gas_estimate: gas_estimate,
            risk_assessment,
            metadata,
        })
    }
    
    /// Convert execution plan to transactions
    pub async fn convert_execution_plan(
        &self,
        plan: &ExecutionPlan,
        intents: &[Intent],
        context: ConversionContext,
    ) -> Result<ConversionResult> {
        let mut transactions = Vec::new();
        let mut total_gas = U256::ZERO;
        let mut all_protocols = Vec::new();
        
        for (index, step) in plan.steps.iter().enumerate() {
            let protocol = self.protocol_adapters.get(&step.protocol)
                .ok_or_else(|| eyre!("Unknown protocol: {}", step.protocol))?;
            
            // For now, use the first intent - in practice would map step to specific intent
            let intent = &intents[0];
            
            let calldata = protocol.build_calldata(intent, &context).await?;
            let to = protocol.get_contract_address(context.chain_id)
                .ok_or_else(|| eyre!("Protocol not available on chain {}", context.chain_id))?;
            
            let gas_estimate = protocol.estimate_gas(intent, &context).await?;
            total_gas = total_gas + gas_estimate;
            
            let tx = ConvertedTransaction {
                id: B256::random(),
                intent_id: intent.id,
                to,
                value: U256::ZERO,
                data: calldata,
                gas_params: self.tx_builder.build_gas_params(gas_estimate, &context)?,
                protocol: step.protocol.clone(),
                sequence_index: Some(index),
            };
            
            transactions.push(tx);
            all_protocols.push(step.protocol.clone());
        }
        
        let risk_assessment = self.safety_checker.assess_batch(&transactions, &context)?;
        
        let metadata = ConversionMetadata {
            conversion_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            protocols_used: all_protocols,
            expected_outcomes: vec![],
            alternative_paths: vec![],
        };
        
        Ok(ConversionResult {
            transactions,
            total_gas_estimate: total_gas,
            risk_assessment,
            metadata,
        })
    }
    
    /// Select best protocol for intent
    fn select_protocol(&self, intent: &Intent) -> Result<&Box<dyn ProtocolAdapter>> {
        // For now, simple selection based on intent type
        match &intent.intent_type {
            IntentType::Swap { .. } => {
                self.protocol_adapters.get("uniswap_v3")
                    .ok_or_else(|| eyre!("No protocol available for swaps"))
            }
            _ => Err(eyre!("No protocol available for intent type")),
        }
    }
    
    /// Calculate expected outcomes
    fn calculate_expected_outcomes(
        &self,
        intent: &Intent,
        tx: &ConvertedTransaction,
    ) -> Result<Vec<ExpectedOutcome>> {
        match &intent.intent_type {
            IntentType::Swap { token_out, amount, .. } => {
                Ok(vec![ExpectedOutcome {
                    token: *token_out,
                    amount: *amount, // Simplified - would calculate with slippage
                    recipient: Address::ZERO, // Would extract from context
                    probability: 0.95,
                }])
            }
            _ => Ok(vec![]),
        }
    }
    
    /// Find alternative execution paths
    async fn find_alternative_paths(
        &self,
        intent: &Intent,
        context: &ConversionContext,
    ) -> Result<Vec<AlternativePath>> {
        // For now, return empty list
        // In practice, would check multiple protocols and routes
        Ok(vec![])
    }
}

impl TransactionBuilder {
    pub fn new() -> Self {
        Self {
            eip1559_enabled: true,
            gas_buffer: 0.2, // 20% buffer
        }
    }
    
    pub fn build_gas_params(
        &self,
        base_estimate: U256,
        context: &ConversionContext,
    ) -> Result<GasParameters> {
        let gas_limit = base_estimate + (base_estimate * U256::from(20u64) / U256::from(100u64));
        
        Ok(GasParameters {
            max_fee_per_gas: context.gas_params.max_fee_per_gas,
            max_priority_fee_per_gas: context.gas_params.max_priority_fee_per_gas,
            gas_limit,
        })
    }
}

impl GasEstimator {
    pub fn new() -> Self {
        Self {
            historical_data: HashMap::new(),
            simulator: Box::new(MockSimulator),
        }
    }
}

impl SafetyChecker {
    pub fn new() -> Self {
        Self {
            blacklist: HashMap::new(),
            malicious_patterns: Self::init_patterns(),
            risk_thresholds: RiskThresholds {
                max_acceptable_risk: RiskSeverity::Medium,
                require_simulation: true,
                max_value_at_risk: U256::from(10_000u64) * U256::from(10u64).pow(U256::from(18u64)),
            },
        }
    }
    
    fn init_patterns() -> Vec<MaliciousPattern> {
        vec![
            MaliciousPattern {
                name: "unusual_gas_limit".to_string(),
                pattern_type: PatternType::GasAnomaly {
                    min: U256::from(21_000u64),
                    max: U256::from(10_000_000u64),
                },
                severity: RiskSeverity::Medium,
            },
        ]
    }
    
    pub fn assess_transaction(
        &self,
        tx: &ConvertedTransaction,
        context: &ConversionContext,
    ) -> Result<RiskAssessment> {
        let mut risk_factors = Vec::new();
        
        // Check blacklist
        if let Some(blacklisted) = self.blacklist.get(&context.chain_id) {
            if blacklisted.contains(&tx.to) {
                risk_factors.push(RiskFactor {
                    factor_type: "blacklisted_address".to_string(),
                    severity: RiskSeverity::Critical,
                    description: "Target address is blacklisted".to_string(),
                    mitigation: None,
                });
            }
        }
        
        // Check patterns
        for pattern in &self.malicious_patterns {
            if self.matches_pattern(tx, pattern) {
                risk_factors.push(RiskFactor {
                    factor_type: pattern.name.clone(),
                    severity: pattern.severity.clone(),
                    description: format!("Transaction matches pattern: {}", pattern.name),
                    mitigation: Some("Review transaction carefully".to_string()),
                });
            }
        }
        
        let overall_risk = risk_factors.iter()
            .map(|f| &f.severity)
            .max()
            .cloned()
            .unwrap_or(RiskSeverity::Low);
        
        Ok(RiskAssessment {
            overall_risk,
            risk_factors,
            requires_confirmation: overall_risk >= RiskSeverity::High,
            simulation_results: None,
        })
    }
    
    pub fn assess_batch(
        &self,
        transactions: &[ConvertedTransaction],
        context: &ConversionContext,
    ) -> Result<RiskAssessment> {
        let mut all_risk_factors = Vec::new();
        
        for tx in transactions {
            let assessment = self.assess_transaction(tx, context)?;
            all_risk_factors.extend(assessment.risk_factors);
        }
        
        let overall_risk = all_risk_factors.iter()
            .map(|f| &f.severity)
            .max()
            .cloned()
            .unwrap_or(RiskSeverity::Low);
        
        Ok(RiskAssessment {
            overall_risk,
            risk_factors: all_risk_factors,
            requires_confirmation: overall_risk >= RiskSeverity::High,
            simulation_results: None,
        })
    }
    
    fn matches_pattern(&self, tx: &ConvertedTransaction, pattern: &MaliciousPattern) -> bool {
        match &pattern.pattern_type {
            PatternType::GasAnomaly { min, max } => {
                &tx.gas_params.gas_limit < min || &tx.gas_params.gas_limit > max
            }
            _ => false,
        }
    }
}

/// Mock simulator for testing
struct MockSimulator;

#[async_trait]
impl TransactionSimulator for MockSimulator {
    async fn simulate_transaction(
        &self,
        tx: &Transaction,
        block: Option<u64>,
    ) -> Result<SimulationResult> {
        Ok(SimulationResult {
            success: true,
            gas_used: U256::from(150_000u64),
            return_data: Bytes::default(),
            state_changes: vec![],
            logs: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_intent_converter_creation() {
        let converter = IntentConverter::new();
        assert!(converter.protocol_adapters.contains_key("uniswap_v3"));
    }
    
    #[test]
    fn test_risk_severity_ordering() {
        assert!(RiskSeverity::Low < RiskSeverity::Medium);
        assert!(RiskSeverity::Medium < RiskSeverity::High);
        assert!(RiskSeverity::High < RiskSeverity::Critical);
    }
}