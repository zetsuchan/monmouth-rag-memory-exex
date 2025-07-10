use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardDistribution {
    pub id: String,
    pub epoch: u64,
    pub total_rewards: u64,
    pub distributions: Vec<OperatorReward>,
    pub timestamp: u64,
    pub status: DistributionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DistributionStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReward {
    pub operator: String,
    pub eigen_rewards: u64,
    pub performance_multiplier: f64,
    pub tasks_completed: u64,
    pub uptime_percentage: f64,
    pub slash_penalty: u64,
    pub final_reward: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardCalculationMetrics {
    pub operator: String,
    pub epoch: u64,
    pub base_stake: u64,
    pub tasks_completed: u64,
    pub task_success_rate: f64,
    pub uptime_percentage: f64,
    pub response_time_avg: f64,
    pub slashing_events: u64,
    pub quality_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncentivePool {
    pub id: String,
    pub name: String,
    pub total_allocation: u64,
    pub current_balance: u64,
    pub inflation_rate: f64, // 4% annual for EIGEN
    pub distribution_frequency: DistributionFrequency,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DistributionFrequency {
    Daily,
    Weekly,
    Monthly,
    Epoch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardPolicy {
    pub id: String,
    pub name: String,
    pub base_reward_rate: f64,
    pub performance_bonus_rate: f64,
    pub uptime_threshold: f64,
    pub quality_weight: f64,
    pub stake_weight: f64,
    pub slash_penalty_rate: f64,
    pub minimum_tasks_threshold: u64,
}

#[derive(Debug)]
pub struct ProgrammaticIncentivesManager {
    incentive_pools: Arc<RwLock<HashMap<String, IncentivePool>>>,
    reward_policies: Arc<RwLock<HashMap<String, RewardPolicy>>>,
    reward_distributions: Arc<RwLock<HashMap<u64, RewardDistribution>>>, // keyed by epoch
    operator_metrics: Arc<RwLock<HashMap<String, RewardCalculationMetrics>>>,
    operator_rewards: Arc<RwLock<HashMap<String, u64>>>, // total accumulated rewards
    current_epoch: Arc<RwLock<u64>>,
}

impl ProgrammaticIncentivesManager {
    pub fn new() -> Self {
        Self {
            incentive_pools: Arc::new(RwLock::new(HashMap::new())),
            reward_policies: Arc::new(RwLock::new(HashMap::new())),
            reward_distributions: Arc::new(RwLock::new(HashMap::new())),
            operator_metrics: Arc::new(RwLock::new(HashMap::new())),
            operator_rewards: Arc::new(RwLock::new(HashMap::new())),
            current_epoch: Arc::new(RwLock::new(1)),
        }
    }
    
    pub async fn initialize_default_incentives(&self) -> Result<()> {
        // Create default EIGEN incentive pool
        let eigen_pool = IncentivePool {
            id: "eigen_main".to_string(),
            name: "EIGEN Token Rewards".to_string(),
            total_allocation: 1_000_000_000_000_000_000_000_000, // 1M EIGEN tokens
            current_balance: 1_000_000_000_000_000_000_000_000,
            inflation_rate: 0.04, // 4% annual
            distribution_frequency: DistributionFrequency::Daily,
            active: true,
        };
        
        // Create default reward policy
        let default_policy = RewardPolicy {
            id: "default_policy".to_string(),
            name: "Standard Reward Policy".to_string(),
            base_reward_rate: 0.0001, // 0.01% of stake per day
            performance_bonus_rate: 0.5, // Up to 50% bonus for excellent performance
            uptime_threshold: 0.95, // 95% uptime required for full rewards
            quality_weight: 0.3,
            stake_weight: 0.4,
            slash_penalty_rate: 0.1, // 10% reward reduction per slashing event
            minimum_tasks_threshold: 10, // Minimum 10 tasks per epoch
        };
        
        let mut pools = self.incentive_pools.write().await;
        pools.insert(eigen_pool.id.clone(), eigen_pool);
        
        let mut policies = self.reward_policies.write().await;
        policies.insert(default_policy.id.clone(), default_policy);
        
        info!("Initialized default incentive pools and policies");
        Ok(())
    }
    
    pub async fn update_operator_metrics(
        &self,
        operator: &str,
        metrics: RewardCalculationMetrics,
    ) -> Result<()> {
        let mut operator_metrics = self.operator_metrics.write().await;
        operator_metrics.insert(operator.to_string(), metrics);
        
        info!("Updated metrics for operator {}", operator);
        Ok(())
    }
    
    pub async fn calculate_epoch_rewards(&self, epoch: u64, policy_id: &str) -> Result<RewardDistribution> {
        let policies = self.reward_policies.read().await;
        let policy = policies.get(policy_id)
            .ok_or_else(|| eyre::eyre!("Reward policy {} not found", policy_id))?;
        
        let operator_metrics = self.operator_metrics.read().await;
        let pools = self.incentive_pools.read().await;
        let eigen_pool = pools.get("eigen_main")
            .ok_or_else(|| eyre::eyre!("EIGEN pool not found"))?;
        
        // Calculate daily reward pool from inflation
        let annual_inflation = eigen_pool.total_allocation as f64 * eigen_pool.inflation_rate;
        let daily_rewards = (annual_inflation / 365.0) as u64;
        
        let mut operator_rewards = Vec::new();
        let mut total_distributed = 0u64;
        
        for (operator, metrics) in operator_metrics.iter() {
            if metrics.epoch != epoch {
                continue; // Skip metrics from other epochs
            }
            
            let reward = self.calculate_operator_reward(metrics, policy, daily_rewards).await?;
            total_distributed += reward.final_reward;
            operator_rewards.push(reward);
        }
        
        let distribution = RewardDistribution {
            id: format!("dist_epoch_{}", epoch),
            epoch,
            total_rewards: total_distributed,
            distributions: operator_rewards,
            timestamp: chrono::Utc::now().timestamp() as u64,
            status: DistributionStatus::Pending,
        };
        
        let mut distributions = self.reward_distributions.write().await;
        distributions.insert(epoch, distribution.clone());
        
        info!("Calculated rewards for epoch {}: {} EIGEN tokens", epoch, total_distributed);
        Ok(distribution)
    }
    
    async fn calculate_operator_reward(
        &self,
        metrics: &RewardCalculationMetrics,
        policy: &RewardPolicy,
        total_pool: u64,
    ) -> Result<OperatorReward> {
        // Base reward proportional to stake
        let stake_proportion = metrics.base_stake as f64 / 1_000_000_000_000_000_000_000.0; // Total staked (mock)
        let base_reward = (total_pool as f64 * policy.base_reward_rate * stake_proportion) as u64;
        
        // Performance multiplier
        let performance_score = self.calculate_performance_score(metrics, policy);
        let performance_multiplier = 1.0 + (performance_score * policy.performance_bonus_rate);
        
        // Apply uptime threshold
        let uptime_penalty = if metrics.uptime_percentage < policy.uptime_threshold {
            0.5 // 50% penalty for low uptime
        } else {
            1.0
        };
        
        // Apply task completion threshold
        let task_penalty = if metrics.tasks_completed < policy.minimum_tasks_threshold {
            0.7 // 30% penalty for insufficient tasks
        } else {
            1.0
        };
        
        // Calculate slashing penalty
        let slash_penalty = (metrics.slashing_events as f64 * policy.slash_penalty_rate) * base_reward as f64;
        
        // Calculate final reward
        let gross_reward = (base_reward as f64 * performance_multiplier * uptime_penalty * task_penalty) as u64;
        let final_reward = gross_reward.saturating_sub(slash_penalty as u64);
        
        Ok(OperatorReward {
            operator: metrics.operator.clone(),
            eigen_rewards: gross_reward,
            performance_multiplier,
            tasks_completed: metrics.tasks_completed,
            uptime_percentage: metrics.uptime_percentage,
            slash_penalty: slash_penalty as u64,
            final_reward,
        })
    }
    
    fn calculate_performance_score(&self, metrics: &RewardCalculationMetrics, policy: &RewardPolicy) -> f64 {
        // Weighted performance score
        let quality_component = metrics.quality_score * policy.quality_weight;
        let success_component = metrics.task_success_rate * 0.3;
        let responsiveness_component = (1.0 / (metrics.response_time_avg + 1.0)) * 0.4;
        
        (quality_component + success_component + responsiveness_component).min(1.0)
    }
    
    pub async fn distribute_rewards(&self, epoch: u64) -> Result<()> {
        let mut distributions = self.reward_distributions.write().await;
        let distribution = distributions.get_mut(&epoch)
            .ok_or_else(|| eyre::eyre!("No distribution found for epoch {}", epoch))?;
        
        distribution.status = DistributionStatus::Processing;
        
        // Distribute rewards to operators
        let mut operator_rewards = self.operator_rewards.write().await;
        for reward in &distribution.distributions {
            let current_balance = operator_rewards.entry(reward.operator.clone()).or_insert(0);
            *current_balance += reward.final_reward;
            
            info!("Distributed {} EIGEN tokens to operator {}", 
                reward.final_reward, reward.operator);
        }
        
        // Update pool balance
        let mut pools = self.incentive_pools.write().await;
        if let Some(pool) = pools.get_mut("eigen_main") {
            pool.current_balance = pool.current_balance.saturating_sub(distribution.total_rewards);
        }
        
        distribution.status = DistributionStatus::Completed;
        
        info!("Completed reward distribution for epoch {}", epoch);
        Ok(())
    }
    
    pub async fn get_operator_total_rewards(&self, operator: &str) -> Result<u64> {
        let operator_rewards = self.operator_rewards.read().await;
        Ok(operator_rewards.get(operator).copied().unwrap_or(0))
    }
    
    pub async fn get_epoch_distribution(&self, epoch: u64) -> Result<Option<RewardDistribution>> {
        let distributions = self.reward_distributions.read().await;
        Ok(distributions.get(&epoch).cloned())
    }
    
    pub async fn advance_epoch(&self) -> Result<u64> {
        let mut current_epoch = self.current_epoch.write().await;
        *current_epoch += 1;
        
        info!("Advanced to epoch {}", *current_epoch);
        Ok(*current_epoch)
    }
    
    pub async fn get_current_epoch(&self) -> Result<u64> {
        let epoch = self.current_epoch.read().await;
        Ok(*epoch)
    }
    
    pub async fn create_incentive_pool(&self, pool: IncentivePool) -> Result<String> {
        let mut pools = self.incentive_pools.write().await;
        let id = pool.id.clone();
        pools.insert(id.clone(), pool);
        
        info!("Created incentive pool: {}", id);
        Ok(id)
    }
    
    pub async fn create_reward_policy(&self, policy: RewardPolicy) -> Result<String> {
        let mut policies = self.reward_policies.write().await;
        let id = policy.id.clone();
        policies.insert(id.clone(), policy);
        
        info!("Created reward policy: {}", id);
        Ok(id)
    }
    
    pub async fn get_pool_balance(&self, pool_id: &str) -> Result<u64> {
        let pools = self.incentive_pools.read().await;
        let pool = pools.get(pool_id)
            .ok_or_else(|| eyre::eyre!("Pool {} not found", pool_id))?;
        Ok(pool.current_balance)
    }
    
    pub async fn estimate_operator_reward(
        &self,
        operator: &str,
        estimated_metrics: &RewardCalculationMetrics,
        policy_id: &str,
    ) -> Result<u64> {
        let policies = self.reward_policies.read().await;
        let policy = policies.get(policy_id)
            .ok_or_else(|| eyre::eyre!("Policy {} not found", policy_id))?;
        
        let pools = self.incentive_pools.read().await;
        let eigen_pool = pools.get("eigen_main")
            .ok_or_else(|| eyre::eyre!("EIGEN pool not found"))?;
        
        let annual_inflation = eigen_pool.total_allocation as f64 * eigen_pool.inflation_rate;
        let daily_rewards = (annual_inflation / 365.0) as u64;
        
        let reward = self.calculate_operator_reward(estimated_metrics, policy, daily_rewards).await?;
        Ok(reward.final_reward)
    }
    
    // Automatic distribution scheduler
    pub async fn run_automatic_distribution(&self) -> Result<()> {
        let current_epoch = self.get_current_epoch().await?;
        
        // Calculate and distribute rewards for current epoch
        let distribution = self.calculate_epoch_rewards(current_epoch, "default_policy").await?;
        
        if distribution.total_rewards > 0 {
            self.distribute_rewards(current_epoch).await?;
        }
        
        // Advance to next epoch
        self.advance_epoch().await?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_programmatic_incentives() {
        let manager = ProgrammaticIncentivesManager::new();
        manager.initialize_default_incentives().await.unwrap();
        
        // Create test metrics
        let metrics = RewardCalculationMetrics {
            operator: "test_operator".to_string(),
            epoch: 1,
            base_stake: 10_000_000_000_000_000_000, // 10 ETH
            tasks_completed: 50,
            task_success_rate: 0.95,
            uptime_percentage: 0.98,
            response_time_avg: 0.5,
            slashing_events: 0,
            quality_score: 0.9,
        };
        
        manager.update_operator_metrics("test_operator", metrics).await.unwrap();
        
        // Calculate rewards
        let distribution = manager.calculate_epoch_rewards(1, "default_policy").await.unwrap();
        assert!(!distribution.distributions.is_empty());
        assert!(distribution.total_rewards > 0);
        
        // Distribute rewards
        manager.distribute_rewards(1).await.unwrap();
        
        // Check operator balance
        let balance = manager.get_operator_total_rewards("test_operator").await.unwrap();
        assert!(balance > 0);
    }
    
    #[tokio::test]
    async fn test_reward_estimation() {
        let manager = ProgrammaticIncentivesManager::new();
        manager.initialize_default_incentives().await.unwrap();
        
        let estimated_metrics = RewardCalculationMetrics {
            operator: "estimate_operator".to_string(),
            epoch: 1,
            base_stake: 20_000_000_000_000_000_000, // 20 ETH
            tasks_completed: 100,
            task_success_rate: 1.0,
            uptime_percentage: 1.0,
            response_time_avg: 0.2,
            slashing_events: 0,
            quality_score: 1.0,
        };
        
        let estimated_reward = manager.estimate_operator_reward(
            "estimate_operator",
            &estimated_metrics,
            "default_policy",
        ).await.unwrap();
        
        assert!(estimated_reward > 0);
    }
}