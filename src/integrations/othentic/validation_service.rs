//! Enhanced Validation Service for Othentic
//! 
//! Provides custom validation endpoints, rule management, and result aggregation
//! for AI inference and transaction validation.

use crate::integrations::othentic::{
    ValidationService, ValidationEndpoint, ValidationRule, ValidationResult,
    AggregatedResult, ResultAggregator, AggregationStrategy, SlashingCondition,
    SlashingSeverity,
};
use eyre::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;

impl ValidationService {
    pub fn new() -> Self {
        let mut aggregator = ResultAggregator {
            strategies: HashMap::new(),
            result_cache: Arc::new(RwLock::new(HashMap::new())),
        };
        
        // Register default aggregation strategies
        aggregator.strategies.insert(
            "consensus".to_string(),
            Box::new(ConsensusAggregation { threshold: 0.67 }),
        );
        aggregator.strategies.insert(
            "weighted".to_string(),
            Box::new(WeightedAggregation),
        );
        aggregator.strategies.insert(
            "optimistic".to_string(),
            Box::new(OptimisticAggregation),
        );
        
        Self {
            endpoints: Arc::new(RwLock::new(HashMap::new())),
            rules: Arc::new(RwLock::new(HashMap::new())),
            aggregator: Arc::new(aggregator),
        }
    }
    
    /// Register a validation endpoint
    pub async fn register_endpoint(&self, endpoint: ValidationEndpoint) -> Result<()> {
        let mut endpoints = self.endpoints.write().await;
        endpoints.insert(endpoint.endpoint_id.clone(), endpoint.clone());
        
        tracing::info!(
            "Registered validation endpoint {} at {}",
            endpoint.endpoint_id,
            endpoint.url
        );
        
        Ok(())
    }
    
    /// Update endpoint configuration
    pub async fn update_endpoint(
        &self,
        endpoint_id: &str,
        endpoint: ValidationEndpoint,
    ) -> Result<()> {
        let mut endpoints = self.endpoints.write().await;
        endpoints.insert(endpoint_id.to_string(), endpoint);
        Ok(())
    }
    
    /// Remove endpoint
    pub async fn remove_endpoint(&self, endpoint_id: &str) -> Result<()> {
        let mut endpoints = self.endpoints.write().await;
        endpoints.remove(endpoint_id);
        Ok(())
    }
    
    /// Register validation rule
    pub async fn register_rule(&self, rule: ValidationRule) -> Result<()> {
        let mut rules = self.rules.write().await;
        rules.insert(rule.rule_id.clone(), rule.clone());
        
        tracing::info!(
            "Registered validation rule {} for task type {}",
            rule.rule_id,
            rule.task_type
        );
        
        Ok(())
    }
    
    /// Get validation rule for task type
    pub async fn get_rule(&self, task_type: &str) -> Result<Option<ValidationRule>> {
        let rules = self.rules.read().await;
        Ok(rules.values().find(|r| r.task_type == task_type).cloned())
    }
    
    /// Validate a task using registered endpoints
    pub async fn validate_task(
        &self,
        task_id: &str,
        task_type: &str,
        task_data: &[u8],
    ) -> Result<AggregatedResult> {
        // Get validation rule
        let rule = self.get_rule(task_type).await?
            .ok_or_else(|| eyre::eyre!("No validation rule for task type {}", task_type))?;
        
        // Get suitable endpoints
        let endpoints = self.get_suitable_endpoints(task_type).await?;
        
        if endpoints.len() < rule.validators_required as usize {
            return Err(eyre::eyre!(
                "Insufficient validators: {} required, {} available",
                rule.validators_required,
                endpoints.len()
            ));
        }
        
        // Submit to validators
        let mut validation_futures = Vec::new();
        for endpoint in endpoints.iter().take(rule.validators_required as usize) {
            let future = self.submit_to_validator(
                endpoint,
                task_id,
                task_data,
                rule.timeout_ms,
            );
            validation_futures.push(future);
        }
        
        // Collect results
        let results = futures::future::join_all(validation_futures).await;
        let valid_results: Vec<ValidationResult> = results
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();
        
        // Check if we have enough results
        if valid_results.len() < rule.validators_required as usize {
            return Err(eyre::eyre!("Insufficient validation responses"));
        }
        
        // Aggregate results
        let strategy = self.aggregator.strategies
            .get("consensus")
            .ok_or_else(|| eyre::eyre!("Aggregation strategy not found"))?;
        
        let aggregated = strategy.aggregate(valid_results).await?;
        
        // Check consensus threshold
        if !aggregated.consensus_achieved {
            // Check slashing conditions
            for condition in &rule.slashing_conditions {
                if condition.condition_type == "consensus_failure" {
                    self.report_slashing_event(task_id, &condition).await?;
                }
            }
        }
        
        // Cache result
        let mut cache = self.aggregator.result_cache.write().await;
        cache.insert(task_id.to_string(), aggregated.clone());
        
        Ok(aggregated)
    }
    
    /// Get suitable endpoints for task type
    async fn get_suitable_endpoints(&self, task_type: &str) -> Result<Vec<ValidationEndpoint>> {
        let endpoints = self.endpoints.read().await;
        Ok(endpoints
            .values()
            .filter(|e| e.supported_types.contains(&task_type.to_string()))
            .cloned()
            .collect())
    }
    
