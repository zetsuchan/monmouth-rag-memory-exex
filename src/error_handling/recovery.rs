use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn, error};

use super::{ErrorHandlingConfig, ErrorContext, ErrorSeverity, CircuitBreaker, RetryPolicy};
use crate::shared::communication::CrossExExCoordinator;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    pub base_config: ErrorHandlingConfig,
    pub auto_recovery_enabled: bool,
    pub recovery_timeout: Duration,
    pub max_recovery_attempts: u32,
    pub health_check_interval: Duration,
    pub graceful_shutdown_timeout: Duration,
    pub component_dependency_map: HashMap<String, Vec<String>>,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            base_config: ErrorHandlingConfig::default(),
            auto_recovery_enabled: true,
            recovery_timeout: Duration::from_secs(300),
            max_recovery_attempts: 3,
            health_check_interval: Duration::from_secs(30),
            graceful_shutdown_timeout: Duration::from_secs(60),
            component_dependency_map: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentStatus {
    Healthy,
    Degraded { issues: Vec<String> },
    Failing { error_count: u32, last_error: String },
    Recovering { attempt: u32, started_at: u64 },
    Failed { final_error: String, failed_at: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryStrategy {
    Restart { graceful: bool },
    Failover { target_component: String },
    Rollback { to_checkpoint: String },
    Degrade { reduced_functionality: Vec<String> },
    Isolate { quarantine_duration: Duration },
}

#[derive(Debug, Clone)]
pub struct ComponentInfo {
    pub component_id: String,
    pub status: ComponentStatus,
    pub last_health_check: Instant,
    pub dependencies: Vec<String>,
    pub recovery_attempts: u32,
    pub total_failures: u64,
    pub last_recovery_attempt: Option<Instant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMetrics {
    pub total_recovery_attempts: u64,
    pub successful_recoveries: u64,
    pub failed_recoveries: u64,
    pub average_recovery_time_ms: f64,
    pub components_recovered: u64,
    pub graceful_degradations: u64,
    pub system_restarts: u64,
}

impl Default for RecoveryMetrics {
    fn default() -> Self {
        Self {
            total_recovery_attempts: 0,
            successful_recoveries: 0,
            failed_recoveries: 0,
            average_recovery_time_ms: 0.0,
            components_recovered: 0,
            graceful_degradations: 0,
            system_restarts: 0,
        }
    }
}

#[derive(Debug)]
pub struct RecoveryCoordinator {
    config: Arc<RwLock<RecoveryConfig>>,
    components: Arc<DashMap<String, ComponentInfo>>,
    circuit_breakers: Arc<DashMap<String, CircuitBreaker>>,
    retry_policies: Arc<DashMap<String, RetryPolicy>>,
    recovery_queue: Arc<RwLock<Vec<RecoveryRequest>>>,
    metrics: Arc<RwLock<RecoveryMetrics>>,
    recovery_handlers: Arc<DashMap<String, Box<dyn RecoveryHandler>>>,
    health_monitor: Arc<HealthMonitor>,
}

#[derive(Debug, Clone)]
pub struct RecoveryRequest {
    pub request_id: B256,
    pub component_id: String,
    pub error_context: ErrorContext,
    pub strategy: RecoveryStrategy,
    pub priority: RecoveryPriority,
    pub created_at: Instant,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq)]
pub enum RecoveryPriority {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

#[derive(Debug)]
struct HealthMonitor {
    monitoring_active: Arc<RwLock<bool>>,
    check_interval: Duration,
    health_check_sender: mpsc::Sender<String>,
}

#[async_trait]
pub trait RecoveryHandler: Send + Sync {
    async fn can_handle(&self, component_id: &str, error: &ErrorContext) -> bool;
    async fn recover(&self, component_id: &str, strategy: &RecoveryStrategy) -> Result<RecoveryResult>;
    async fn health_check(&self, component_id: &str) -> Result<bool>;
    async fn get_dependencies(&self, component_id: &str) -> Vec<String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    pub success: bool,
    pub recovery_time: Duration,
    pub strategy_used: RecoveryStrategy,
    pub components_affected: Vec<String>,
    pub error_message: Option<String>,
}

impl RecoveryCoordinator {
    pub fn new(config: RecoveryConfig) -> Self {
        let (health_check_sender, _health_check_receiver) = mpsc::channel(1000);
        
        let health_monitor = Arc::new(HealthMonitor {
            monitoring_active: Arc::new(RwLock::new(false)),
            check_interval: config.health_check_interval,
            health_check_sender,
        });

        Self {
            config: Arc::new(RwLock::new(config)),
            components: Arc::new(DashMap::new()),
            circuit_breakers: Arc::new(DashMap::new()),
            retry_policies: Arc::new(DashMap::new()),
            recovery_queue: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(RwLock::new(RecoveryMetrics::default())),
            recovery_handlers: Arc::new(DashMap::new()),
            health_monitor,
        }
    }

    pub async fn register_component(&self, component_info: ComponentInfo) -> Result<()> {
        info!("Registering component: {}", component_info.component_id);
        self.components.insert(component_info.component_id.clone(), component_info);
        Ok(())
    }

    pub async fn register_recovery_handler(
        &self,
        component_type: String,
        handler: Box<dyn RecoveryHandler>,
    ) -> Result<()> {
        info!("Registering recovery handler for: {}", component_type);
        self.recovery_handlers.insert(component_type, handler);
        Ok(())
    }

    pub async fn handle_error(&self, error: ErrorContext) -> Result<()> {
        info!("Handling error for component: {}", error.component);
        
        self.update_component_status(&error.component, &error).await?;
        
        if self.should_trigger_recovery(&error).await {
            let strategy = self.determine_recovery_strategy(&error).await?;
            let recovery_request = RecoveryRequest {
                request_id: B256::random(),
                component_id: error.component.clone(),
                error_context: error,
                strategy,
                priority: self.determine_priority(&error.component).await,
                created_at: Instant::now(),
                timeout: self.config.read().await.recovery_timeout,
            };
            
            self.enqueue_recovery(recovery_request).await?;
        }

        Ok(())
    }

    async fn update_component_status(&self, component_id: &str, error: &ErrorContext) -> Result<()> {
        if let Some(mut component) = self.components.get_mut(component_id) {
            match error.severity {
                ErrorSeverity::Critical => {
                    component.status = ComponentStatus::Failed {
                        final_error: error.message.clone(),
                        failed_at: error.timestamp,
                    };
                }
                ErrorSeverity::High => {
                    if let ComponentStatus::Failing { error_count, .. } = component.status {
                        component.status = ComponentStatus::Failing {
                            error_count: error_count + 1,
                            last_error: error.message.clone(),
                        };
                    } else {
                        component.status = ComponentStatus::Failing {
                            error_count: 1,
                            last_error: error.message.clone(),
                        };
                    }
                }
                ErrorSeverity::Medium => {
                    let mut issues = if let ComponentStatus::Degraded { issues } = &component.status {
                        issues.clone()
                    } else {
                        Vec::new()
                    };
                    issues.push(error.message.clone());
                    component.status = ComponentStatus::Degraded { issues };
                }
                ErrorSeverity::Low => {
                    // Keep existing status for low severity errors
                }
            }
            
            component.total_failures += 1;
        }

        Ok(())
    }

    async fn should_trigger_recovery(&self, error: &ErrorContext) -> bool {
        let config = self.config.read().await;
        
        if !config.auto_recovery_enabled {
            return false;
        }

        match error.severity {
            ErrorSeverity::Critical | ErrorSeverity::High => true,
            ErrorSeverity::Medium => {
                if let Some(component) = self.components.get(&error.component) {
                    matches!(component.status, ComponentStatus::Degraded { .. })
                } else {
                    false
                }
            }
            ErrorSeverity::Low => false,
        }
    }

    async fn determine_recovery_strategy(&self, error: &ErrorContext) -> Result<RecoveryStrategy> {
        match error.severity {
            ErrorSeverity::Critical => {
                if error.component == "core" {
                    Ok(RecoveryStrategy::Restart { graceful: false })
                } else {
                    Ok(RecoveryStrategy::Isolate {
                        quarantine_duration: Duration::from_secs(300),
                    })
                }
            }
            ErrorSeverity::High => {
                match error.category {
                    super::ErrorCategory::Network => {
                        Ok(RecoveryStrategy::Failover {
                            target_component: format!("{}_backup", error.component),
                        })
                    }
                    super::ErrorCategory::Memory => {
                        Ok(RecoveryStrategy::Restart { graceful: true })
                    }
                    _ => {
                        Ok(RecoveryStrategy::Degrade {
                            reduced_functionality: vec!["non_critical_features".to_string()],
                        })
                    }
                }
            }
            ErrorSeverity::Medium => {
                Ok(RecoveryStrategy::Degrade {
                    reduced_functionality: vec!["optional_features".to_string()],
                })
            }
            ErrorSeverity::Low => {
                Ok(RecoveryStrategy::Restart { graceful: true })
            }
        }
    }

    async fn determine_priority(&self, component_id: &str) -> RecoveryPriority {
        match component_id {
            id if id.contains("core") => RecoveryPriority::Critical,
            id if id.contains("memory") || id.contains("storage") => RecoveryPriority::High,
            id if id.contains("network") || id.contains("api") => RecoveryPriority::Medium,
            _ => RecoveryPriority::Low,
        }
    }

    async fn enqueue_recovery(&self, request: RecoveryRequest) -> Result<()> {
        let mut queue = self.recovery_queue.write().await;
        queue.push(request);
        queue.sort_by_key(|r| r.priority.clone());
        
        info!("Enqueued recovery request, queue size: {}", queue.len());
        Ok(())
    }

    pub async fn start_recovery_processing(&self) -> Result<()> {
        info!("Starting recovery processing");
        
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.recovery_processing_loop().await;
        });

        Ok(())
    }

    async fn recovery_processing_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        
        loop {
            interval.tick().await;
            
            if let Some(request) = self.dequeue_recovery().await {
                if let Err(e) = self.process_recovery_request(request).await {
                    error!("Failed to process recovery request: {}", e);
                }
            }
        }
    }

    async fn dequeue_recovery(&self) -> Option<RecoveryRequest> {
        let mut queue = self.recovery_queue.write().await;
        if !queue.is_empty() {
            Some(queue.remove(0))
        } else {
            None
        }
    }

    async fn process_recovery_request(&self, request: RecoveryRequest) -> Result<()> {
        info!("Processing recovery request for component: {}", request.component_id);
        
        let start_time = Instant::now();
        
        if let Some(mut component) = self.components.get_mut(&request.component_id) {
            component.status = ComponentStatus::Recovering {
                attempt: component.recovery_attempts + 1,
                started_at: chrono::Utc::now().timestamp() as u64,
            };
            component.recovery_attempts += 1;
            component.last_recovery_attempt = Some(start_time);
        }

        let result = self.execute_recovery(&request).await;
        let recovery_time = start_time.elapsed();

        match result {
            Ok(recovery_result) => {
                self.handle_successful_recovery(&request, recovery_result, recovery_time).await?;
            }
            Err(e) => {
                self.handle_failed_recovery(&request, e, recovery_time).await?;
            }
        }

        Ok(())
    }

    async fn execute_recovery(&self, request: &RecoveryRequest) -> Result<RecoveryResult> {
        for handler in self.recovery_handlers.iter() {
            if handler.can_handle(&request.component_id, &request.error_context).await {
                debug!("Using recovery handler: {}", handler.key());
                return handler.recover(&request.component_id, &request.strategy).await;
            }
        }

        self.default_recovery(&request.component_id, &request.strategy).await
    }

    async fn default_recovery(&self, component_id: &str, strategy: &RecoveryStrategy) -> Result<RecoveryResult> {
        let start_time = Instant::now();
        
        match strategy {
            RecoveryStrategy::Restart { graceful } => {
                info!("Performing {} restart for component: {}", 
                    if *graceful { "graceful" } else { "forced" }, component_id);
                
                if *graceful {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                } else {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                
                Ok(RecoveryResult {
                    success: true,
                    recovery_time: start_time.elapsed(),
                    strategy_used: strategy.clone(),
                    components_affected: vec![component_id.to_string()],
                    error_message: None,
                })
            }
            RecoveryStrategy::Failover { target_component } => {
                info!("Performing failover from {} to {}", component_id, target_component);
                
                tokio::time::sleep(Duration::from_millis(200)).await;
                
                Ok(RecoveryResult {
                    success: true,
                    recovery_time: start_time.elapsed(),
                    strategy_used: strategy.clone(),
                    components_affected: vec![component_id.to_string(), target_component.clone()],
                    error_message: None,
                })
            }
            RecoveryStrategy::Degrade { reduced_functionality } => {
                info!("Degrading component {} by disabling: {:?}", component_id, reduced_functionality);
                
                self.metrics.write().await.graceful_degradations += 1;
                
                Ok(RecoveryResult {
                    success: true,
                    recovery_time: start_time.elapsed(),
                    strategy_used: strategy.clone(),
                    components_affected: vec![component_id.to_string()],
                    error_message: None,
                })
            }
            RecoveryStrategy::Isolate { quarantine_duration } => {
                info!("Isolating component {} for {:?}", component_id, quarantine_duration);
                
                Ok(RecoveryResult {
                    success: true,
                    recovery_time: start_time.elapsed(),
                    strategy_used: strategy.clone(),
                    components_affected: vec![component_id.to_string()],
                    error_message: None,
                })
            }
            RecoveryStrategy::Rollback { to_checkpoint } => {
                info!("Rolling back component {} to checkpoint: {}", component_id, to_checkpoint);
                
                tokio::time::sleep(Duration::from_millis(300)).await;
                
                Ok(RecoveryResult {
                    success: true,
                    recovery_time: start_time.elapsed(),
                    strategy_used: strategy.clone(),
                    components_affected: vec![component_id.to_string()],
                    error_message: None,
                })
            }
        }
    }

    async fn handle_successful_recovery(
        &self,
        request: &RecoveryRequest,
        result: RecoveryResult,
        recovery_time: Duration,
    ) -> Result<()> {
        info!("Recovery successful for component: {} in {:?}", 
            request.component_id, recovery_time);
        
        if let Some(mut component) = self.components.get_mut(&request.component_id) {
            component.status = ComponentStatus::Healthy;
            component.recovery_attempts = 0;
        }

        let mut metrics = self.metrics.write().await;
        metrics.total_recovery_attempts += 1;
        metrics.successful_recoveries += 1;
        metrics.components_recovered += 1;
        
        let alpha = 0.1;
        metrics.average_recovery_time_ms = alpha * recovery_time.as_millis() as f64 + 
            (1.0 - alpha) * metrics.average_recovery_time_ms;

        Ok(())
    }

    async fn handle_failed_recovery(
        &self,
        request: &RecoveryRequest,
        error: eyre::Report,
        recovery_time: Duration,
    ) -> Result<()> {
        error!("Recovery failed for component: {} after {:?}: {}", 
            request.component_id, recovery_time, error);
        
        let config = self.config.read().await;
        
        if let Some(mut component) = self.components.get_mut(&request.component_id) {
            if component.recovery_attempts >= config.max_recovery_attempts {
                component.status = ComponentStatus::Failed {
                    final_error: format!("Max recovery attempts exceeded: {}", error),
                    failed_at: chrono::Utc::now().timestamp() as u64,
                };
                warn!("Component {} marked as permanently failed", request.component_id);
            } else {
                let next_request = RecoveryRequest {
                    request_id: B256::random(),
                    component_id: request.component_id.clone(),
                    error_context: request.error_context.clone(),
                    strategy: self.escalate_recovery_strategy(&request.strategy).await,
                    priority: RecoveryPriority::High,
                    created_at: Instant::now(),
                    timeout: request.timeout,
                };
                
                self.enqueue_recovery(next_request).await?;
            }
        }

        let mut metrics = self.metrics.write().await;
        metrics.total_recovery_attempts += 1;
        metrics.failed_recoveries += 1;

        Ok(())
    }

    async fn escalate_recovery_strategy(&self, current_strategy: &RecoveryStrategy) -> RecoveryStrategy {
        match current_strategy {
            RecoveryStrategy::Degrade { .. } => RecoveryStrategy::Restart { graceful: true },
            RecoveryStrategy::Restart { graceful: true } => RecoveryStrategy::Restart { graceful: false },
            RecoveryStrategy::Restart { graceful: false } => RecoveryStrategy::Isolate {
                quarantine_duration: Duration::from_secs(600),
            },
            _ => current_strategy.clone(),
        }
    }

    pub async fn start_health_monitoring(&self) -> Result<()> {
        if *self.health_monitor.monitoring_active.read().await {
            return Ok(());
        }

        *self.health_monitor.monitoring_active.write().await = true;
        
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator.health_monitoring_loop().await;
        });

        info!("Started health monitoring");
        Ok(())
    }

    async fn health_monitoring_loop(&self) {
        let mut interval = tokio::time::interval(self.health_monitor.check_interval);
        
        while *self.health_monitor.monitoring_active.read().await {
            interval.tick().await;
            
            if let Err(e) = self.perform_health_checks().await {
                error!("Health check failed: {}", e);
            }
        }
    }

    async fn perform_health_checks(&self) -> Result<()> {
        let component_ids: Vec<String> = self.components.iter()
            .map(|entry| entry.key().clone())
            .collect();

        for component_id in component_ids {
            if let Err(e) = self.check_component_health(&component_id).await {
                warn!("Health check failed for component {}: {}", component_id, e);
            }
        }

        Ok(())
    }

    async fn check_component_health(&self, component_id: &str) -> Result<()> {
        debug!("Checking health of component: {}", component_id);
        
        let is_healthy = if let Some(handler) = self.get_recovery_handler(component_id).await {
            handler.health_check(component_id).await?
        } else {
            self.default_health_check(component_id).await?
        };

        if let Some(mut component) = self.components.get_mut(component_id) {
            component.last_health_check = Instant::now();
            
            if !is_healthy && matches!(component.status, ComponentStatus::Healthy) {
                component.status = ComponentStatus::Degraded {
                    issues: vec!["Health check failed".to_string()],
                };
                
                let error_context = super::ErrorContext {
                    error_id: uuid::Uuid::new_v4().to_string(),
                    severity: ErrorSeverity::Medium,
                    category: super::ErrorCategory::Unknown,
                    message: "Health check failed".to_string(),
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    component: component_id.to_string(),
                    metadata: serde_json::json!({}),
                };
                
                drop(component);
                self.handle_error(error_context).await?;
            }
        }

        Ok(())
    }

    async fn get_recovery_handler(&self, component_id: &str) -> Option<&Box<dyn RecoveryHandler>> {
        for handler in self.recovery_handlers.iter() {
            if component_id.starts_with(handler.key()) {
                return Some(handler.value());
            }
        }
        None
    }

    async fn default_health_check(&self, _component_id: &str) -> Result<bool> {
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok(rand::random::<f64>() > 0.1)
    }

    pub async fn get_component_status(&self, component_id: &str) -> Option<ComponentStatus> {
        self.components.get(component_id).map(|c| c.status.clone())
    }

    pub async fn get_all_component_status(&self) -> HashMap<String, ComponentStatus> {
        self.components.iter()
            .map(|entry| (entry.key().clone(), entry.value().status.clone()))
            .collect()
    }

    pub async fn get_recovery_metrics(&self) -> RecoveryMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn force_recovery(&self, component_id: String, strategy: RecoveryStrategy) -> Result<()> {
        let error_context = super::ErrorContext {
            error_id: uuid::Uuid::new_v4().to_string(),
            severity: ErrorSeverity::High,
            category: super::ErrorCategory::Unknown,
            message: "Manual recovery triggered".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            component: component_id.clone(),
            metadata: serde_json::json!({}),
        };

        let recovery_request = RecoveryRequest {
            request_id: B256::random(),
            component_id,
            error_context,
            strategy,
            priority: RecoveryPriority::Critical,
            created_at: Instant::now(),
            timeout: self.config.read().await.recovery_timeout,
        };

        self.enqueue_recovery(recovery_request).await?;
        info!("Manual recovery triggered");
        Ok(())
    }
}

