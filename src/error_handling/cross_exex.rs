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

use super::{
    ErrorHandlingConfig, ErrorContext, ErrorSeverity, ErrorCategory, 
    ErrorHandler, ErrorHandlingResponse, ErrorHandlingMetrics
};
use crate::shared::communication::{CrossExExMessage, CrossExExCoordinator};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossExExErrorConfig {
    pub base_config: ErrorHandlingConfig,
    pub exex_health_check_interval: Duration,
    pub max_failed_health_checks: u32,
    pub failover_timeout: Duration,
    pub enable_automatic_failover: bool,
    pub enable_load_redistribution: bool,
    pub quarantine_duration: Duration,
}

impl Default for CrossExExErrorConfig {
    fn default() -> Self {
        Self {
            base_config: ErrorHandlingConfig::default(),
            exex_health_check_interval: Duration::from_secs(10),
            max_failed_health_checks: 3,
            failover_timeout: Duration::from_secs(30),
            enable_automatic_failover: true,
            enable_load_redistribution: true,
            quarantine_duration: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExExStatus {
    Healthy,
    Degraded { issues: Vec<String> },
    Unhealthy { error_count: u32 },
    Quarantined { until: Instant },
    Failed { last_error: String },
}

#[derive(Debug, Clone)]
pub struct ExExHealthInfo {
    pub exex_id: String,
    pub status: ExExStatus,
    pub last_successful_communication: Option<Instant>,
    pub consecutive_failures: u32,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub average_response_time: Duration,
    pub last_health_check: Instant,
}

#[derive(Debug)]
pub struct CrossExExErrorHandler {
    config: Arc<RwLock<CrossExExErrorConfig>>,
    exex_health: Arc<DashMap<String, ExExHealthInfo>>,
    error_patterns: Arc<RwLock<HashMap<String, ErrorCategory>>>,
    metrics: Arc<RwLock<ErrorHandlingMetrics>>,
    failover_coordinator: Arc<FailoverCoordinator>,
    health_monitor: Arc<HealthMonitor>,
}

#[derive(Debug)]
struct FailoverCoordinator {
    primary_exex: Arc<RwLock<Option<String>>>,
    backup_exexs: Arc<RwLock<Vec<String>>>,
    current_loads: Arc<DashMap<String, f64>>,
    failover_history: Arc<RwLock<Vec<FailoverEvent>>>,
}

#[derive(Debug, Clone)]
struct FailoverEvent {
    timestamp: Instant,
    from_exex: String,
    to_exex: String,
    reason: String,
    success: bool,
}

#[derive(Debug)]
struct HealthMonitor {
    check_interval: Duration,
    monitoring_active: Arc<RwLock<bool>>,
    health_check_sender: mpsc::Sender<String>,
}

impl CrossExExErrorHandler {
    pub fn new(config: CrossExExErrorConfig) -> Self {
        let (health_check_sender, _health_check_receiver) = mpsc::channel(1000);
        
        let failover_coordinator = Arc::new(FailoverCoordinator {
            primary_exex: Arc::new(RwLock::new(None)),
            backup_exexs: Arc::new(RwLock::new(Vec::new())),
            current_loads: Arc::new(DashMap::new()),
            failover_history: Arc::new(RwLock::new(Vec::new())),
        });

        let health_monitor = Arc::new(HealthMonitor {
            check_interval: config.exex_health_check_interval,
            monitoring_active: Arc::new(RwLock::new(false)),
            health_check_sender,
        });

        Self {
            config: Arc::new(RwLock::new(config)),
            exex_health: Arc::new(DashMap::new()),
            error_patterns: Arc::new(RwLock::new(Self::initialize_error_patterns())),
            metrics: Arc::new(RwLock::new(ErrorHandlingMetrics::default())),
            failover_coordinator,
            health_monitor,
        }
    }

    fn initialize_error_patterns() -> HashMap<String, ErrorCategory> {
        let mut patterns = HashMap::new();
        patterns.insert("connection refused".to_string(), ErrorCategory::Network);
        patterns.insert("timeout".to_string(), ErrorCategory::Network);
        patterns.insert("dns resolution failed".to_string(), ErrorCategory::Network);
        patterns.insert("database".to_string(), ErrorCategory::Database);
        patterns.insert("out of memory".to_string(), ErrorCategory::Memory);
        patterns.insert("stack overflow".to_string(), ErrorCategory::Memory);
        patterns.insert("invalid configuration".to_string(), ErrorCategory::Configuration);
        patterns.insert("external service".to_string(), ErrorCategory::External);
        patterns
    }

    pub async fn register_exex(&self, exex_id: String) -> Result<()> {
        let health_info = ExExHealthInfo {
            exex_id: exex_id.clone(),
            status: ExExStatus::Healthy,
            last_successful_communication: None,
            consecutive_failures: 0,
            total_requests: 0,
            successful_requests: 0,
            average_response_time: Duration::ZERO,
            last_health_check: Instant::now(),
        };

        self.exex_health.insert(exex_id.clone(), health_info);
        self.failover_coordinator.current_loads.insert(exex_id.clone(), 0.0);
        
        let backup_exexs = &mut *self.failover_coordinator.backup_exexs.write().await;
        if !backup_exexs.contains(&exex_id) {
            backup_exexs.push(exex_id.clone());
        }

        if self.failover_coordinator.primary_exex.read().await.is_none() {
            *self.failover_coordinator.primary_exex.write().await = Some(exex_id.clone());
            info!("Set {} as primary ExEx", exex_id);
        }

        info!("Registered ExEx: {}", exex_id);
        Ok(())
    }

    pub async fn record_communication_result(
        &self,
        exex_id: &str,
        success: bool,
        response_time: Duration,
        error_msg: Option<String>,
    ) -> Result<()> {
        if let Some(mut health_info) = self.exex_health.get_mut(exex_id) {
            health_info.total_requests += 1;
            
            if success {
                health_info.successful_requests += 1;
                health_info.last_successful_communication = Some(Instant::now());
                health_info.consecutive_failures = 0;
                
                let alpha = 0.1;
                health_info.average_response_time = Duration::from_millis(
                    (alpha * response_time.as_millis() as f64 + 
                     (1.0 - alpha) * health_info.average_response_time.as_millis() as f64) as u64
                );

                if !matches!(health_info.status, ExExStatus::Healthy) {
                    health_info.status = ExExStatus::Healthy;
                    info!("ExEx {} recovered to healthy status", exex_id);
                }
            } else {
                health_info.consecutive_failures += 1;
                
                let config = self.config.read().await;
                
                if health_info.consecutive_failures >= config.max_failed_health_checks {
                    health_info.status = ExExStatus::Failed {
                        last_error: error_msg.unwrap_or_else(|| "Unknown error".to_string()),
                    };
                    
                    warn!("ExEx {} marked as failed after {} consecutive failures", 
                        exex_id, health_info.consecutive_failures);
                    
                    if config.enable_automatic_failover {
                        self.initiate_failover(exex_id).await?;
                    }
                } else {
                    health_info.status = ExExStatus::Degraded {
                        issues: vec![error_msg.unwrap_or_else(|| "Communication failure".to_string())],
                    };
                }
            }
        }

        Ok(())
    }

    async fn initiate_failover(&self, failed_exex: &str) -> Result<()> {
        info!("Initiating failover from ExEx: {}", failed_exex);
        
        let backup_exex = self.select_best_backup_exex(failed_exex).await?;
        
        let failover_event = FailoverEvent {
            timestamp: Instant::now(),
            from_exex: failed_exex.to_string(),
            to_exex: backup_exex.clone(),
            reason: "Health check failures".to_string(),
            success: false,
        };

        if self.execute_failover(failed_exex, &backup_exex).await.is_ok() {
            let mut event = failover_event;
            event.success = true;
            
            self.failover_coordinator.failover_history.write().await.push(event);
            
            if self.failover_coordinator.primary_exex.read().await.as_ref() == Some(&failed_exex.to_string()) {
                *self.failover_coordinator.primary_exex.write().await = Some(backup_exex.clone());
            }
            
            info!("Failover completed successfully from {} to {}", failed_exex, backup_exex);
        } else {
            self.failover_coordinator.failover_history.write().await.push(failover_event);
            error!("Failover failed from {} to {}", failed_exex, backup_exex);
        }

        Ok(())
    }

    async fn select_best_backup_exex(&self, failed_exex: &str) -> Result<String> {
        let backup_exexs = self.failover_coordinator.backup_exexs.read().await;
        
        let mut best_candidate = None;
        let mut best_score = f64::NEG_INFINITY;

        for exex_id in backup_exexs.iter() {
            if exex_id == failed_exex {
                continue;
            }

            if let Some(health_info) = self.exex_health.get(exex_id) {
                if matches!(health_info.status, ExExStatus::Healthy) {
                    let load = self.failover_coordinator.current_loads.get(exex_id)
                        .map(|l| *l)
                        .unwrap_or(0.0);
                    
                    let success_rate = if health_info.total_requests > 0 {
                        health_info.successful_requests as f64 / health_info.total_requests as f64
                    } else {
                        1.0
                    };

                    let response_time_score = 1.0 / (1.0 + health_info.average_response_time.as_millis() as f64);
                    let load_score = 1.0 / (1.0 + load);
                    
                    let score = success_rate * 0.4 + response_time_score * 0.3 + load_score * 0.3;
                    
                    if score > best_score {
                        best_score = score;
                        best_candidate = Some(exex_id.clone());
                    }
                }
            }
        }

        best_candidate.ok_or_else(|| eyre::eyre!("No healthy backup ExEx available"))
    }

    async fn execute_failover(&self, from_exex: &str, to_exex: &str) -> Result<()> {
        debug!("Executing failover from {} to {}", from_exex, to_exex);
        
        if let Some(mut health_info) = self.exex_health.get_mut(from_exex) {
            health_info.status = ExExStatus::Quarantined {
                until: Instant::now() + self.config.read().await.quarantine_duration,
            };
        }

        self.redistribute_load(from_exex, to_exex).await?;
        
        Ok(())
    }

    async fn redistribute_load(&self, from_exex: &str, to_exex: &str) -> Result<()> {
        let config = self.config.read().await;
        
        if !config.enable_load_redistribution {
            return Ok();
        }

        let from_load = self.failover_coordinator.current_loads.get(from_exex)
            .map(|l| *l)
            .unwrap_or(0.0);

        if let Some(mut to_load) = self.failover_coordinator.current_loads.get_mut(to_exex) {
            *to_load += from_load;
        }

        self.failover_coordinator.current_loads.insert(from_exex.to_string(), 0.0);
        
        info!("Redistributed load from {} to {} (load: {:.2})", from_exex, to_exex, from_load);
        Ok(())
    }

    pub async fn start_health_monitoring(&self) -> Result<()> {
        if *self.health_monitor.monitoring_active.read().await {
            return Ok(());
        }

        *self.health_monitor.monitoring_active.write().await = true;
        
        let handler = self.clone();
        let check_interval = self.health_monitor.check_interval;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(check_interval);
            
            while *handler.health_monitor.monitoring_active.read().await {
                interval.tick().await;
                
                if let Err(e) = handler.perform_health_checks().await {
                    error!("Health check failed: {}", e);
                }
            }
        });

        info!("Started health monitoring with interval: {:?}", check_interval);
        Ok(())
    }

    async fn perform_health_checks(&self) -> Result<()> {
        let exex_ids: Vec<String> = self.exex_health.iter()
            .map(|entry| entry.key().clone())
            .collect();

        for exex_id in exex_ids {
            if let Err(e) = self.check_exex_health(&exex_id).await {
                warn!("Health check failed for ExEx {}: {}", exex_id, e);
            }
        }

        self.check_quarantined_exexs().await?;
        Ok(())
    }

    async fn check_exex_health(&self, exex_id: &str) -> Result<()> {
        debug!("Performing health check for ExEx: {}", exex_id);
        
        let start_time = Instant::now();
        let health_check_result = self.send_health_check_ping(exex_id).await;
        let response_time = start_time.elapsed();

        match health_check_result {
            Ok(()) => {
                self.record_communication_result(exex_id, true, response_time, None).await?;
            }
            Err(e) => {
                self.record_communication_result(
                    exex_id, 
                    false, 
                    response_time, 
                    Some(e.to_string())
                ).await?;
            }
        }

        if let Some(mut health_info) = self.exex_health.get_mut(exex_id) {
            health_info.last_health_check = Instant::now();
        }

        Ok(())
    }

    async fn send_health_check_ping(&self, _exex_id: &str) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    async fn check_quarantined_exexs(&self) -> Result<()> {
        let now = Instant::now();
        let mut recovered_exexs = Vec::new();

        for mut entry in self.exex_health.iter_mut() {
            if let ExExStatus::Quarantined { until } = entry.status {
                if now >= until {
                    entry.status = ExExStatus::Healthy;
                    entry.consecutive_failures = 0;
                    recovered_exexs.push(entry.exex_id.clone());
                }
            }
        }

        for exex_id in recovered_exexs {
            info!("ExEx {} recovered from quarantine", exex_id);
        }

        Ok(())
    }

    pub async fn get_exex_status(&self, exex_id: &str) -> Option<ExExStatus> {
        self.exex_health.get(exex_id).map(|info| info.status.clone())
    }

    pub async fn get_all_exex_health(&self) -> HashMap<String, ExExHealthInfo> {
        self.exex_health.iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    pub async fn get_failover_history(&self) -> Vec<FailoverEvent> {
        self.failover_coordinator.failover_history.read().await.clone()
    }
}

impl Clone for CrossExExErrorHandler {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            exex_health: self.exex_health.clone(),
            error_patterns: self.error_patterns.clone(),
            metrics: self.metrics.clone(),
            failover_coordinator: self.failover_coordinator.clone(),
            health_monitor: self.health_monitor.clone(),
        }
    }
}

