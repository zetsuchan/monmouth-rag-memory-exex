use alloy::primitives::{Address, B256};
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::interval;

use super::agent_standard::{AgentCapability, AgentRegistry};
use super::types::{AgentContext, AgentMemoryState};
use crate::rag_exex::agent_context::{UnifiedAgentContext, ActivityPattern};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub address: Address,
    pub lifecycle_state: LifecycleState,
    pub health_status: HealthStatus,
    pub resource_allocation: ResourceAllocation,
    pub performance_summary: PerformanceSummary,
    pub last_heartbeat: Instant,
    pub created_at: Instant,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LifecycleState {
    Initializing,
    Active,
    Idle,
    Suspended,
    Terminating,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub is_healthy: bool,
    pub health_score: f64,
    pub error_count: u64,
    pub consecutive_failures: u64,
    pub last_error: Option<ErrorInfo>,
    pub recovery_attempts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub error_type: String,
    pub message: String,
    pub timestamp: Instant,
    pub context: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    pub cpu_quota: f64,
    pub memory_limit_mb: u64,
    pub transaction_quota: u64,
    pub priority_level: PriorityLevel,
    pub current_load: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PriorityLevel {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    pub total_transactions: u64,
    pub successful_transactions: u64,
    pub average_response_time_ms: f64,
    pub throughput_per_second: f64,
    pub reputation_score: f64,
    pub specialization_scores: HashMap<String, f64>,
}

#[derive(Debug)]
pub struct AgentStateManager {
    states: Arc<DashMap<Address, AgentState>>,
    state_history: Arc<RwLock<HashMap<Address, VecDeque<StateTransition>>>>,
    resource_pool: Arc<ResourcePool>,
    health_monitor: Arc<HealthMonitor>,
    load_balancer: Arc<LoadBalancer>,
    event_tx: mpsc::Sender<StateEvent>,
    config: StateManagerConfig,
}

#[derive(Debug, Clone)]
struct StateTransition {
    from_state: LifecycleState,
    to_state: LifecycleState,
    timestamp: Instant,
    reason: String,
}

#[derive(Debug, Clone)]
pub enum StateEvent {
    AgentRegistered { address: Address },
    StateChanged { address: Address, old_state: LifecycleState, new_state: LifecycleState },
    HealthChanged { address: Address, healthy: bool },
    ResourcesAllocated { address: Address, allocation: ResourceAllocation },
    AgentTerminated { address: Address, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManagerConfig {
    pub heartbeat_timeout_seconds: u64,
    pub health_check_interval_seconds: u64,
    pub max_consecutive_failures: u64,
    pub resource_rebalance_interval_seconds: u64,
    pub state_history_limit: usize,
}

impl Default for StateManagerConfig {
    fn default() -> Self {
        Self {
            heartbeat_timeout_seconds: 30,
            health_check_interval_seconds: 10,
            max_consecutive_failures: 5,
            resource_rebalance_interval_seconds: 60,
            state_history_limit: 100,
        }
    }
}

#[derive(Debug)]
struct ResourcePool {
    total_cpu: f64,
    total_memory_mb: u64,
    total_transaction_quota: u64,
    available_cpu: Arc<RwLock<f64>>,
    available_memory_mb: Arc<RwLock<u64>>,
    available_transaction_quota: Arc<RwLock<u64>>,
}

#[derive(Debug)]
struct HealthMonitor {
    check_interval: Duration,
    timeout_duration: Duration,
    recovery_strategies: HashMap<String, Box<dyn RecoveryStrategy>>,
}

#[derive(Debug)]
struct LoadBalancer {
    strategy: LoadBalancingStrategy,
    agent_loads: Arc<DashMap<Address, f64>>,
}

#[derive(Debug, Clone)]
enum LoadBalancingStrategy {
    RoundRobin,
    LeastLoaded,
    WeightedRoundRobin,
    PriorityBased,
}

impl AgentStateManager {
    pub fn new(config: StateManagerConfig) -> (Self, mpsc::Receiver<StateEvent>) {
        let (event_tx, event_rx) = mpsc::channel(1000);

        let resource_pool = Arc::new(ResourcePool {
            total_cpu: 100.0,
            total_memory_mb: 16384,
            total_transaction_quota: 100000,
            available_cpu: Arc::new(RwLock::new(100.0)),
            available_memory_mb: Arc::new(RwLock::new(16384)),
            available_transaction_quota: Arc::new(RwLock::new(100000)),
        });

        let health_monitor = Arc::new(HealthMonitor {
            check_interval: Duration::from_secs(config.health_check_interval_seconds),
            timeout_duration: Duration::from_secs(config.heartbeat_timeout_seconds),
            recovery_strategies: Self::default_recovery_strategies(),
        });

        let load_balancer = Arc::new(LoadBalancer {
            strategy: LoadBalancingStrategy::LeastLoaded,
            agent_loads: Arc::new(DashMap::new()),
        });

        let manager = Self {
            states: Arc::new(DashMap::new()),
            state_history: Arc::new(RwLock::new(HashMap::new())),
            resource_pool,
            health_monitor,
            load_balancer,
            event_tx,
            config,
        };

        // Start background tasks
        manager.start_health_monitoring();
        manager.start_resource_rebalancing();

        (manager, event_rx)
    }

    pub async fn register_agent(
        &self,
        address: Address,
        capabilities: Vec<AgentCapability>,
    ) -> Result<()> {
        let initial_allocation = self.allocate_initial_resources(&capabilities).await?;

        let state = AgentState {
            address,
            lifecycle_state: LifecycleState::Initializing,
            health_status: HealthStatus {
                is_healthy: true,
                health_score: 1.0,
                error_count: 0,
                consecutive_failures: 0,
                last_error: None,
                recovery_attempts: 0,
            },
            resource_allocation: initial_allocation,
            performance_summary: PerformanceSummary {
                total_transactions: 0,
                successful_transactions: 0,
                average_response_time_ms: 0.0,
                throughput_per_second: 0.0,
                reputation_score: 0.5,
                specialization_scores: HashMap::new(),
            },
            last_heartbeat: Instant::now(),
            created_at: Instant::now(),
            metadata: HashMap::new(),
        };

        self.states.insert(address, state);
        self.load_balancer.agent_loads.insert(address, 0.0);

        let _ = self.event_tx.send(StateEvent::AgentRegistered { address }).await;

        Ok(())
    }

    pub async fn transition_state(
        &self,
        address: Address,
        new_state: LifecycleState,
        reason: String,
    ) -> Result<()> {
        let mut old_state = LifecycleState::Terminated;

        if let Some(mut agent_state) = self.states.get_mut(&address) {
            old_state = agent_state.lifecycle_state.clone();
            
            // Validate state transition
            if !self.is_valid_transition(&old_state, &new_state) {
                return Err(eyre::eyre!(
                    "Invalid state transition from {:?} to {:?}",
                    old_state,
                    new_state
                ));
            }

            agent_state.lifecycle_state = new_state.clone();
        } else {
            return Err(eyre::eyre!("Agent not found"));
        }

        // Record transition
        self.record_state_transition(address, old_state.clone(), new_state.clone(), reason).await;

        // Handle state-specific actions
        match new_state {
            LifecycleState::Active => {
                self.handle_agent_activation(address).await?;
            }
            LifecycleState::Suspended => {
                self.handle_agent_suspension(address).await?;
            }
            LifecycleState::Terminating => {
                self.handle_agent_termination(address).await?;
            }
            _ => {}
        }

        let _ = self.event_tx.send(StateEvent::StateChanged {
            address,
            old_state,
            new_state,
        }).await;

        Ok(())
    }

    pub async fn update_heartbeat(&self, address: Address) -> Result<()> {
        if let Some(mut state) = self.states.get_mut(&address) {
            state.last_heartbeat = Instant::now();
            
            // Reset consecutive failures on successful heartbeat
            if state.health_status.consecutive_failures > 0 {
                state.health_status.consecutive_failures = 0;
                state.health_status.is_healthy = true;
                self.update_health_score(&mut state);
            }
            
            Ok(())
        } else {
            Err(eyre::eyre!("Agent not found"))
        }
    }

    pub async fn report_error(
        &self,
        address: Address,
        error_type: String,
        message: String,
        context: HashMap<String, String>,
    ) -> Result<()> {
        if let Some(mut state) = self.states.get_mut(&address) {
            state.health_status.error_count += 1;
            state.health_status.consecutive_failures += 1;
            state.health_status.last_error = Some(ErrorInfo {
                error_type: error_type.clone(),
                message,
                timestamp: Instant::now(),
                context,
            });

            // Check if agent should be marked unhealthy
            if state.health_status.consecutive_failures >= self.config.max_consecutive_failures {
                state.health_status.is_healthy = false;
                
                let _ = self.event_tx.send(StateEvent::HealthChanged {
                    address,
                    healthy: false,
                }).await;

                // Attempt recovery
                self.attempt_recovery(address, &error_type).await?;
            }

            self.update_health_score(&mut state);
            
            Ok(())
        } else {
            Err(eyre::eyre!("Agent not found"))
        }
    }

    pub async fn update_performance(
        &self,
        address: Address,
        success: bool,
        response_time_ms: f64,
    ) -> Result<()> {
        if let Some(mut state) = self.states.get_mut(&address) {
            let perf = &mut state.performance_summary;
            
            perf.total_transactions += 1;
            if success {
                perf.successful_transactions += 1;
            }

            // Update average response time
            perf.average_response_time_ms = 
                (perf.average_response_time_ms * (perf.total_transactions - 1) as f64 + response_time_ms) 
                / perf.total_transactions as f64;

            // Update throughput (simplified - in practice would use time windows)
            let elapsed = state.created_at.elapsed().as_secs_f64();
            perf.throughput_per_second = perf.total_transactions as f64 / elapsed;

            // Update reputation based on success rate
            let success_rate = perf.successful_transactions as f64 / perf.total_transactions as f64;
            perf.reputation_score = perf.reputation_score * 0.95 + success_rate * 0.05;

            // Update load
            let load = response_time_ms / 1000.0; // Simplified load calculation
            self.load_balancer.agent_loads.insert(address, load);

            Ok(())
        } else {
            Err(eyre::eyre!("Agent not found"))
        }
    }

    pub async fn get_agent_state(&self, address: Address) -> Option<AgentState> {
        self.states.get(&address).map(|state| state.clone())
    }

    pub async fn get_active_agents(&self) -> Vec<Address> {
        self.states
            .iter()
            .filter(|entry| entry.lifecycle_state == LifecycleState::Active)
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub async fn get_agents_by_capability(&self, capability: &str) -> Vec<Address> {
        // Would integrate with AgentRegistry to filter by capability
        self.get_active_agents().await
    }

    pub async fn select_agent_for_task(
        &self,
        required_capabilities: Vec<String>,
        priority: PriorityLevel,
    ) -> Option<Address> {
        let active_agents = self.get_active_agents().await;
        
        match self.load_balancer.strategy {
            LoadBalancingStrategy::LeastLoaded => {
                self.select_least_loaded_agent(active_agents).await
            }
            LoadBalancingStrategy::PriorityBased => {
                self.select_priority_based_agent(active_agents, priority).await
            }
            _ => active_agents.first().cloned(),
        }
    }

    async fn allocate_initial_resources(
        &self,
        capabilities: &[AgentCapability],
    ) -> Result<ResourceAllocation> {
        let base_cpu = 1.0;
        let base_memory = 256;
        let base_quota = 1000;

        // Allocate more resources for agents with more capabilities
        let cpu_quota = base_cpu * (1.0 + capabilities.len() as f64 * 0.2);
        let memory_limit_mb = base_memory * (1 + capabilities.len());
        let transaction_quota = base_quota * (1 + capabilities.len());

        // Check availability
        let mut available_cpu = self.resource_pool.available_cpu.write().await;
        let mut available_memory = self.resource_pool.available_memory_mb.write().await;
        let mut available_quota = self.resource_pool.available_transaction_quota.write().await;

        if *available_cpu < cpu_quota || *available_memory < memory_limit_mb || *available_quota < transaction_quota {
            return Err(eyre::eyre!("Insufficient resources"));
        }

        *available_cpu -= cpu_quota;
        *available_memory -= memory_limit_mb;
        *available_quota -= transaction_quota;

        Ok(ResourceAllocation {
            cpu_quota,
            memory_limit_mb,
            transaction_quota,
            priority_level: PriorityLevel::Normal,
            current_load: 0.0,
        })
    }

    fn is_valid_transition(&self, from: &LifecycleState, to: &LifecycleState) -> bool {
        match (from, to) {
            (LifecycleState::Initializing, LifecycleState::Active) => true,
            (LifecycleState::Active, LifecycleState::Idle) => true,
            (LifecycleState::Active, LifecycleState::Suspended) => true,
            (LifecycleState::Active, LifecycleState::Terminating) => true,
            (LifecycleState::Idle, LifecycleState::Active) => true,
            (LifecycleState::Idle, LifecycleState::Terminating) => true,
            (LifecycleState::Suspended, LifecycleState::Active) => true,
            (LifecycleState::Suspended, LifecycleState::Terminating) => true,
            (LifecycleState::Terminating, LifecycleState::Terminated) => true,
            _ => false,
        }
    }

    async fn record_state_transition(
        &self,
        address: Address,
        from_state: LifecycleState,
        to_state: LifecycleState,
        reason: String,
    ) {
        let mut history = self.state_history.write().await;
        let transitions = history.entry(address).or_insert_with(|| VecDeque::with_capacity(self.config.state_history_limit));
        
        transitions.push_back(StateTransition {
            from_state,
            to_state,
            timestamp: Instant::now(),
            reason,
        });

        if transitions.len() > self.config.state_history_limit {
            transitions.pop_front();
        }
    }

    async fn handle_agent_activation(&self, address: Address) -> Result<()> {
        // Ensure resources are allocated
        if let Some(state) = self.states.get(&address) {
            let _ = self.event_tx.send(StateEvent::ResourcesAllocated {
                address,
                allocation: state.resource_allocation.clone(),
            }).await;
        }
        Ok(())
    }

    async fn handle_agent_suspension(&self, address: Address) -> Result<()> {
        // Release some resources
        if let Some(mut state) = self.states.get_mut(&address) {
            let released_cpu = state.resource_allocation.cpu_quota * 0.5;
            let released_memory = state.resource_allocation.memory_limit_mb / 2;
            
            state.resource_allocation.cpu_quota *= 0.5;
            state.resource_allocation.memory_limit_mb /= 2;

            // Return resources to pool
            *self.resource_pool.available_cpu.write().await += released_cpu;
            *self.resource_pool.available_memory_mb.write().await += released_memory;
        }
        Ok(())
    }

    async fn handle_agent_termination(&self, address: Address) -> Result<()> {
        // Release all resources
        if let Some(state) = self.states.get(&address) {
            *self.resource_pool.available_cpu.write().await += state.resource_allocation.cpu_quota;
            *self.resource_pool.available_memory_mb.write().await += state.resource_allocation.memory_limit_mb;
            *self.resource_pool.available_transaction_quota.write().await += state.resource_allocation.transaction_quota;
        }

        // Schedule removal
        tokio::spawn({
            let states = self.states.clone();
            let event_tx = self.event_tx.clone();
            let address = address.clone();
            
            async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
                states.remove(&address);
                let _ = event_tx.send(StateEvent::AgentTerminated {
                    address,
                    reason: "Cleanup after termination".to_string(),
                }).await;
            }
        });

        Ok(())
    }

    fn update_health_score(&self, state: &mut AgentState) {
        let error_factor = 1.0 / (1.0 + state.health_status.error_count as f64 * 0.01);
        let failure_factor = 1.0 / (1.0 + state.health_status.consecutive_failures as f64 * 0.1);
        let recovery_factor = 1.0 / (1.0 + state.health_status.recovery_attempts as f64 * 0.05);
        
        state.health_status.health_score = (error_factor * failure_factor * recovery_factor).max(0.0).min(1.0);
    }

    async fn attempt_recovery(&self, address: Address, error_type: &str) -> Result<()> {
        if let Some(mut state) = self.states.get_mut(&address) {
            state.health_status.recovery_attempts += 1;

            // Apply recovery strategy based on error type
            if let Some(strategy) = self.health_monitor.recovery_strategies.get(error_type) {
                strategy.recover(address, &state).await?;
            } else {
                // Default recovery: transition to suspended state
                drop(state); // Release the lock
                self.transition_state(address, LifecycleState::Suspended, "Health check failed".to_string()).await?;
            }
        }
        Ok(())
    }

    async fn select_least_loaded_agent(&self, agents: Vec<Address>) -> Option<Address> {
        let loads = self.load_balancer.agent_loads.clone();
        
        agents.into_iter()
            .filter_map(|addr| {
                loads.get(&addr).map(|load| (addr, *load))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(addr, _)| addr)
    }

    async fn select_priority_based_agent(
        &self,
        agents: Vec<Address>,
        required_priority: PriorityLevel,
    ) -> Option<Address> {
        agents.into_iter()
            .filter(|addr| {
                self.states.get(addr)
                    .map(|state| state.resource_allocation.priority_level >= required_priority)
                    .unwrap_or(false)
            })
            .next()
    }

    fn start_health_monitoring(&self) {
        let states = self.states.clone();
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();
        let health_monitor = self.health_monitor.clone();

        tokio::spawn(async move {
            let mut interval = interval(health_monitor.check_interval);
            
            loop {
                interval.tick().await;
                
                let now = Instant::now();
                let timeout = health_monitor.timeout_duration;
                
                for entry in states.iter() {
                    let address = *entry.key();
                    let state = entry.value();
                    
                    // Check heartbeat timeout
                    if now.duration_since(state.last_heartbeat) > timeout {
                        let _ = event_tx.send(StateEvent::HealthChanged {
                            address,
                            healthy: false,
                        }).await;
                    }
                }
            }
        });
    }

    fn start_resource_rebalancing(&self) {
        let states = self.states.clone();
        let config = self.config.clone();
        let resource_pool = self.resource_pool.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.resource_rebalance_interval_seconds));
            
            loop {
                interval.tick().await;
                
                // Rebalance resources based on actual usage
                // This is a simplified implementation
                let active_agents: Vec<_> = states.iter()
                    .filter(|entry| entry.lifecycle_state == LifecycleState::Active)
                    .map(|entry| (entry.key().clone(), entry.value().clone()))
                    .collect();

                if active_agents.is_empty() {
                    continue;
                }

                let total_available_cpu = *resource_pool.available_cpu.read().await;
                let cpu_per_agent = total_available_cpu / active_agents.len() as f64;

                for (address, mut state) in active_agents {
                    // Adjust allocation based on performance
                    let performance_factor = state.performance_summary.reputation_score;
                    let new_cpu_quota = cpu_per_agent * performance_factor * 2.0;
                    
                    if let Some(mut agent_state) = states.get_mut(&address) {
                        agent_state.resource_allocation.cpu_quota = new_cpu_quota;
                    }
                }
            }
        });
    }

    fn default_recovery_strategies() -> HashMap<String, Box<dyn RecoveryStrategy>> {
        let mut strategies: HashMap<String, Box<dyn RecoveryStrategy>> = HashMap::new();
        
        strategies.insert("timeout".to_string(), Box::new(TimeoutRecovery));
        strategies.insert("memory".to_string(), Box::new(MemoryRecovery));
        strategies.insert("crash".to_string(), Box::new(CrashRecovery));
        
        strategies
    }
}

