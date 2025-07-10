use crate::integrations::crypto::{
    BlsKeyPair, BlsPublicKey, BlsSignature, BlsAggregationService, AggregationRequest,
    OperatorSetManager, OperatorSet, OperatorCapability, ValidationRule,
    SlashingRedistributionManager, SlashingReason, RedistributionPolicy,
    MultiQuorumManager, QuorumConfig, QuorumAssignment, SecurityLevel,
    TaskArchetypeManager, TaskArchetype, TaskCategory, ArchetypeConfig,
    ProgrammaticIncentivesManager, RewardDistribution, RewardCalculationMetrics,
    IncentivePool, RewardPolicy,
};
use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tokio::sync::{RwLock, mpsc};
use std::sync::Arc;

#[derive(Debug)]
pub struct EigenLayerIntegration {
    avs_registry: String,
    operator_address: String,
    service_manager: String,
    task_queue: Arc<RwLock<HashMap<String, AVSTask>>>,
    operator_stakes: Arc<RwLock<HashMap<String, u64>>>,
    operator_bls_keys: Arc<RwLock<HashMap<String, BlsPublicKey>>>,
    bls_aggregation_sender: Option<mpsc::Sender<AggregationRequest>>,
    operator_set_manager: OperatorSetManager,
    slashing_redistribution_manager: SlashingRedistributionManager,
    multi_quorum_manager: MultiQuorumManager,
    task_archetype_manager: TaskArchetypeManager,
    programmatic_incentives_manager: ProgrammaticIncentivesManager,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AVSTask {
    pub task_id: String,
    pub agent_id: String,
    pub task_type: TaskType,
    pub input_data: Vec<u8>,
    pub timestamp: u64,
    pub required_stake: u64,
    pub quorum_threshold: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    RAGQuery {
        query: String,
        max_tokens: u32,
    },
    MemoryOperation {
        operation: MemoryOpType,
        memory_hash: [u8; 32],
    },
    AgentCoordination {
        agent_ids: Vec<String>,
        coordination_type: String,
    },
    ValidationRequest {
        data_hash: [u8; 32],
        validation_type: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryOpType {
    Store,
    Retrieve,
    Update,
    Delete,
    Checkpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignatureType {
    ECDSA(Vec<u8>),        // 65-byte ECDSA signature
    BLS(BlsSignature),     // BLS signature on G1 or G2
    Aggregated {           // Aggregated BLS signatures
        signatures: Vec<BlsSignature>,
        signers: Vec<BlsPublicKey>,
        total_stake: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResponse {
    pub task_id: String,
    pub result: Vec<u8>,
    pub signature: SignatureType,
    pub computation_proof: Option<Vec<u8>>,
    pub operator_stake: u64,
    pub timestamp: u64,
    pub operator_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorInfo {
    pub address: String,
    pub stake: u64,
    pub reputation_score: u32,
    pub tasks_completed: u64,
    pub last_active: u64,
    pub bls_public_key: Option<BlsPublicKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumInfo {
    pub operators: Vec<String>,
    pub total_stake: u64,
    pub threshold_percentage: u8,
}

impl EigenLayerIntegration {
    pub fn new(avs_registry: String, operator_address: String, service_manager: String) -> Self {
        Self {
            avs_registry,
            operator_address,
            service_manager,
            task_queue: Arc::new(RwLock::new(HashMap::new())),
            operator_stakes: Arc::new(RwLock::new(HashMap::new())),
            operator_bls_keys: Arc::new(RwLock::new(HashMap::new())),
            bls_aggregation_sender: None,
            operator_set_manager: OperatorSetManager::new(),
            slashing_redistribution_manager: SlashingRedistributionManager::new(),
            multi_quorum_manager: MultiQuorumManager::new(),
            task_archetype_manager: TaskArchetypeManager::new(),
            programmatic_incentives_manager: ProgrammaticIncentivesManager::new(),
        }
    }
    
    pub fn with_bls_aggregation(mut self, sender: mpsc::Sender<AggregationRequest>) -> Self {
        self.bls_aggregation_sender = Some(sender);
        self
    }
    
    pub async fn register_operator(&self, stake_amount: u64, bls_keypair: Option<BlsKeyPair>, metadata: Option<String>) -> Result<()> {
        // In production: Call EigenLayer contracts to register operator
        // For now, simulate registration
        let mut stakes = self.operator_stakes.write().await;
        stakes.insert(self.operator_address.clone(), stake_amount);
        
        // Register BLS public key if provided
        if let Some(keypair) = bls_keypair {
            let mut bls_keys = self.operator_bls_keys.write().await;
            bls_keys.insert(self.operator_address.clone(), keypair.public_key.clone());
            
            tracing::info!(
                "Registered operator {} with stake {} ETH and BLS public key",
                self.operator_address,
                stake_amount as f64 / 1e18
            );
        } else {
            tracing::info!(
                "Registered operator {} with stake {} ETH (no BLS key)",
                self.operator_address,
                stake_amount as f64 / 1e18
            );
        }
        
        Ok(())
    }
    
    pub async fn register_bls_key(&self, operator: &str, bls_public_key: BlsPublicKey) -> Result<()> {
        let mut bls_keys = self.operator_bls_keys.write().await;
        bls_keys.insert(operator.to_string(), bls_public_key);
        
        tracing::info!("Registered BLS public key for operator {}", operator);
        Ok(())
    }
    
    pub async fn get_operator_bls_key(&self, operator: &str) -> Result<Option<BlsPublicKey>> {
        let bls_keys = self.operator_bls_keys.read().await;
        Ok(bls_keys.get(operator).cloned())
    }
    
    // Operator Set Management Methods
    pub async fn create_operator_set(&self, operator_set: OperatorSet) -> Result<String> {
        self.operator_set_manager.create_operator_set(operator_set).await
    }
    
    pub async fn add_operator_to_set(&self, operator: &str, set_id: &str, stake: u64) -> Result<()> {
        self.operator_set_manager.add_operator_to_set(operator, set_id, stake).await
    }
    
    pub async fn remove_operator_from_set(&self, operator: &str, set_id: &str) -> Result<()> {
        self.operator_set_manager.remove_operator_from_set(operator, set_id).await
    }
    
    pub async fn register_operator_capabilities(
        &self,
        operator: &str,
        capabilities: std::collections::HashSet<OperatorCapability>,
    ) -> Result<()> {
        self.operator_set_manager.register_operator_capabilities(operator, capabilities).await
    }
    
    pub async fn get_operator_sets_for_task(
        &self,
        required_capabilities: &std::collections::HashSet<OperatorCapability>,
        minimum_operators: usize,
    ) -> Result<Vec<String>> {
        self.operator_set_manager.get_operator_sets_for_task(required_capabilities, minimum_operators).await
    }
    
    pub async fn get_operator_set(&self, set_id: &str) -> Result<Option<OperatorSet>> {
        self.operator_set_manager.get_operator_set(set_id).await
    }
    
    pub async fn list_operator_sets(&self) -> Result<Vec<OperatorSet>> {
        self.operator_set_manager.list_operator_sets().await
    }
    
    // Slashing and Redistribution Methods
    pub async fn create_redistribution_policy(&self, policy: RedistributionPolicy) -> Result<String> {
        self.slashing_redistribution_manager.create_redistribution_policy(policy).await
    }
    
    pub async fn initialize_default_redistribution_policy(&self) -> Result<String> {
        self.slashing_redistribution_manager.create_default_policy().await
    }
    
    pub async fn get_treasury_balance(&self) -> Result<u64> {
        self.slashing_redistribution_manager.get_treasury_balance().await
    }
    
    pub async fn get_insurance_balance(&self) -> Result<u64> {
        self.slashing_redistribution_manager.get_insurance_balance().await
    }
    
    pub async fn get_operator_redistribution_balance(&self, operator: &str) -> Result<u64> {
        self.slashing_redistribution_manager.get_operator_balance(operator).await
    }
    
    // Multi-Quorum Management Methods
    pub async fn create_quorum_config(&self, config: QuorumConfig) -> Result<String> {
        self.multi_quorum_manager.create_quorum_config(config).await
    }
    
    pub async fn initialize_default_quorums(&self) -> Result<Vec<String>> {
        self.multi_quorum_manager.create_default_quorums().await
    }
    
    pub async fn assign_task_to_quorum(&self, task_id: &str, task_type: &TaskType) -> Result<QuorumAssignment> {
        // Get available operators
        let stakes = self.operator_stakes.read().await;
        let bls_keys = self.operator_bls_keys.read().await;
        
        let mut operators = Vec::new();
        for (address, &stake) in stakes.iter() {
            let bls_public_key = bls_keys.get(address).cloned();
            operators.push(OperatorInfo {
                address: address.clone(),
                stake,
                reputation_score: 95, // Mock value
                tasks_completed: 100, // Mock value
                last_active: chrono::Utc::now().timestamp() as u64,
                bls_public_key,
            });
        }
        
        self.multi_quorum_manager.assign_task_to_quorum(task_id, task_type, &operators).await
    }
    
    pub async fn submit_quorum_vote(
        &self,
        task_id: &str,
        operator: &str,
        vote: bool,
        signature: Option<Vec<u8>>,
    ) -> Result<()> {
        let stakes = self.operator_stakes.read().await;
        let stake = stakes.get(operator).copied().unwrap_or(0);
        
        self.multi_quorum_manager.submit_vote(task_id, operator, vote, stake, signature).await
    }
    
    pub async fn check_quorum_result(&self, task_id: &str) -> Result<Option<crate::integrations::crypto::QuorumResult>> {
        self.multi_quorum_manager.check_quorum_result(task_id).await
    }
    
    pub async fn get_quorum_config(&self, quorum_id: &str) -> Result<Option<QuorumConfig>> {
        self.multi_quorum_manager.get_quorum_config(quorum_id).await
    }
    
    pub async fn list_quorum_configs(&self) -> Result<Vec<QuorumConfig>> {
        self.multi_quorum_manager.list_quorum_configs().await
    }
    
    pub async fn get_operator_quorums(&self, operator: &str) -> Result<Vec<String>> {
        self.multi_quorum_manager.get_operator_quorums(operator).await
    }
    
    // Task Archetype Methods
    pub fn get_task_archetype(&self, archetype_id: &str) -> Option<&TaskArchetype> {
        self.task_archetype_manager.get_archetype(archetype_id)
    }
    
    pub fn list_task_archetypes(&self) -> Vec<&TaskArchetype> {
        self.task_archetype_manager.list_archetypes()
    }
    
    pub fn get_archetypes_by_category(&self, category: &TaskCategory) -> Vec<&TaskArchetype> {
        self.task_archetype_manager.get_archetypes_by_category(category)
    }
    
    pub fn create_task_from_archetype(
        &self,
        archetype_id: &str,
        input_data: serde_json::Value,
        custom_config: Option<ArchetypeConfig>,
    ) -> Result<TaskType> {
        self.task_archetype_manager.create_task_from_archetype(archetype_id, input_data, custom_config)
    }
    
    pub fn estimate_task_execution_cost(
        &self,
        archetype_id: &str,
        operator_count: usize,
        execution_time: u64,
    ) -> Result<u64> {
        self.task_archetype_manager.estimate_execution_cost(archetype_id, operator_count, execution_time)
    }
    
    // Programmatic Incentives Methods
    pub async fn initialize_incentive_system(&self) -> Result<()> {
        self.programmatic_incentives_manager.initialize_default_incentives().await
    }
    
    pub async fn update_operator_metrics(
        &self,
        operator: &str,
        metrics: RewardCalculationMetrics,
    ) -> Result<()> {
        self.programmatic_incentives_manager.update_operator_metrics(operator, metrics).await
    }
    
    pub async fn calculate_epoch_rewards(&self, epoch: u64) -> Result<RewardDistribution> {
        self.programmatic_incentives_manager.calculate_epoch_rewards(epoch, "default_policy").await
    }
    
    pub async fn distribute_epoch_rewards(&self, epoch: u64) -> Result<()> {
        self.programmatic_incentives_manager.distribute_rewards(epoch).await
    }
    
    pub async fn get_operator_eigen_rewards(&self, operator: &str) -> Result<u64> {
        self.programmatic_incentives_manager.get_operator_total_rewards(operator).await
    }
    
    pub async fn estimate_operator_rewards(
        &self,
        operator: &str,
        estimated_metrics: &RewardCalculationMetrics,
    ) -> Result<u64> {
        self.programmatic_incentives_manager.estimate_operator_reward(
            operator,
            estimated_metrics,
            "default_policy",
        ).await
    }
    
    pub async fn get_current_epoch(&self) -> Result<u64> {
        self.programmatic_incentives_manager.get_current_epoch().await
    }
    
    pub async fn advance_epoch(&self) -> Result<u64> {
        self.programmatic_incentives_manager.advance_epoch().await
    }
    
    pub async fn run_automatic_reward_distribution(&self) -> Result<()> {
        self.programmatic_incentives_manager.run_automatic_distribution().await
    }
    
    pub async fn create_incentive_pool(&self, pool: IncentivePool) -> Result<String> {
        self.programmatic_incentives_manager.create_incentive_pool(pool).await
    }
    
    pub async fn create_reward_policy(&self, policy: RewardPolicy) -> Result<String> {
        self.programmatic_incentives_manager.create_reward_policy(policy).await
    }
    
    pub async fn get_eigen_pool_balance(&self) -> Result<u64> {
        self.programmatic_incentives_manager.get_pool_balance("eigen_main").await
    }
    
    pub async fn submit_task(&self, mut task: AVSTask) -> Result<String> {
        // Generate unique task ID if not provided
        if task.task_id.is_empty() {
            task.task_id = format!("task_{}", uuid::Uuid::new_v4());
        }
        
        // Validate task parameters
        self.validate_task(&task)?;
        
        // Add to task queue
        let mut queue = self.task_queue.write().await;
        queue.insert(task.task_id.clone(), task.clone());
        
        tracing::info!(
            "Submitted AVS task {} of type {:?}",
            task.task_id,
            task.task_type
        );
        
        // In production: Submit to EigenLayer AVS contracts
        // Return task ID for tracking
        Ok(task.task_id)
    }
    
    pub async fn get_task_response(&self, task_id: &str) -> Result<Option<TaskResponse>> {
        // Check if task exists
        let queue = self.task_queue.read().await;
        if let Some(task) = queue.get(task_id) {
            // In production: Query AVS contracts for responses
            // For now, simulate a response after some conditions
            
            // Simulate computation proof for certain task types
            let computation_proof = match &task.task_type {
                TaskType::ValidationRequest { .. } => Some(vec![0u8; 256]), // ZK proof
                TaskType::MemoryOperation { .. } => Some(vec![1u8; 128]),   // Merkle proof
                _ => None,
            };
            
            // Check if operator has BLS key for BLS signature
            let bls_keys = self.operator_bls_keys.read().await;
            let signature = if let Some(bls_key) = bls_keys.get(&self.operator_address) {
                // In production: Sign with actual BLS key
                // For now, create mock BLS signature
                SignatureType::BLS(BlsSignature {
                    g1_signature: None,
                    g2_signature: Some(crate::integrations::crypto::G2Signature {
                        x: ["0".to_string(), "0".to_string()],
                        y: ["0".to_string(), "0".to_string()],
                    }),
                })
            } else {
                // Fallback to ECDSA
                SignatureType::ECDSA(vec![0u8; 65])
            };
            
            let response = TaskResponse {
                task_id: task_id.to_string(),
                result: self.generate_mock_result(&task.task_type),
                signature,
                computation_proof,
                operator_stake: 1_000_000_000_000_000_000, // 1 ETH
                timestamp: chrono::Utc::now().timestamp() as u64,
                operator_address: self.operator_address.clone(),
            };
            
            Ok(Some(response))
        } else {
            Ok(None)
        }
    }
    
    pub async fn validate_operator(&self, operator: &str) -> Result<bool> {
        let stakes = self.operator_stakes.read().await;
        
        // Check if operator is registered and has sufficient stake
        if let Some(&stake) = stakes.get(operator) {
            // Minimum stake requirement: 32 ETH
            let min_stake = 32_000_000_000_000_000_000u64;
            Ok(stake >= min_stake)
        } else {
            Ok(false)
        }
    }
    
    pub async fn slash_operator(&self, operator: &str, reason: &str) -> Result<()> {
        let mut stakes = self.operator_stakes.write().await;
        
        if let Some(stake) = stakes.get_mut(operator) {
            // Slash 10% of stake
            let slash_amount = *stake / 10;
            *stake = stake.saturating_sub(slash_amount);
            
            tracing::warn!(
                "Slashed operator {} for {} ETH. Reason: {}",
                operator,
                slash_amount as f64 / 1e18,
                reason
            );
            
            // Convert reason to SlashingReason enum
            let slashing_reason = match reason {
                "double_signing" => SlashingReason::DoubleSigning,
                "unavailable" => SlashingReason::Unavailability,
                "invalid_computation" => SlashingReason::InvalidComputation,
                _ => SlashingReason::RuleViolation(reason.to_string()),
            };
            
            // Use redistribution mechanism
            self.slashing_redistribution_manager.slash_and_redistribute(
                operator,
                slash_amount,
                slashing_reason,
                None,
                "default",
            ).await?;
            
            // In production: Call slashing contract
        }
        
        Ok(())
    }
    
    pub async fn get_operator_info(&self, operator: &str) -> Result<Option<OperatorInfo>> {
        let stakes = self.operator_stakes.read().await;
        let bls_keys = self.operator_bls_keys.read().await;
        
        if let Some(&stake) = stakes.get(operator) {
            let bls_public_key = bls_keys.get(operator).cloned();
            
            Ok(Some(OperatorInfo {
                address: operator.to_string(),
                stake,
                reputation_score: 95, // Mock reputation
                tasks_completed: 1337,
                last_active: chrono::Utc::now().timestamp() as u64,
                bls_public_key,
            }))
        } else {
            Ok(None)
        }
    }
    
    pub async fn get_quorum_info(&self) -> Result<QuorumInfo> {
        let stakes = self.operator_stakes.read().await;
        
        let operators: Vec<String> = stakes.keys().cloned().collect();
        let total_stake: u64 = stakes.values().sum();
        
        Ok(QuorumInfo {
            operators,
            total_stake,
            threshold_percentage: 67, // 2/3 majority
        })
    }
    
    fn validate_task(&self, task: &AVSTask) -> Result<()> {
        // Validate required stake
        if task.required_stake == 0 {
            return Err(eyre::eyre!("Task must specify required stake"));
        }
        
        // Validate quorum threshold
        if task.quorum_threshold == 0 || task.quorum_threshold > 100 {
            return Err(eyre::eyre!("Invalid quorum threshold"));
        }
        
        // Validate task-specific parameters
        match &task.task_type {
            TaskType::RAGQuery { max_tokens, .. } => {
                if *max_tokens == 0 || *max_tokens > 100_000 {
                    return Err(eyre::eyre!("Invalid max_tokens"));
                }
            }
            TaskType::MemoryOperation { memory_hash, .. } => {
                if memory_hash == &[0u8; 32] {
                    return Err(eyre::eyre!("Invalid memory hash"));
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    fn generate_mock_result(&self, task_type: &TaskType) -> Vec<u8> {
        match task_type {
            TaskType::RAGQuery { .. } => {
                b"RAG query result with context embeddings".to_vec()
            }
            TaskType::MemoryOperation { operation, .. } => {
                format!("Memory {:?} operation completed", operation).into_bytes()
            }
            TaskType::AgentCoordination { .. } => {
                b"Coordination consensus achieved".to_vec()
            }
            TaskType::ValidationRequest { .. } => {
                b"Validation proof generated".to_vec()
            }
        }
    }
    
    pub async fn verify_task_response(&self, response: &TaskResponse) -> Result<bool> {
        match &response.signature {
            SignatureType::ECDSA(sig) => {
                // In production: Verify ECDSA signature
                // For now, simulate verification
                Ok(sig.len() == 65)
            }
            SignatureType::BLS(bls_sig) => {
                // Get operator's BLS public key
                let bls_keys = self.operator_bls_keys.read().await;
                if let Some(pubkey) = bls_keys.get(&response.operator_address) {
                    // Verify BLS signature
                    let message = self.create_task_message(&response.task_id, &response.result);
                    BlsKeyPair::verify_signature(pubkey, &message, bls_sig)
                } else {
                    Err(eyre::eyre!("No BLS key found for operator"))
                }
            }
            SignatureType::Aggregated { signatures: _, signers, total_stake } => {
                // Verify aggregated signature meets quorum
                let quorum_info = self.get_quorum_info().await?;
                let quorum_met = *total_stake >= (quorum_info.total_stake * quorum_info.threshold_percentage as u64) / 100;
                
                if !quorum_met {
                    return Err(eyre::eyre!("Quorum not met: {} < required", total_stake));
                }
                
                // In production: Verify aggregated BLS signature
                Ok(true)
            }
        }
    }
    
    fn create_task_message(&self, task_id: &str, result: &[u8]) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(task_id.as_bytes());
        message.extend_from_slice(b"|");
        message.extend_from_slice(result);
        message
    }
    
    pub async fn aggregate_task_responses(&self, task_id: &str, responses: Vec<TaskResponse>) -> Result<TaskResponse> {
        if responses.is_empty() {
            return Err(eyre::eyre!("No responses to aggregate"));
        }
        
        let mut bls_responses = Vec::new();
        let mut total_stake = 0u64;
        
        for response in &responses {
            if let SignatureType::BLS(sig) = &response.signature {
                let bls_keys = self.operator_bls_keys.read().await;
                if let Some(pubkey) = bls_keys.get(&response.operator_address) {
                    bls_responses.push((sig.clone(), pubkey.clone(), response.operator_stake));
                    total_stake += response.operator_stake;
                }
            }
        }
        
        if bls_responses.is_empty() {
            return Err(eyre::eyre!("No BLS signatures to aggregate"));
        }
        
        // Use BLS aggregator
        let aggregated = crate::integrations::crypto::BlsAggregator::aggregate_signatures(bls_responses)?;
        
        // Return aggregated response
        let first_response = &responses[0];
        Ok(TaskResponse {
            task_id: task_id.to_string(),
            result: first_response.result.clone(),
            signature: SignatureType::Aggregated {
                signatures: vec![aggregated.signature],
                signers: aggregated.signers,
                total_stake: aggregated.total_stake,
            },
            computation_proof: first_response.computation_proof.clone(),
            operator_stake: total_stake,
            timestamp: chrono::Utc::now().timestamp() as u64,
            operator_address: "aggregated".to_string(),
        })
    }
}