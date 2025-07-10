use crate::integrations::crypto::BlsPublicKey;
use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorSet {
    pub id: String,
    pub name: String,
    pub description: String,
    pub operators: HashSet<String>,
    pub minimum_stake: u64,
    pub required_capabilities: HashSet<OperatorCapability>,
    pub custom_validation_rules: Vec<ValidationRule>,
    pub created_at: u64,
    pub updated_at: u64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OperatorCapability {
    GPUCompute,
    HighMemory,
    LowLatency,
    GeographicRegion(String),
    SpecializedHardware(String),
    BLSEnabled,
    TEEEnabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationRule {
    MinimumReputation(u32),
    MaximumSlashingRate(f64),
    MinimumUptime(f64),
    RequiredCertification(String),
    GeographicRestriction {
        allowed_regions: Vec<String>,
        blocked_regions: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorSetAllocation {
    pub operator: String,
    pub operator_set_id: String,
    pub allocated_stake: u64,
    pub allocation_timestamp: u64,
}

#[derive(Debug)]
pub struct OperatorSetManager {
    operator_sets: Arc<RwLock<HashMap<String, OperatorSet>>>,
    operator_allocations: Arc<RwLock<HashMap<String, Vec<OperatorSetAllocation>>>>,
    operator_capabilities: Arc<RwLock<HashMap<String, HashSet<OperatorCapability>>>>,
}

impl OperatorSetManager {
    pub fn new() -> Self {
        Self {
            operator_sets: Arc::new(RwLock::new(HashMap::new())),
            operator_allocations: Arc::new(RwLock::new(HashMap::new())),
            operator_capabilities: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn create_operator_set(&self, mut operator_set: OperatorSet) -> Result<String> {
        let mut sets = self.operator_sets.write().await;
        
        if operator_set.id.is_empty() {
            operator_set.id = format!("opset_{}", uuid::Uuid::new_v4());
        }
        
        operator_set.created_at = chrono::Utc::now().timestamp() as u64;
        operator_set.updated_at = operator_set.created_at;
        operator_set.active = true;
        
        let id = operator_set.id.clone();
        sets.insert(id.clone(), operator_set);
        
        info!("Created operator set: {}", id);
        Ok(id)
    }
    
    pub async fn add_operator_to_set(
        &self, 
        operator: &str, 
        set_id: &str,
        stake: u64,
    ) -> Result<()> {
        let mut sets = self.operator_sets.write().await;
        
        if let Some(operator_set) = sets.get_mut(set_id) {
            // Validate operator meets requirements
            if stake < operator_set.minimum_stake {
                return Err(eyre::eyre!(
                    "Operator stake {} is below minimum {} for set {}",
                    stake, operator_set.minimum_stake, set_id
                ));
            }
            
            // Check capabilities
            let capabilities = self.operator_capabilities.read().await;
            if let Some(op_caps) = capabilities.get(operator) {
                for required_cap in &operator_set.required_capabilities {
                    if !op_caps.contains(required_cap) {
                        return Err(eyre::eyre!(
                            "Operator {} lacks required capability: {:?}",
                            operator, required_cap
                        ));
                    }
                }
            } else if !operator_set.required_capabilities.is_empty() {
                return Err(eyre::eyre!("Operator {} has no registered capabilities", operator));
            }
            
            // Add operator to set
            operator_set.operators.insert(operator.to_string());
            operator_set.updated_at = chrono::Utc::now().timestamp() as u64;
            
            // Record allocation
            let allocation = OperatorSetAllocation {
                operator: operator.to_string(),
                operator_set_id: set_id.to_string(),
                allocated_stake: stake,
                allocation_timestamp: chrono::Utc::now().timestamp() as u64,
            };
            
            let mut allocations = self.operator_allocations.write().await;
            allocations
                .entry(operator.to_string())
                .or_insert_with(Vec::new)
                .push(allocation);
            
            info!("Added operator {} to set {} with stake {}", operator, set_id, stake);
            Ok(())
        } else {
            Err(eyre::eyre!("Operator set {} not found", set_id))
        }
    }
    
    pub async fn remove_operator_from_set(&self, operator: &str, set_id: &str) -> Result<()> {
        let mut sets = self.operator_sets.write().await;
        
        if let Some(operator_set) = sets.get_mut(set_id) {
            if operator_set.operators.remove(operator) {
                operator_set.updated_at = chrono::Utc::now().timestamp() as u64;
                
                // Remove allocation
                let mut allocations = self.operator_allocations.write().await;
                if let Some(op_allocations) = allocations.get_mut(operator) {
                    op_allocations.retain(|a| a.operator_set_id != set_id);
                }
                
                info!("Removed operator {} from set {}", operator, set_id);
                Ok(())
            } else {
                Err(eyre::eyre!("Operator {} not found in set {}", operator, set_id))
            }
        } else {
            Err(eyre::eyre!("Operator set {} not found", set_id))
        }
    }
    
    pub async fn register_operator_capabilities(
        &self,
        operator: &str,
        capabilities: HashSet<OperatorCapability>,
    ) -> Result<()> {
        let mut caps = self.operator_capabilities.write().await;
        caps.insert(operator.to_string(), capabilities);
        
        info!("Registered capabilities for operator {}", operator);
        Ok(())
    }
    
    pub async fn get_operator_sets_for_task(
        &self,
        required_capabilities: &HashSet<OperatorCapability>,
        minimum_operators: usize,
    ) -> Result<Vec<String>> {
        let sets = self.operator_sets.read().await;
        let mut matching_sets = Vec::new();
        
        for (id, set) in sets.iter() {
            if !set.active {
                continue;
            }
            
            // Check if set has enough operators
            if set.operators.len() < minimum_operators {
                continue;
            }
            
            // Check if set supports required capabilities
            let mut supports_all = true;
            for req_cap in required_capabilities {
                if !set.required_capabilities.contains(req_cap) {
                    // Set doesn't guarantee this capability
                    supports_all = false;
                    break;
                }
            }
            
            if supports_all {
                matching_sets.push(id.clone());
            }
        }
        
        Ok(matching_sets)
    }
    
    pub async fn validate_operator_for_rules(
        &self,
        operator: &str,
        rules: &[ValidationRule],
        operator_info: &crate::integrations::eigenlayer::OperatorInfo,
    ) -> Result<bool> {
        for rule in rules {
            match rule {
                ValidationRule::MinimumReputation(min_rep) => {
                    if operator_info.reputation_score < *min_rep {
                        warn!("Operator {} reputation {} below minimum {}", 
                            operator, operator_info.reputation_score, min_rep);
                        return Ok(false);
                    }
                }
                ValidationRule::MaximumSlashingRate(max_rate) => {
                    // In production: Calculate actual slashing rate
                    let slashing_rate = 0.01; // Mock: 1% slashing rate
                    if slashing_rate > *max_rate {
                        warn!("Operator {} slashing rate {} above maximum {}", 
                            operator, slashing_rate, max_rate);
                        return Ok(false);
                    }
                }
                ValidationRule::MinimumUptime(min_uptime) => {
                    // In production: Calculate actual uptime
                    let uptime = 0.99; // Mock: 99% uptime
                    if uptime < *min_uptime {
                        warn!("Operator {} uptime {} below minimum {}", 
                            operator, uptime, min_uptime);
                        return Ok(false);
                    }
                }
                ValidationRule::RequiredCertification(cert) => {
                    // In production: Check operator certifications
                    info!("Checking certification {} for operator {} (mock: passed)", cert, operator);
                }
                ValidationRule::GeographicRestriction { allowed_regions, blocked_regions } => {
                    // In production: Check operator's geographic location
                    info!("Checking geographic restrictions for operator {} (mock: passed)", operator);
                }
            }
        }
        
        Ok(true)
    }
    
    pub async fn get_operator_set(&self, set_id: &str) -> Result<Option<OperatorSet>> {
        let sets = self.operator_sets.read().await;
        Ok(sets.get(set_id).cloned())
    }
    
    pub async fn list_operator_sets(&self) -> Result<Vec<OperatorSet>> {
        let sets = self.operator_sets.read().await;
        Ok(sets.values().cloned().collect())
    }
    
    pub async fn get_operator_allocations(&self, operator: &str) -> Result<Vec<OperatorSetAllocation>> {
        let allocations = self.operator_allocations.read().await;
        Ok(allocations.get(operator).cloned().unwrap_or_default())
    }
    
    pub async fn deactivate_operator_set(&self, set_id: &str) -> Result<()> {
        let mut sets = self.operator_sets.write().await;
        
        if let Some(operator_set) = sets.get_mut(set_id) {
            operator_set.active = false;
            operator_set.updated_at = chrono::Utc::now().timestamp() as u64;
            
            info!("Deactivated operator set {}", set_id);
            Ok(())
        } else {
            Err(eyre::eyre!("Operator set {} not found", set_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_operator_set_management() {
        let manager = OperatorSetManager::new();
        
        // Create operator set
        let mut capabilities = HashSet::new();
        capabilities.insert(OperatorCapability::GPUCompute);
        capabilities.insert(OperatorCapability::BLSEnabled);
        
        let operator_set = OperatorSet {
            id: String::new(),
            name: "GPU Compute Set".to_string(),
            description: "Operators with GPU compute capability".to_string(),
            operators: HashSet::new(),
            minimum_stake: 10_000_000_000_000_000_000, // 10 ETH
            required_capabilities: capabilities.clone(),
            custom_validation_rules: vec![
                ValidationRule::MinimumReputation(80),
                ValidationRule::MinimumUptime(0.95),
            ],
            created_at: 0,
            updated_at: 0,
            active: true,
        };
        
        let set_id = manager.create_operator_set(operator_set).await.unwrap();
        
        // Register operator capabilities
        manager.register_operator_capabilities("operator1", capabilities).await.unwrap();
        
        // Add operator to set
        manager.add_operator_to_set("operator1", &set_id, 20_000_000_000_000_000_000).await.unwrap();
        
        // Get operator set
        let retrieved_set = manager.get_operator_set(&set_id).await.unwrap().unwrap();
        assert!(retrieved_set.operators.contains("operator1"));
        
        // Get operator allocations
        let allocations = manager.get_operator_allocations("operator1").await.unwrap();
        assert_eq!(allocations.len(), 1);
        assert_eq!(allocations[0].operator_set_id, set_id);
    }
}