use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

use crate::context::preprocessing::PreprocessedContext;
use crate::memory_exex::agent_integration::AgentMemoryIntegration;
use crate::shared::types::{AgentAction, AgentContext, AgentMemoryState, MemoryType};
use crate::shared::transaction_analyzer::TransactionAnalysis;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedAgentContext {
    pub agent_address: Address,
    pub current_context: AgentContext,
    pub memory_state: AgentMemoryState,
    pub action_history: VecDeque<ActionRecord>,
    pub context_windows: HashMap<String, ContextWindow>,
    pub performance_metrics: PerformanceMetrics,
    pub relationships: HashMap<Address, RelationshipMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action: AgentAction,
    pub transaction_hash: B256,
    pub timestamp: DateTime<Utc>,
    pub success: bool,
    pub gas_used: u64,
    pub context_hash: B256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextWindow {
    pub window_id: String,
    pub window_type: WindowType,
    pub contexts: VecDeque<PreprocessedContext>,
    pub max_size: usize,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowType {
    Temporal(std::time::Duration),
    ActionBased(usize),
    Semantic(Vec<String>),
    Priority(f64),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_actions: u64,
    pub successful_actions: u64,
    pub total_gas_used: u64,
    pub average_priority_score: f64,
    pub context_switches: u64,
    pub memory_efficiency: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipMetrics {
    pub interaction_count: u64,
    pub trust_score: f64,
    pub common_actions: Vec<String>,
    pub last_interaction: DateTime<Utc>,
}

pub struct UnifiedContextManager {
    contexts: Arc<RwLock<HashMap<Address, UnifiedAgentContext>>>,
    memory_integration: Arc<AgentMemoryIntegration>,
    config: ContextManagerConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextManagerConfig {
    pub max_action_history: usize,
    pub default_window_size: usize,
    pub relationship_threshold: u64,
    pub performance_update_interval: u64,
}

impl Default for ContextManagerConfig {
    fn default() -> Self {
        Self {
            max_action_history: 1000,
            default_window_size: 50,
            relationship_threshold: 10,
            performance_update_interval: 100,
        }
    }
}

impl UnifiedContextManager {
    pub fn new(
        memory_integration: Arc<AgentMemoryIntegration>,
        config: ContextManagerConfig,
    ) -> Self {
        Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
            memory_integration,
            config,
        }
    }

    pub async fn get_or_create_context(
        &self,
        agent_address: Address,
    ) -> UnifiedAgentContext {
        let mut contexts = self.contexts.write().await;
        
        contexts.entry(agent_address).or_insert_with(|| {
            UnifiedAgentContext {
                agent_address,
                current_context: AgentContext {
                    agent_address,
                    last_action: AgentAction::Transfer {
                        to: Address::default(),
                        amount: alloy::primitives::U256::ZERO,
                    },
                    reputation_score: 50,
                    total_interactions: 0,
                    success_rate: 0.0,
                    specialization: vec![],
                },
                memory_state: AgentMemoryState {
                    agent_address,
                    working_memory: vec![],
                    episodic_buffer: vec![],
                    last_access_time: 0,
                    context_switches: 0,
                },
                action_history: VecDeque::with_capacity(self.config.max_action_history),
                context_windows: HashMap::new(),
                performance_metrics: PerformanceMetrics::default(),
                relationships: HashMap::new(),
            }
        }).clone()
    }

    pub async fn update_context(
        &self,
        agent_address: Address,
        transaction: &TransactionAnalysis,
        preprocessed: &PreprocessedContext,
        success: bool,
    ) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(&agent_address)
            .ok_or(ContextError::AgentNotFound)?;

        // Update action history
        let action_record = ActionRecord {
            action: preprocessed.action_type.clone(),
            transaction_hash: transaction.transaction_hash,
            timestamp: Utc::now(),
            success,
            gas_used: transaction.gas_used,
            context_hash: preprocessed.transaction_hash,
        };

        context.action_history.push_back(action_record.clone());
        if context.action_history.len() > self.config.max_action_history {
            context.action_history.pop_front();
        }

        // Update current context
        context.current_context.last_action = preprocessed.action_type.clone();
        context.current_context.total_interactions += 1;
        if success {
            context.current_context.success_rate = 
                (context.current_context.success_rate * (context.current_context.total_interactions - 1) as f64 + 1.0) 
                / context.current_context.total_interactions as f64;
        } else {
            context.current_context.success_rate = 
                (context.current_context.success_rate * (context.current_context.total_interactions - 1) as f64) 
                / context.current_context.total_interactions as f64;
        }

        // Update specialization based on actions
        self.update_specialization(&mut context.current_context);

        // Update performance metrics
        self.update_performance_metrics(context, &action_record, preprocessed);

        // Update context windows
        self.update_context_windows(context, preprocessed.clone()).await?;

        // Update relationships if interaction with another agent
        if let AgentAction::Transfer { to, .. } = &preprocessed.action_type {
            self.update_relationship(context, *to);
        }

        Ok(())
    }

    pub async fn create_context_window(
        &self,
        agent_address: Address,
        window_id: String,
        window_type: WindowType,
        max_size: usize,
    ) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(&agent_address)
            .ok_or(ContextError::AgentNotFound)?;

        let window = ContextWindow {
            window_id: window_id.clone(),
            window_type,
            contexts: VecDeque::with_capacity(max_size),
            max_size,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
        };

        context.context_windows.insert(window_id, window);
        Ok(())
    }

    pub async fn get_context_window(
        &self,
        agent_address: Address,
        window_id: &str,
    ) -> Result<Vec<PreprocessedContext>, ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(&agent_address)
            .ok_or(ContextError::AgentNotFound)?;

        let window = context.context_windows.get_mut(window_id)
            .ok_or(ContextError::WindowNotFound)?;

        window.last_accessed = Utc::now();
        Ok(window.contexts.iter().cloned().collect())
    }

    pub async fn sync_with_memory(
        &self,
        agent_address: Address,
    ) -> Result<(), ContextError> {
        let context = self.get_or_create_context(agent_address).await;

        // Get memory state from integration
        if let Some(memory_state) = self.memory_integration.get_agent_state(agent_address).await {
            let mut contexts = self.contexts.write().await;
            if let Some(unified_context) = contexts.get_mut(&agent_address) {
                unified_context.memory_state = memory_state;
            }
        }

        Ok(())
    }

    pub async fn get_agent_insights(
        &self,
        agent_address: Address,
    ) -> Result<AgentInsights, ContextError> {
        let contexts = self.contexts.read().await;
        let context = contexts.get(&agent_address)
            .ok_or(ContextError::AgentNotFound)?;

        let insights = AgentInsights {
            agent_address,
            primary_specialization: self.determine_primary_specialization(&context.current_context),
            performance_trend: self.calculate_performance_trend(&context.action_history),
            memory_utilization: self.calculate_memory_utilization(&context.memory_state),
            relationship_network: context.relationships.len(),
            activity_pattern: self.analyze_activity_pattern(&context.action_history),
            risk_score: self.calculate_risk_score(context),
        };

        Ok(insights)
    }

    fn update_specialization(&self, context: &mut AgentContext) {
        let mut action_counts: HashMap<String, u64> = HashMap::new();

        // Count recent actions (implementation would look at action history)
        // This is simplified for now
        match &context.last_action {
            AgentAction::Swap { .. } => {
                *action_counts.entry("swap".to_string()).or_insert(0) += 1;
            }
            AgentAction::AddLiquidity { .. } | AgentAction::RemoveLiquidity { .. } => {
                *action_counts.entry("liquidity".to_string()).or_insert(0) += 1;
            }
            AgentAction::Stake { .. } | AgentAction::Unstake { .. } => {
                *action_counts.entry("staking".to_string()).or_insert(0) += 1;
            }
            AgentAction::Transfer { .. } => {
                *action_counts.entry("transfer".to_string()).or_insert(0) += 1;
            }
        }

        // Update specialization based on most common actions
        context.specialization = action_counts
            .into_iter()
            .filter(|(_, count)| *count > 5) // Threshold for specialization
            .map(|(action, _)| action)
            .collect();
    }

    fn update_performance_metrics(
        &self,
        context: &mut UnifiedAgentContext,
        action: &ActionRecord,
        preprocessed: &PreprocessedContext,
    ) {
        let metrics = &mut context.performance_metrics;
        
        metrics.total_actions += 1;
        if action.success {
            metrics.successful_actions += 1;
        }
        metrics.total_gas_used += action.gas_used;
        
        // Update average priority score
        metrics.average_priority_score = 
            (metrics.average_priority_score * (metrics.total_actions - 1) as f64 + preprocessed.priority_score) 
            / metrics.total_actions as f64;
        
        metrics.context_switches = context.memory_state.context_switches;
        
        // Calculate memory efficiency (simplified)
        let memory_used = context.memory_state.working_memory.len() + context.memory_state.episodic_buffer.len();
        metrics.memory_efficiency = if memory_used > 0 {
            metrics.successful_actions as f64 / memory_used as f64
        } else {
            0.0
        };
    }

    async fn update_context_windows(
        &self,
        context: &mut UnifiedAgentContext,
        preprocessed: PreprocessedContext,
    ) -> Result<(), ContextError> {
        for (_, window) in context.context_windows.iter_mut() {
            match &window.window_type {
                WindowType::Temporal(duration) => {
                    // Remove old contexts outside the time window
                    let cutoff = Utc::now() - chrono::Duration::from_std(*duration)
                        .map_err(|_| ContextError::InvalidDuration)?;
                    
                    window.contexts.retain(|ctx| {
                        DateTime::<Utc>::from_timestamp(ctx.timestamp as i64, 0)
                            .map(|dt| dt > cutoff)
                            .unwrap_or(false)
                    });
                }
                WindowType::ActionBased(max_actions) => {
                    // Keep only the last N actions
                    while window.contexts.len() >= *max_actions {
                        window.contexts.pop_front();
                    }
                }
                WindowType::Semantic(tags) => {
                    // Only add if semantically relevant
                    let relevant = preprocessed.semantic_tags.iter()
                        .any(|tag| tags.contains(tag));
                    if !relevant {
                        continue;
                    }
                }
                WindowType::Priority(threshold) => {
                    // Only add if priority exceeds threshold
                    if preprocessed.priority_score < *threshold {
                        continue;
                    }
                }
            }

            window.contexts.push_back(preprocessed.clone());
            window.last_accessed = Utc::now();
        }

        Ok(())
    }

    fn update_relationship(
        &self,
        context: &mut UnifiedAgentContext,
        other_agent: Address,
    ) {
        let relationship = context.relationships.entry(other_agent).or_insert_with(|| {
            RelationshipMetrics {
                interaction_count: 0,
                trust_score: 0.5,
                common_actions: vec![],
                last_interaction: Utc::now(),
            }
        });

        relationship.interaction_count += 1;
        relationship.last_interaction = Utc::now();
        
        // Update trust score based on success rate
        if context.current_context.success_rate > 0.8 {
            relationship.trust_score = (relationship.trust_score * 0.9 + 0.1).min(1.0);
        }
    }

    fn determine_primary_specialization(&self, context: &AgentContext) -> String {
        context.specialization.first()
            .cloned()
            .unwrap_or_else(|| "generalist".to_string())
    }

    fn calculate_performance_trend(&self, history: &VecDeque<ActionRecord>) -> f64 {
        if history.len() < 10 {
            return 0.0;
        }

        let recent: Vec<_> = history.iter().rev().take(10).collect();
        let older: Vec<_> = history.iter().rev().skip(10).take(10).collect();

        let recent_success_rate = recent.iter().filter(|r| r.success).count() as f64 / recent.len() as f64;
        let older_success_rate = older.iter().filter(|r| r.success).count() as f64 / older.len() as f64;

        recent_success_rate - older_success_rate
    }

    fn calculate_memory_utilization(&self, memory_state: &AgentMemoryState) -> f64 {
        let total_memories = memory_state.working_memory.len() + memory_state.episodic_buffer.len();
        let max_capacity = 150; // Assuming max capacity
        
        total_memories as f64 / max_capacity as f64
    }

    fn analyze_activity_pattern(&self, history: &VecDeque<ActionRecord>) -> ActivityPattern {
        if history.is_empty() {
            return ActivityPattern::Inactive;
        }

        let recent_actions: Vec<_> = history.iter().rev().take(20).collect();
        let time_diffs: Vec<_> = recent_actions.windows(2)
            .map(|w| (w[0].timestamp - w[1].timestamp).num_seconds())
            .collect();

        if time_diffs.is_empty() {
            return ActivityPattern::Sporadic;
        }

        let avg_diff = time_diffs.iter().sum::<i64>() as f64 / time_diffs.len() as f64;

        match avg_diff {
            d if d < 60.0 => ActivityPattern::HighFrequency,
            d if d < 300.0 => ActivityPattern::Regular,
            d if d < 3600.0 => ActivityPattern::Sporadic,
            _ => ActivityPattern::LowFrequency,
        }
    }

    fn calculate_risk_score(&self, context: &UnifiedAgentContext) -> f64 {
        let mut risk = 0.0;

        // Factor 1: Low success rate
        if context.current_context.success_rate < 0.5 {
            risk += 0.3;
        }

        // Factor 2: High gas usage
        let avg_gas = context.performance_metrics.total_gas_used as f64 / context.performance_metrics.total_actions.max(1) as f64;
        if avg_gas > 100_000.0 {
            risk += 0.2;
        }

        // Factor 3: High context switching
        if context.memory_state.context_switches > 100 {
            risk += 0.2;
        }

        // Factor 4: Low relationship trust
        let avg_trust = if !context.relationships.is_empty() {
            context.relationships.values().map(|r| r.trust_score).sum::<f64>() / context.relationships.len() as f64
        } else {
            0.5
        };
        
        if avg_trust < 0.3 {
            risk += 0.3;
        }

        risk.min(1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInsights {
    pub agent_address: Address,
    pub primary_specialization: String,
    pub performance_trend: f64,
    pub memory_utilization: f64,
    pub relationship_network: usize,
    pub activity_pattern: ActivityPattern,
    pub risk_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActivityPattern {
    HighFrequency,
    Regular,
    Sporadic,
    LowFrequency,
    Inactive,
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("Agent not found")]
    AgentNotFound,
    #[error("Context window not found")]
    WindowNotFound,
    #[error("Invalid duration")]
    InvalidDuration,
    #[error("Memory sync failed: {0}")]
    MemorySyncError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::preprocessing::{ContextPreprocessor, ProcessingConfig};
    use crate::memory_exex::memory_store::MemoryStore;
    use crate::rag_exex::context_retrieval::ContextRetriever;

    #[tokio::test]
    async fn test_unified_context_management() {
        let memory_store = Arc::new(MemoryStore::new(Default::default()));
        let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
        let context_preprocessor = Arc::new(ContextPreprocessor::new(ProcessingConfig::default()));
        
        let (memory_integration, _) = AgentMemoryIntegration::new(
            Default::default(),
            memory_store,
            context_retriever,
            context_preprocessor,
        );

        let manager = UnifiedContextManager::new(
            Arc::new(memory_integration),
            ContextManagerConfig::default(),
        );

        let agent_address = Address::from([1u8; 20]);
        let context = manager.get_or_create_context(agent_address).await;

        assert_eq!(context.agent_address, agent_address);
        assert_eq!(context.performance_metrics.total_actions, 0);
    }
}