// Recovery strategies
#[async_trait::async_trait]
trait RecoveryStrategy: Send + Sync {
    async fn recover(&self, address: Address, state: &AgentState) -> Result<()>;
}

struct TimeoutRecovery;

#[async_trait::async_trait]
impl RecoveryStrategy for TimeoutRecovery {
    async fn recover(&self, address: Address, state: &AgentState) -> Result<()> {
        // For timeout errors, just reset the heartbeat
        Ok(())
    }
}

struct MemoryRecovery;

#[async_trait::async_trait]
impl RecoveryStrategy for MemoryRecovery {
    async fn recover(&self, address: Address, state: &AgentState) -> Result<()> {
        // For memory errors, could trigger garbage collection or increase memory limit
        Ok(())
    }
}

struct CrashRecovery;

#[async_trait::async_trait]
impl RecoveryStrategy for CrashRecovery {
    async fn recover(&self, address: Address, state: &AgentState) -> Result<()> {
        // For crashes, might need to restart the agent
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_registration() {
        let config = StateManagerConfig::default();
        let (manager, mut event_rx) = AgentStateManager::new(config);

        let agent_address = Address::from([1u8; 20]);
        let capabilities = vec![AgentCapability::RAGQuery, AgentCapability::MemoryManagement];

        manager.register_agent(agent_address, capabilities).await.unwrap();

        // Check state was created
        let state = manager.get_agent_state(agent_address).await.unwrap();
        assert_eq!(state.lifecycle_state, LifecycleState::Initializing);
        assert!(state.health_status.is_healthy);

        // Check event was sent
        if let Ok(StateEvent::AgentRegistered { address }) = event_rx.try_recv() {
            assert_eq!(address, agent_address);
        } else {
            panic!("Expected AgentRegistered event");
        }
    }

    #[tokio::test]
    async fn test_state_transitions() {
        let config = StateManagerConfig::default();
        let (manager, _) = AgentStateManager::new(config);

        let agent_address = Address::from([1u8; 20]);
        manager.register_agent(agent_address, vec![]).await.unwrap();

        // Valid transition
        manager.transition_state(
            agent_address,
            LifecycleState::Active,
            "Initialization complete".to_string(),
        ).await.unwrap();

        let state = manager.get_agent_state(agent_address).await.unwrap();
        assert_eq!(state.lifecycle_state, LifecycleState::Active);

        // Invalid transition
        let result = manager.transition_state(
            agent_address,
            LifecycleState::Initializing,
            "Invalid".to_string(),
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_health_monitoring() {
        let config = StateManagerConfig::default();
        let (manager, _) = AgentStateManager::new(config);

        let agent_address = Address::from([1u8; 20]);
        manager.register_agent(agent_address, vec![]).await.unwrap();

        // Report errors
        for i in 0..5 {
            manager.report_error(
                agent_address,
                "test_error".to_string(),
                format!("Error {}", i),
                HashMap::new(),
            ).await.unwrap();
        }

        let state = manager.get_agent_state(agent_address).await.unwrap();
        assert!(!state.health_status.is_healthy);
        assert_eq!(state.health_status.consecutive_failures, 5);
    }
}