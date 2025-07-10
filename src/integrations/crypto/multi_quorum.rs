use crate::integrations::crypto::{BlsPublicKey, OperatorCapability};
use crate::integrations::eigenlayer::{TaskType, OperatorInfo};
use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub threshold_percentage: u8,
    pub minimum_operators: usize,
    pub maximum_operators: Option<usize>,
    pub minimum_stake: u64,
    pub required_capabilities: HashSet<OperatorCapability>,
    pub task_types: HashSet<TaskTypePattern>,
    pub security_level: SecurityLevel,
    pub active: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TaskTypePattern {
    RAGQuery,
    MemoryOperation,
    AgentCoordination,
    ValidationRequest,
    All,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityLevel {
    Low,      // Basic security for simple tasks
    Medium,   // Standard security for most tasks
    High,     // Enhanced security for critical tasks
    Critical, // Maximum security for high-value tasks
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumAssignment {
    pub task_id: String,
    pub quorum_id: String,
    pub assigned_operators: Vec<String>,
    pub total_stake: u64,
    pub assignment_timestamp: u64,
    pub status: QuorumAssignmentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuorumAssignmentStatus {
    Pending,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumVote {
    pub task_id: String,
    pub quorum_id: String,
    pub operator: String,
    pub vote: bool,
    pub stake: u64,
    pub timestamp: u64,
    pub signature: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumResult {
    pub task_id: String,
    pub quorum_id: String,
    pub votes_for: u64,
    pub votes_against: u64,
    pub total_stake: u64,
    pub threshold_met: bool,
    pub participation_rate: f64,
    pub finalized_at: u64,
}

#[derive(Debug)]
pub struct MultiQuorumManager {
    quorum_configs: Arc<RwLock<HashMap<String, QuorumConfig>>>,
    quorum_assignments: Arc<RwLock<HashMap<String, QuorumAssignment>>>,
    quorum_votes: Arc<RwLock<HashMap<String, Vec<QuorumVote>>>>,
    operator_quorums: Arc<RwLock<HashMap<String, HashSet<String>>>>,
}

impl MultiQuorumManager {
    pub fn new() -> Self {
        Self {
            quorum_configs: Arc::new(RwLock::new(HashMap::new())),
            quorum_assignments: Arc::new(RwLock::new(HashMap::new())),
            quorum_votes: Arc::new(RwLock::new(HashMap::new())),
            operator_quorums: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn create_quorum_config(&self, mut config: QuorumConfig) -> Result<String> {
        if config.id.is_empty() {
            config.id = format!("quorum_{}", uuid::Uuid::new_v4());
        }
        
        config.created_at = chrono::Utc::now().timestamp() as u64;
        config.active = true;
        
        // Validate configuration
        self.validate_quorum_config(&config)?;
        
        let id = config.id.clone();
        let mut configs = self.quorum_configs.write().await;
        configs.insert(id.clone(), config);
        
        info!("Created quorum config: {}", id);
        Ok(id)
    }
    
    fn validate_quorum_config(&self, config: &QuorumConfig) -> Result<()> {
        if config.threshold_percentage == 0 || config.threshold_percentage > 100 {
            return Err(eyre::eyre!("Invalid threshold percentage: {}", config.threshold_percentage));
        }
        
        if config.minimum_operators == 0 {
            return Err(eyre::eyre!("Minimum operators must be at least 1"));
        }
        
        if let Some(max) = config.maximum_operators {
            if max < config.minimum_operators {
                return Err(eyre::eyre!("Maximum operators cannot be less than minimum"));
            }
        }
        
        Ok(())
    }
    
    pub async fn create_default_quorums(&self) -> Result<Vec<String>> {
        let mut quorum_ids = Vec::new();
        
        // Low security quorum for basic queries
        let low_security = QuorumConfig {
            id: "low_security".to_string(),
            name: "Low Security Quorum".to_string(),
            description: "For simple RAG queries and basic operations".to_string(),
            threshold_percentage: 51,
            minimum_operators: 3,
            maximum_operators: Some(10),
            minimum_stake: 1_000_000_000_000_000_000, // 1 ETH
            required_capabilities: HashSet::new(),
            task_types: {
                let mut types = HashSet::new();
                types.insert(TaskTypePattern::RAGQuery);
                types
            },
            security_level: SecurityLevel::Low,
            active: true,
            created_at: 0,
        };
        
        // High security quorum for validation and critical operations
        let high_security = QuorumConfig {
            id: "high_security".to_string(),
            name: "High Security Quorum".to_string(),
            description: "For validation requests and critical operations".to_string(),
            threshold_percentage: 75,
            minimum_operators: 5,
            maximum_operators: Some(20),
            minimum_stake: 10_000_000_000_000_000_000, // 10 ETH
            required_capabilities: {
                let mut caps = HashSet::new();
                caps.insert(OperatorCapability::BLSEnabled);
                caps.insert(OperatorCapability::TEEEnabled);
                caps
            },
            task_types: {
                let mut types = HashSet::new();
                types.insert(TaskTypePattern::ValidationRequest);
                types.insert(TaskTypePattern::MemoryOperation);
                types
            },
            security_level: SecurityLevel::High,
            active: true,
            created_at: 0,
        };
        
        // GPU compute quorum for specialized tasks
        let gpu_compute = QuorumConfig {
            id: "gpu_compute".to_string(),
            name: "GPU Compute Quorum".to_string(),
            description: "For GPU-accelerated computations".to_string(),
            threshold_percentage: 67,
            minimum_operators: 4,
            maximum_operators: Some(15),
            minimum_stake: 5_000_000_000_000_000_000, // 5 ETH
            required_capabilities: {
                let mut caps = HashSet::new();
                caps.insert(OperatorCapability::GPUCompute);
                caps.insert(OperatorCapability::BLSEnabled);
                caps
            },
            task_types: {
                let mut types = HashSet::new();
                types.insert(TaskTypePattern::AgentCoordination);
                types.insert(TaskTypePattern::Custom("ml_inference".to_string()));
                types
            },
            security_level: SecurityLevel::Medium,
            active: true,
            created_at: 0,
        };
        
        for config in [low_security, high_security, gpu_compute] {
            let id = self.create_quorum_config(config).await?;
            quorum_ids.push(id);
        }
        
        Ok(quorum_ids)
    }
    
    pub async fn assign_task_to_quorum(
        &self,
        task_id: &str,
        task_type: &TaskType,
        available_operators: &[OperatorInfo],
    ) -> Result<QuorumAssignment> {
        // Find suitable quorum
        let quorum_id = self.find_suitable_quorum(task_type).await?;
        
        // Select operators for this quorum
        let selected_operators = self.select_operators_for_quorum(
            &quorum_id,
            available_operators,
        ).await?;
        
        let total_stake = selected_operators.iter()
            .map(|op| op.stake)
            .sum();
        
        let assignment = QuorumAssignment {
            task_id: task_id.to_string(),
            quorum_id: quorum_id.clone(),
            assigned_operators: selected_operators.iter()
                .map(|op| op.address.clone())
                .collect(),
            total_stake,
            assignment_timestamp: chrono::Utc::now().timestamp() as u64,
            status: QuorumAssignmentStatus::Pending,
        };
        
        // Store assignment
        let mut assignments = self.quorum_assignments.write().await;
        assignments.insert(task_id.to_string(), assignment.clone());
        
        // Update operator-quorum mapping
        let mut operator_quorums = self.operator_quorums.write().await;
        for operator in &assignment.assigned_operators {
            operator_quorums
                .entry(operator.clone())
                .or_insert_with(HashSet::new)
                .insert(quorum_id.clone());
        }
        
        info!("Assigned task {} to quorum {} with {} operators", 
            task_id, quorum_id, assignment.assigned_operators.len());
        
        Ok(assignment)
    }
    
    async fn find_suitable_quorum(&self, task_type: &TaskType) -> Result<String> {
        let configs = self.quorum_configs.read().await;
        
        let task_pattern = self.task_type_to_pattern(task_type);
        
        // Find quorums that support this task type
        let mut suitable_quorums: Vec<&QuorumConfig> = configs
            .values()
            .filter(|config| {
                config.active && (
                    config.task_types.contains(&task_pattern) ||
                    config.task_types.contains(&TaskTypePattern::All)
                )
            })
            .collect();
        
        if suitable_quorums.is_empty() {
            return Err(eyre::eyre!("No suitable quorum found for task type: {:?}", task_type));
        }
        
        // Sort by security level (prefer higher security for important tasks)
        suitable_quorums.sort_by(|a, b| {
            self.security_level_priority(&b.security_level)
                .cmp(&self.security_level_priority(&a.security_level))
        });
        
        Ok(suitable_quorums[0].id.clone())
    }
    
    fn task_type_to_pattern(&self, task_type: &TaskType) -> TaskTypePattern {
        match task_type {
            TaskType::RAGQuery { .. } => TaskTypePattern::RAGQuery,
            TaskType::MemoryOperation { .. } => TaskTypePattern::MemoryOperation,
            TaskType::AgentCoordination { .. } => TaskTypePattern::AgentCoordination,
            TaskType::ValidationRequest { .. } => TaskTypePattern::ValidationRequest,
        }
    }
    
    fn security_level_priority(&self, level: &SecurityLevel) -> u8 {
        match level {
            SecurityLevel::Critical => 4,
            SecurityLevel::High => 3,
            SecurityLevel::Medium => 2,
            SecurityLevel::Low => 1,
        }
    }
    
    async fn select_operators_for_quorum(
        &self,
        quorum_id: &str,
        available_operators: &[OperatorInfo],
    ) -> Result<Vec<OperatorInfo>> {
        let configs = self.quorum_configs.read().await;
        let config = configs.get(quorum_id)
            .ok_or_else(|| eyre::eyre!("Quorum config {} not found", quorum_id))?;
        
        // Filter operators by requirements
        let mut eligible_operators: Vec<&OperatorInfo> = available_operators
            .iter()
            .filter(|op| {
                // Check minimum stake
                if op.stake < config.minimum_stake {
                    return false;
                }
                
                // Check required capabilities (simplified - in production would check actual capabilities)
                if !config.required_capabilities.is_empty() && op.bls_public_key.is_none() {
                    // If BLS is required but operator doesn't have BLS key
                    if config.required_capabilities.contains(&OperatorCapability::BLSEnabled) {
                        return false;
                    }
                }
                
                // Check reputation (basic check)
                if op.reputation_score < 70 {
                    return false;
                }
                
                true
            })
            .collect();
        
        if eligible_operators.len() < config.minimum_operators {
            return Err(eyre::eyre!(
                "Not enough eligible operators: {} < {}",
                eligible_operators.len(),
                config.minimum_operators
            ));
        }
        
        // Sort by stake (prefer higher stake operators)
        eligible_operators.sort_by(|a, b| b.stake.cmp(&a.stake));
        
        // Select operators up to maximum
        let max_operators = config.maximum_operators.unwrap_or(eligible_operators.len());
        let selected_count = eligible_operators.len().min(max_operators);
        
        Ok(eligible_operators[..selected_count]
            .iter()
            .map(|&op| op.clone())
            .collect())
    }
    
    pub async fn submit_vote(
        &self,
        task_id: &str,
        operator: &str,
        vote: bool,
        stake: u64,
        signature: Option<Vec<u8>>,
    ) -> Result<()> {
        let assignments = self.quorum_assignments.read().await;
        let assignment = assignments.get(task_id)
            .ok_or_else(|| eyre::eyre!("No quorum assignment found for task {}", task_id))?;
        
        // Verify operator is part of the quorum
        if !assignment.assigned_operators.contains(&operator.to_string()) {
            return Err(eyre::eyre!("Operator {} not part of quorum for task {}", operator, task_id));
        }
        
        let vote_entry = QuorumVote {
            task_id: task_id.to_string(),
            quorum_id: assignment.quorum_id.clone(),
            operator: operator.to_string(),
            vote,
            stake,
            timestamp: chrono::Utc::now().timestamp() as u64,
            signature,
        };
        
        let mut votes = self.quorum_votes.write().await;
        votes.entry(task_id.to_string())
            .or_insert_with(Vec::new)
            .push(vote_entry);
        
        info!("Recorded vote from operator {} for task {}: {}", operator, task_id, vote);
        Ok(())
    }
    
    pub async fn check_quorum_result(&self, task_id: &str) -> Result<Option<QuorumResult>> {
        let assignments = self.quorum_assignments.read().await;
        let assignment = assignments.get(task_id)
            .ok_or_else(|| eyre::eyre!("No quorum assignment found for task {}", task_id))?;
        
        let configs = self.quorum_configs.read().await;
        let config = configs.get(&assignment.quorum_id)
            .ok_or_else(|| eyre::eyre!("Quorum config {} not found", assignment.quorum_id))?;
        
        let votes = self.quorum_votes.read().await;
        let task_votes = votes.get(task_id).unwrap_or(&Vec::new());
        
        if task_votes.is_empty() {
            return Ok(None);
        }
        
        let votes_for = task_votes.iter()
            .filter(|v| v.vote)
            .map(|v| v.stake)
            .sum::<u64>();
        
        let votes_against = task_votes.iter()
            .filter(|v| !v.vote)
            .map(|v| v.stake)
            .sum::<u64>();
        
        let total_voting_stake = votes_for + votes_against;
        let required_stake = (assignment.total_stake * config.threshold_percentage as u64) / 100;
        
        let threshold_met = votes_for >= required_stake;
        let participation_rate = total_voting_stake as f64 / assignment.total_stake as f64;
        
        // Check if we have enough participation
        if participation_rate < 0.5 {
            return Ok(None); // Wait for more votes
        }
        
        Ok(Some(QuorumResult {
            task_id: task_id.to_string(),
            quorum_id: assignment.quorum_id.clone(),
            votes_for,
            votes_against,
            total_stake: assignment.total_stake,
            threshold_met,
            participation_rate,
            finalized_at: chrono::Utc::now().timestamp() as u64,
        }))
    }
    
    pub async fn get_quorum_config(&self, quorum_id: &str) -> Result<Option<QuorumConfig>> {
        let configs = self.quorum_configs.read().await;
        Ok(configs.get(quorum_id).cloned())
    }
    
    pub async fn list_quorum_configs(&self) -> Result<Vec<QuorumConfig>> {
        let configs = self.quorum_configs.read().await;
        Ok(configs.values().cloned().collect())
    }
    
    pub async fn get_operator_quorums(&self, operator: &str) -> Result<Vec<String>> {
        let operator_quorums = self.operator_quorums.read().await;
        Ok(operator_quorums.get(operator)
            .map(|quorums| quorums.iter().cloned().collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::eigenlayer::OperatorInfo;
    
    #[tokio::test]
    async fn test_multi_quorum_management() {
        let manager = MultiQuorumManager::new();
        
        // Create default quorums
        let quorum_ids = manager.create_default_quorums().await.unwrap();
        assert_eq!(quorum_ids.len(), 3);
        
        // Create mock operators
        let operators = vec![
            OperatorInfo {
                address: "operator1".to_string(),
                stake: 15_000_000_000_000_000_000, // 15 ETH
                reputation_score: 95,
                tasks_completed: 100,
                last_active: chrono::Utc::now().timestamp() as u64,
                bls_public_key: None,
            },
            OperatorInfo {
                address: "operator2".to_string(),
                stake: 20_000_000_000_000_000_000, // 20 ETH
                reputation_score: 90,
                tasks_completed: 200,
                last_active: chrono::Utc::now().timestamp() as u64,
                bls_public_key: None,
            },
        ];
        
        // Test task assignment
        let task_type = crate::integrations::eigenlayer::TaskType::ValidationRequest {
            data_hash: [0u8; 32],
            validation_type: "proof".to_string(),
        };
        
        let assignment = manager.assign_task_to_quorum(
            "test_task",
            &task_type,
            &operators,
        ).await.unwrap();
        
        assert_eq!(assignment.quorum_id, "high_security");
        assert!(!assignment.assigned_operators.is_empty());
        
        // Test voting
        manager.submit_vote(
            "test_task",
            "operator1",
            true,
            15_000_000_000_000_000_000,
            None,
        ).await.unwrap();
        
        let result = manager.check_quorum_result("test_task").await.unwrap();
        assert!(result.is_some());
    }
}