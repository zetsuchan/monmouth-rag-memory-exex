use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    pub id: String,
    pub operator: String,
    pub amount: u64,
    pub reason: SlashingReason,
    pub timestamp: u64,
    pub evidence: Option<Vec<u8>>,
    pub dispute_deadline: u64,
    pub status: SlashingStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashingReason {
    DoubleSigning,
    Unavailability,
    InvalidComputation,
    RuleViolation(String),
    Consensus(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashingStatus {
    Pending,
    Confirmed,
    Disputed,
    Resolved,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedistributionEvent {
    pub id: String,
    pub slashing_event_id: String,
    pub total_amount: u64,
    pub distributions: Vec<RedistributionTarget>,
    pub timestamp: u64,
    pub status: RedistributionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedistributionStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedistributionTarget {
    pub target_type: RedistributionTargetType,
    pub recipient: String,
    pub amount: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedistributionTargetType {
    Operator,           // Redistribute to good operators
    Treasury,           // Send to AVS treasury
    Stakers,            // Redistribute to stakers
    Insurance,          // Send to insurance fund
    Burn,               // Burn the tokens
    Charity,            // Donate to charity
    Development,        // Fund development
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedistributionPolicy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<RedistributionRule>,
    pub active: bool,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedistributionRule {
    pub condition: SlashingCondition,
    pub distribution: Vec<DistributionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashingCondition {
    ReasonEquals(SlashingReason),
    AmountGreaterThan(u64),
    AmountLessThan(u64),
    OperatorReputationBelow(u32),
    FirstOffense,
    RepeatedOffense,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionRule {
    pub target: RedistributionTargetType,
    pub percentage: f64,
    pub max_amount: Option<u64>,
    pub recipients: Vec<String>,
}

#[derive(Debug)]
pub struct SlashingRedistributionManager {
    slashing_events: Arc<RwLock<HashMap<String, SlashingEvent>>>,
    redistribution_events: Arc<RwLock<HashMap<String, RedistributionEvent>>>,
    policies: Arc<RwLock<HashMap<String, RedistributionPolicy>>>,
    operator_balances: Arc<RwLock<HashMap<String, u64>>>,
    treasury_balance: Arc<RwLock<u64>>,
    insurance_balance: Arc<RwLock<u64>>,
}

impl SlashingRedistributionManager {
    pub fn new() -> Self {
        Self {
            slashing_events: Arc::new(RwLock::new(HashMap::new())),
            redistribution_events: Arc::new(RwLock::new(HashMap::new())),
            policies: Arc::new(RwLock::new(HashMap::new())),
            operator_balances: Arc::new(RwLock::new(HashMap::new())),
            treasury_balance: Arc::new(RwLock::new(0)),
            insurance_balance: Arc::new(RwLock::new(0)),
        }
    }
    
    pub async fn create_redistribution_policy(&self, policy: RedistributionPolicy) -> Result<String> {
        let mut policies = self.policies.write().await;
        let id = policy.id.clone();
        policies.insert(id.clone(), policy);
        
        info!("Created redistribution policy: {}", id);
        Ok(id)
    }
    
    pub async fn create_default_policy(&self) -> Result<String> {
        let policy = RedistributionPolicy {
            id: "default".to_string(),
            name: "Default Redistribution Policy".to_string(),
            description: "Standard redistribution for slashed funds".to_string(),
            rules: vec![
                RedistributionRule {
                    condition: SlashingCondition::ReasonEquals(SlashingReason::DoubleSigning),
                    distribution: vec![
                        DistributionRule {
                            target: RedistributionTargetType::Operator,
                            percentage: 0.5,
                            max_amount: None,
                            recipients: vec![],
                        },
                        DistributionRule {
                            target: RedistributionTargetType::Treasury,
                            percentage: 0.3,
                            max_amount: None,
                            recipients: vec![],
                        },
                        DistributionRule {
                            target: RedistributionTargetType::Insurance,
                            percentage: 0.2,
                            max_amount: None,
                            recipients: vec![],
                        },
                    ],
                },
                RedistributionRule {
                    condition: SlashingCondition::All,
                    distribution: vec![
                        DistributionRule {
                            target: RedistributionTargetType::Operator,
                            percentage: 0.4,
                            max_amount: None,
                            recipients: vec![],
                        },
                        DistributionRule {
                            target: RedistributionTargetType::Treasury,
                            percentage: 0.4,
                            max_amount: None,
                            recipients: vec![],
                        },
                        DistributionRule {
                            target: RedistributionTargetType::Insurance,
                            percentage: 0.2,
                            max_amount: None,
                            recipients: vec![],
                        },
                    ],
                },
            ],
            active: true,
            created_at: chrono::Utc::now().timestamp() as u64,
        };
        
        self.create_redistribution_policy(policy).await
    }
    
    pub async fn slash_and_redistribute(
        &self,
        operator: &str,
        amount: u64,
        reason: SlashingReason,
        evidence: Option<Vec<u8>>,
        policy_id: &str,
    ) -> Result<String> {
        // Create slashing event
        let slashing_event = SlashingEvent {
            id: format!("slash_{}", uuid::Uuid::new_v4()),
            operator: operator.to_string(),
            amount,
            reason: reason.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            evidence,
            dispute_deadline: chrono::Utc::now().timestamp() as u64 + 604800, // 7 days
            status: SlashingStatus::Pending,
        };
        
        let slashing_id = slashing_event.id.clone();
        
        // Store slashing event
        let mut slashing_events = self.slashing_events.write().await;
        slashing_events.insert(slashing_id.clone(), slashing_event);
        drop(slashing_events);
        
        // Apply redistribution policy
        let redistribution_id = self.apply_redistribution_policy(
            &slashing_id,
            amount,
            &reason,
            policy_id,
        ).await?;
        
        info!("Slashed operator {} for {} with redistribution {}", 
            operator, amount, redistribution_id);
        
        Ok(slashing_id)
    }
    
    async fn apply_redistribution_policy(
        &self,
        slashing_event_id: &str,
        amount: u64,
        reason: &SlashingReason,
        policy_id: &str,
    ) -> Result<String> {
        let policies = self.policies.read().await;
        let policy = policies.get(policy_id)
            .ok_or_else(|| eyre::eyre!("Policy {} not found", policy_id))?;
        
        // Find matching rule
        let matching_rule = policy.rules.iter()
            .find(|rule| self.matches_condition(&rule.condition, reason, amount))
            .or_else(|| policy.rules.iter().find(|rule| 
                matches!(rule.condition, SlashingCondition::All)
            ))
            .ok_or_else(|| eyre::eyre!("No matching redistribution rule found"))?;
        
        // Calculate distributions
        let mut distributions = Vec::new();
        for dist_rule in &matching_rule.distribution {
            let dist_amount = (amount as f64 * dist_rule.percentage) as u64;
            let final_amount = if let Some(max) = dist_rule.max_amount {
                dist_amount.min(max)
            } else {
                dist_amount
            };
            
            if final_amount > 0 {
                distributions.push(RedistributionTarget {
                    target_type: dist_rule.target.clone(),
                    recipient: self.select_recipient(&dist_rule.target, &dist_rule.recipients).await?,
                    amount: final_amount,
                    reason: format!("Redistribution from slashing: {:?}", reason),
                });
            }
        }
        
        // Create redistribution event
        let redistribution_event = RedistributionEvent {
            id: format!("redist_{}", uuid::Uuid::new_v4()),
            slashing_event_id: slashing_event_id.to_string(),
            total_amount: amount,
            distributions,
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: RedistributionStatus::Pending,
        };
        
        let redistribution_id = redistribution_event.id.clone();
        
        // Store redistribution event
        let mut redistribution_events = self.redistribution_events.write().await;
        redistribution_events.insert(redistribution_id.clone(), redistribution_event);
        drop(redistribution_events);
        
        // Execute redistribution
        self.execute_redistribution(&redistribution_id).await?;
        
        Ok(redistribution_id)
    }
    
    fn matches_condition(&self, condition: &SlashingCondition, reason: &SlashingReason, amount: u64) -> bool {
        match condition {
            SlashingCondition::ReasonEquals(expected) => {
                std::mem::discriminant(reason) == std::mem::discriminant(expected)
            }
            SlashingCondition::AmountGreaterThan(threshold) => amount > *threshold,
            SlashingCondition::AmountLessThan(threshold) => amount < *threshold,
            SlashingCondition::OperatorReputationBelow(_) => {
                // In production: Check operator reputation
                true
            }
            SlashingCondition::FirstOffense => {
                // In production: Check if this is first offense
                true
            }
            SlashingCondition::RepeatedOffense => {
                // In production: Check if this is repeated offense
                false
            }
            SlashingCondition::All => true,
        }
    }
    
    async fn select_recipient(&self, target_type: &RedistributionTargetType, recipients: &[String]) -> Result<String> {
        match target_type {
            RedistributionTargetType::Operator => {
                if recipients.is_empty() {
                    // Select random good operator
                    Ok("good_operator_1".to_string())
                } else {
                    Ok(recipients[0].clone())
                }
            }
            RedistributionTargetType::Treasury => Ok("treasury".to_string()),
            RedistributionTargetType::Stakers => Ok("stakers_pool".to_string()),
            RedistributionTargetType::Insurance => Ok("insurance_fund".to_string()),
            RedistributionTargetType::Burn => Ok("burn_address".to_string()),
            RedistributionTargetType::Charity => Ok("charity_address".to_string()),
            RedistributionTargetType::Development => Ok("dev_fund".to_string()),
        }
    }
    
    async fn execute_redistribution(&self, redistribution_id: &str) -> Result<()> {
        let redistribution_events = self.redistribution_events.read().await;
        let event = redistribution_events.get(redistribution_id)
            .ok_or_else(|| eyre::eyre!("Redistribution event {} not found", redistribution_id))?;
        
        for distribution in &event.distributions {
            match distribution.target_type {
                RedistributionTargetType::Operator => {
                    let mut balances = self.operator_balances.write().await;
                    *balances.entry(distribution.recipient.clone()).or_insert(0) += distribution.amount;
                    info!("Redistributed {} to operator {}", distribution.amount, distribution.recipient);
                }
                RedistributionTargetType::Treasury => {
                    let mut treasury = self.treasury_balance.write().await;
                    *treasury += distribution.amount;
                    info!("Redistributed {} to treasury", distribution.amount);
                }
                RedistributionTargetType::Insurance => {
                    let mut insurance = self.insurance_balance.write().await;
                    *insurance += distribution.amount;
                    info!("Redistributed {} to insurance fund", distribution.amount);
                }
                RedistributionTargetType::Stakers => {
                    info!("Redistributed {} to stakers pool", distribution.amount);
                }
                RedistributionTargetType::Burn => {
                    info!("Burned {} tokens", distribution.amount);
                }
                RedistributionTargetType::Charity => {
                    info!("Donated {} to charity", distribution.amount);
                }
                RedistributionTargetType::Development => {
                    info!("Allocated {} to development fund", distribution.amount);
                }
            }
        }
        
        drop(redistribution_events);
        
        // Update redistribution status
        let mut redistribution_events = self.redistribution_events.write().await;
        if let Some(event) = redistribution_events.get_mut(redistribution_id) {
            event.status = RedistributionStatus::Completed;
        }
        
        Ok(())
    }
    
    pub async fn get_slashing_event(&self, event_id: &str) -> Result<Option<SlashingEvent>> {
        let events = self.slashing_events.read().await;
        Ok(events.get(event_id).cloned())
    }
    
    pub async fn get_redistribution_event(&self, event_id: &str) -> Result<Option<RedistributionEvent>> {
        let events = self.redistribution_events.read().await;
        Ok(events.get(event_id).cloned())
    }
    
    pub async fn get_treasury_balance(&self) -> Result<u64> {
        let balance = self.treasury_balance.read().await;
        Ok(*balance)
    }
    
    pub async fn get_insurance_balance(&self) -> Result<u64> {
        let balance = self.insurance_balance.read().await;
        Ok(*balance)
    }
    
    pub async fn get_operator_balance(&self, operator: &str) -> Result<u64> {
        let balances = self.operator_balances.read().await;
        Ok(balances.get(operator).copied().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_slashing_redistribution() {
        let manager = SlashingRedistributionManager::new();
        
        // Create default policy
        let policy_id = manager.create_default_policy().await.unwrap();
        
        // Slash and redistribute
        let slashing_id = manager.slash_and_redistribute(
            "bad_operator",
            1000,
            SlashingReason::DoubleSigning,
            Some(b"evidence".to_vec()),
            &policy_id,
        ).await.unwrap();
        
        // Check slashing event
        let slashing_event = manager.get_slashing_event(&slashing_id).await.unwrap().unwrap();
        assert_eq!(slashing_event.operator, "bad_operator");
        assert_eq!(slashing_event.amount, 1000);
        
        // Check balances
        let treasury_balance = manager.get_treasury_balance().await.unwrap();
        let insurance_balance = manager.get_insurance_balance().await.unwrap();
        let operator_balance = manager.get_operator_balance("good_operator_1").await.unwrap();
        
        assert!(treasury_balance > 0);
        assert!(insurance_balance > 0);
        assert!(operator_balance > 0);
        
        // Total should equal slashed amount
        assert_eq!(treasury_balance + insurance_balance + operator_balance, 1000);
    }
}