#[async_trait]
impl ErrorHandler for CrossExExErrorHandler {
    async fn handle_error(&self, error: ErrorContext) -> Result<ErrorHandlingResponse> {
        let mut metrics = self.metrics.write().await;
        metrics.total_errors += 1;
        
        let severity_key = format!("{:?}", error.severity);
        *metrics.errors_by_severity.entry(severity_key).or_insert(0) += 1;
        
        let category_key = format!("{:?}", error.category);
        *metrics.errors_by_category.entry(category_key).or_insert(0) += 1;

        match error.severity {
            ErrorSeverity::Critical => {
                Ok(ErrorHandlingResponse::Fatal {
                    error_id: error.error_id,
                    shutdown_required: error.component == "core",
                })
            }
            ErrorSeverity::High => {
                if matches!(error.category, ErrorCategory::Network) {
                    metrics.circuit_breaker_trips += 1;
                    Ok(ErrorHandlingResponse::CircuitBreaker {
                        trip_reason: error.message,
                        recovery_time: Duration::from_secs(60),
                    })
                } else {
                    metrics.retry_attempts += 1;
                    Ok(ErrorHandlingResponse::Retry {
                        delay: Duration::from_millis(1000),
                        attempt: 1,
                    })
                }
            }
            ErrorSeverity::Medium => {
                metrics.degradation_events += 1;
                Ok(ErrorHandlingResponse::Graceful {
                    fallback_action: "use_cache".to_string(),
                    degraded_mode: true,
                })
            }
            ErrorSeverity::Low => {
                Ok(ErrorHandlingResponse::Retry {
                    delay: Duration::from_millis(100),
                    attempt: 1,
                })
            }
        }
    }