impl Clone for RecoveryCoordinator {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            components: self.components.clone(),
            circuit_breakers: self.circuit_breakers.clone(),
            retry_policies: self.retry_policies.clone(),
            recovery_queue: self.recovery_queue.clone(),
            metrics: self.metrics.clone(),
            recovery_handlers: self.recovery_handlers.clone(),
            health_monitor: self.health_monitor.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_recovery_coordinator_creation() {
        let config = RecoveryConfig::default();
        let coordinator = RecoveryCoordinator::new(config);
        
        let metrics = coordinator.get_recovery_metrics().await;
        assert_eq!(metrics.total_recovery_attempts, 0);
    }

    #[tokio::test]
    async fn test_component_registration() {
        let config = RecoveryConfig::default();
        let coordinator = RecoveryCoordinator::new(config);
        
        let component_info = ComponentInfo {
            component_id: "test_component".to_string(),
            status: ComponentStatus::Healthy,
            last_health_check: Instant::now(),
            dependencies: vec![],
            recovery_attempts: 0,
            total_failures: 0,
            last_recovery_attempt: None,
        };
        
        coordinator.register_component(component_info).await.unwrap();
        
        let status = coordinator.get_component_status("test_component").await;
        assert!(matches!(status, Some(ComponentStatus::Healthy)));
    }

    #[tokio::test]
    async fn test_error_handling() {
        let config = RecoveryConfig::default();
        let coordinator = RecoveryCoordinator::new(config);
        
        let component_info = ComponentInfo {
            component_id: "test_component".to_string(),
            status: ComponentStatus::Healthy,
            last_health_check: Instant::now(),
            dependencies: vec![],
            recovery_attempts: 0,
            total_failures: 0,
            last_recovery_attempt: None,
        };
        
        coordinator.register_component(component_info).await.unwrap();
        
        let error_context = super::ErrorContext {
            error_id: uuid::Uuid::new_v4().to_string(),
            severity: ErrorSeverity::High,
            category: super::ErrorCategory::Network,
            message: "Connection failed".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            component: "test_component".to_string(),
            metadata: serde_json::json!({}),
        };
        
        coordinator.handle_error(error_context).await.unwrap();
        
        let status = coordinator.get_component_status("test_component").await;
        assert!(matches!(status, Some(ComponentStatus::Failing { .. })));
    }
}