    /// Submit task to validator
    async fn submit_to_validator(
        &self,
        endpoint: &ValidationEndpoint,
        task_id: &str,
        task_data: &[u8],
        timeout_ms: u64,
    ) -> Result<ValidationResult> {
        // In production: HTTP request to validator endpoint
        // For now, mock validation
        
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(task_data);
        let result_hash = hasher.finalize();
        
        let validator_addr = {
            let mut addr = [0u8; 20];
            addr.copy_from_slice(&endpoint.endpoint_id.as_bytes()[..20.min(endpoint.endpoint_id.len())]);
            addr
        };
        
        Ok(ValidationResult {
            validator: validator_addr,
            task_id: task_id.to_string(),
            result_hash: result_hash.into(),
            confidence: 0.95,
            proof: Some(vec![0u8; 64]), // Mock proof
        })
    }
    
    /// Report slashing event
    async fn report_slashing_event(
        &self,
        task_id: &str,
        condition: &SlashingCondition,
    ) -> Result<()> {
        tracing::warn!(
            "Slashing condition {} triggered for task {} with severity {:?}",
            condition.condition_type,
            task_id,
            condition.severity
        );
        
        // In production: Submit to slashing contract
        Ok(())
    }
    
    /// Get validation statistics
    pub async fn get_validation_stats(&self) -> Result<ValidationStats> {
        let endpoints = self.endpoints.read().await;
        let rules = self.rules.read().await;
        let cache = self.aggregator.result_cache.read().await;
        
        let mut task_type_counts: HashMap<String, u32> = HashMap::new();
        for rule in rules.values() {
            *task_type_counts.entry(rule.task_type.clone()).or_default() += 1;
        }
        
        Ok(ValidationStats {
            total_endpoints: endpoints.len() as u32,
            total_rules: rules.len() as u32,
            cached_results: cache.len() as u32,
            task_type_distribution: task_type_counts,
        })
    }
}

/// Consensus-based aggregation
pub struct ConsensusAggregation {
    pub threshold: f64,
}

#[async_trait]
impl AggregationStrategy for ConsensusAggregation {
    async fn aggregate(&self, results: Vec<ValidationResult>) -> Result<AggregatedResult> {
        if results.is_empty() {
            return Err(eyre::eyre!("No results to aggregate"));
        }
        
        // Group by result hash
        let mut hash_counts: HashMap<[u8; 32], Vec<[u8; 20]>> = HashMap::new();
        for result in &results {
            hash_counts
                .entry(result.result_hash)
                .or_default()
                .push(result.validator);
        }
        
        // Find majority result
        let total = results.len() as f64;
        let mut consensus_result = None;
        let mut consensus_validators = Vec::new();
        
        for (hash, validators) in hash_counts {
            let ratio = validators.len() as f64 / total;
            if ratio >= self.threshold {
                consensus_result = Some(hash);
                consensus_validators = validators;
                break;
            }
        }
        
        let consensus_achieved = consensus_result.is_some();
        let final_result = consensus_result
            .map(|h| h.to_vec())
            .unwrap_or_else(|| results[0].result_hash.to_vec());
        
        // Generate aggregation proof
        let proof = self.generate_consensus_proof(&results, consensus_achieved);
        
        Ok(AggregatedResult {
            task_id: results[0].task_id.clone(),
            final_result,
            consensus_achieved,
            participating_validators: results.iter().map(|r| r.validator).collect(),
            aggregation_proof: proof,
        })
    }
}

impl ConsensusAggregation {
    fn generate_consensus_proof(&self, results: &[ValidationResult], consensus: bool) -> Vec<u8> {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        
        // Include all result hashes
        for result in results {
            hasher.update(&result.result_hash);
        }
        
        // Include consensus status
        hasher.update(&[consensus as u8]);
        
        // Include threshold
        hasher.update(&self.threshold.to_le_bytes());
        
        hasher.finalize().to_vec()
    }
}

/// Weighted aggregation based on validator confidence
pub struct WeightedAggregation;

#[async_trait]
impl AggregationStrategy for WeightedAggregation {
    async fn aggregate(&self, results: Vec<ValidationResult>) -> Result<AggregatedResult> {
        if results.is_empty() {
            return Err(eyre::eyre!("No results to aggregate"));
        }
        
        // Weight by confidence scores
        let mut weighted_results: HashMap<[u8; 32], f64> = HashMap::new();
        let mut total_weight = 0.0;
        
        for result in &results {
            let weight = result.confidence;
            *weighted_results.entry(result.result_hash).or_default() += weight;
            total_weight += weight;
        }
        
        // Find highest weighted result
        let (final_hash, weight) = weighted_results
            .iter()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .ok_or_else(|| eyre::eyre!("No weighted result found"))?;
        
        let consensus_achieved = weight / total_weight > 0.5;
        
        Ok(AggregatedResult {
            task_id: results[0].task_id.clone(),
            final_result: final_hash.to_vec(),
            consensus_achieved,
            participating_validators: results.iter().map(|r| r.validator).collect(),
            aggregation_proof: vec![0u8; 32], // Mock proof
        })
    }
}

/// Optimistic aggregation - accepts first valid result
pub struct OptimisticAggregation;

#[async_trait]
impl AggregationStrategy for OptimisticAggregation {
    async fn aggregate(&self, results: Vec<ValidationResult>) -> Result<AggregatedResult> {
        let first = results.first()
            .ok_or_else(|| eyre::eyre!("No results to aggregate"))?;
        
        Ok(AggregatedResult {
            task_id: first.task_id.clone(),
            final_result: first.result_hash.to_vec(),
            consensus_achieved: true, // Optimistic assumption
            participating_validators: vec![first.validator],
            aggregation_proof: first.proof.clone().unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidationStats {
    pub total_endpoints: u32,
    pub total_rules: u32,
    pub cached_results: u32,
    pub task_type_distribution: HashMap<String, u32>,
}