    async fn classify_error(&self, error: &str) -> ErrorContext {
        let error_lower = error.to_lowercase();
        let patterns = self.error_patterns.read().await;
        
        let mut category = ErrorCategory::Unknown;
        let mut severity = ErrorSeverity::Low;

        for (pattern, cat) in patterns.iter() {
            if error_lower.contains(pattern) {
                category = cat.clone();
                break;
            }
        }

        severity = match category {
            ErrorCategory::Memory => ErrorSeverity::Critical,
            ErrorCategory::Network => ErrorSeverity::High,
            ErrorCategory::Database => ErrorSeverity::High,
            ErrorCategory::Processing => ErrorSeverity::Medium,
            ErrorCategory::Configuration => ErrorSeverity::Medium,
            ErrorCategory::External => ErrorSeverity::Low,
            ErrorCategory::Unknown => ErrorSeverity::Low,
        };

        let config = self.config.read().await;
        for critical_pattern in &config.base_config.critical_error_patterns {
            if error_lower.contains(critical_pattern) {
                severity = ErrorSeverity::Critical;
                break;
            }
        }

        ErrorContext {
            error_id: uuid::Uuid::new_v4().to_string(),
            severity,
            category,
            message: error.to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            component: "cross_exex".to_string(),
            metadata: serde_json::json!({}),
        }
    }

    async fn should_retry(&self, error: &ErrorContext) -> bool {
        let config = self.config.read().await;
        
        if !config.base_config.enable_retry {
            return false;
        }

        match error.severity {
            ErrorSeverity::Critical => false,
            ErrorSeverity::High => matches!(error.category, ErrorCategory::Network | ErrorCategory::External),
            ErrorSeverity::Medium | ErrorSeverity::Low => true,
        }
    }

    async fn get_metrics(&self) -> ErrorHandlingMetrics {
        self.metrics.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cross_exex_error_handler_creation() {
        let config = CrossExExErrorConfig::default();
        let handler = CrossExExErrorHandler::new(config);
        
        let metrics = handler.get_metrics().await;
        assert_eq!(metrics.total_errors, 0);
    }

    #[tokio::test]
    async fn test_exex_registration() {
        let config = CrossExExErrorConfig::default();
        let handler = CrossExExErrorHandler::new(config);
        
        handler.register_exex("test_exex".to_string()).await.unwrap();
        
        let status = handler.get_exex_status("test_exex").await;
        assert!(matches!(status, Some(ExExStatus::Healthy)));
    }

    #[tokio::test]
    async fn test_error_classification() {
        let config = CrossExExErrorConfig::default();
        let handler = CrossExExErrorHandler::new(config);
        
        let error_context = handler.classify_error("connection refused").await;
        assert!(matches!(error_context.category, ErrorCategory::Network));
        assert!(matches!(error_context.severity, ErrorSeverity::High));
    }

    #[tokio::test]
    async fn test_communication_result_recording() {
        let config = CrossExExErrorConfig::default();
        let handler = CrossExExErrorHandler::new(config);
        
        handler.register_exex("test_exex".to_string()).await.unwrap();
        
        handler.record_communication_result(
            "test_exex",
            true,
            Duration::from_millis(50),
            None,
        ).await.unwrap();
        
        let health_info = handler.exex_health.get("test_exex").unwrap();
        assert_eq!(health_info.successful_requests, 1);
    }
}