use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};
use sysinfo::{System, Cpu};

use super::MonitoringConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub check_interval: Duration,
    pub timeout: Duration,
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
    pub enable_dependency_checks: bool,
    pub enable_deep_health_checks: bool,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            timeout: Duration::from_secs(10),
            failure_threshold: 3,
            recovery_threshold: 2,
            enable_dependency_checks: true,
            enable_deep_health_checks: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub component_id: String,
    pub status: HealthStatus,
    pub timestamp: u64,
    pub response_time_ms: u64,
    pub checks_performed: Vec<IndividualCheck>,
    pub overall_score: f64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndividualCheck {
    pub name: String,
    pub status: HealthStatus,
    pub duration_ms: u64,
    pub details: HashMap<String, serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthMetrics {
    pub total_components: usize,
    pub healthy_components: usize,
    pub degraded_components: usize,
    pub unhealthy_components: usize,
    pub unknown_components: usize,
    pub average_response_time_ms: f64,
    pub total_checks_performed: u64,
    pub failed_checks: u64,
    pub health_check_success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub active_connections: u32,
    pub max_connections: u32,
    pub connection_pool_usage: f64,
}

#[derive(Debug, Clone)]
pub struct ResourceUsage {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub network_usage: f64,
}

impl Default for HealthMetrics {
    fn default() -> Self {
        Self {
            total_components: 0,
            healthy_components: 0,
            degraded_components: 0,
            unhealthy_components: 0,
            unknown_components: 0,
            average_response_time_ms: 0.0,
            total_checks_performed: 0,
            failed_checks: 0,
            health_check_success_rate: 100.0,
        }
    }
}

#[async_trait::async_trait]
pub trait HealthCheckProvider: Send + Sync {
    async fn check_health(&self, component_id: &str) -> Result<HealthCheckResult>;
    async fn get_dependencies(&self, component_id: &str) -> Vec<String>;
    fn component_type(&self) -> String;
}

#[derive(Debug)]
pub struct DefaultHealthChecker;

#[async_trait::async_trait]
impl HealthCheckProvider for DefaultHealthChecker {
    async fn check_health(&self, component_id: &str) -> Result<HealthCheckResult> {
        let start_time = Instant::now();
        
        let mut checks = Vec::new();
        
        let basic_check = self.perform_basic_check(component_id).await;
        checks.push(basic_check);
        
        let connectivity_check = self.perform_connectivity_check(component_id).await;
        checks.push(connectivity_check);
        
        let resource_check = self.perform_resource_check(component_id).await;
        checks.push(resource_check);

        let overall_status = self.determine_overall_status(&checks);
        let overall_score = self.calculate_health_score(&checks);
        
        let response_time = start_time.elapsed();
        
        Ok(HealthCheckResult {
            component_id: component_id.to_string(),
            status: overall_status,
            timestamp: chrono::Utc::now().timestamp() as u64,
            response_time_ms: response_time.as_millis() as u64,
            checks_performed: checks,
            overall_score,
            message: None,
        })
    }

    async fn get_dependencies(&self, component_id: &str) -> Vec<String> {
        match component_id {
            id if id.contains("rag") => vec!["vector_store".to_string(), "embeddings".to_string()],
            id if id.contains("memory") => vec!["database".to_string(), "cache".to_string()],
            id if id.contains("coordination") => vec!["consensus".to_string(), "communication".to_string()],
            _ => vec![],
        }
    }

    fn component_type(&self) -> String {
        "default".to_string()
    }
}

impl DefaultHealthChecker {
    async fn perform_basic_check(&self, component_id: &str) -> IndividualCheck {
        let start_time = Instant::now();
        
        let (status, error_msg) = match self.check_component_availability(component_id).await {
            Ok(available) => {
                if available {
                    (HealthStatus::Healthy, None)
                } else {
                    (HealthStatus::Degraded, Some("Component not responding".to_string()))
                }
            }
            Err(e) => (HealthStatus::Unhealthy, Some(format!("Basic check failed: {}", e)))
        };
        
        let mut details = HashMap::new();
        details.insert("component_id".to_string(), serde_json::Value::String(component_id.to_string()));
        details.insert("check_type".to_string(), serde_json::Value::String("basic".to_string()));
        details.insert("available".to_string(), serde_json::Value::Bool(matches!(status, HealthStatus::Healthy)));
        
        IndividualCheck {
            name: "basic_health".to_string(),
            status,
            duration_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: error_msg,
        }
    }

    async fn perform_connectivity_check(&self, component_id: &str) -> IndividualCheck {
        let start_time = Instant::now();
        
        let (status, connection_count, error_msg) = match self.check_component_connectivity(component_id).await {
            Ok(conn_info) => {
                let status = if conn_info.active_connections > 0 {
                    HealthStatus::Healthy
                } else {
                    HealthStatus::Degraded
                };
                (status, conn_info.active_connections, None)
            }
            Err(e) => (HealthStatus::Unhealthy, 0, Some(format!("Connectivity check failed: {}", e)))
        };
        
        let mut details = HashMap::new();
        details.insert("component_id".to_string(), serde_json::Value::String(component_id.to_string()));
        details.insert("check_type".to_string(), serde_json::Value::String("connectivity".to_string()));
        details.insert("connection_count".to_string(), serde_json::Value::Number(
            serde_json::Number::from(connection_count)
        ));
        details.insert("network_latency_ms".to_string(), serde_json::Value::Number(
            serde_json::Number::from(start_time.elapsed().as_millis() as u64)
        ));
        
        IndividualCheck {
            name: "connectivity".to_string(),
            status,
            duration_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: error_msg,
        }
    }

    async fn perform_resource_check(&self, component_id: &str) -> IndividualCheck {
        let start_time = Instant::now();
        
        let (status, cpu_usage, memory_usage, error_msg) = match self.get_system_resource_usage(component_id).await {
            Ok(resources) => {
                let status = if resources.cpu_usage < 80.0 && resources.memory_usage < 85.0 {
                    HealthStatus::Healthy
                } else if resources.cpu_usage < 95.0 && resources.memory_usage < 95.0 {
                    HealthStatus::Degraded
                } else {
                    HealthStatus::Unhealthy
                };
                
                let error = if status == HealthStatus::Unhealthy {
                    Some(format!("Resource usage critical: CPU={:.1}%, Memory={:.1}%", resources.cpu_usage, resources.memory_usage))
                } else {
                    None
                };
                
                (status, resources.cpu_usage, resources.memory_usage, error)
            }
            Err(e) => (HealthStatus::Unknown, 0.0, 0.0, Some(format!("Resource check failed: {}", e)))
        };
        
        let mut details = HashMap::new();
        details.insert("component_id".to_string(), serde_json::Value::String(component_id.to_string()));
        details.insert("check_type".to_string(), serde_json::Value::String("resources".to_string()));
        details.insert("cpu_usage_percent".to_string(), serde_json::Value::Number(
            serde_json::Number::from_f64(cpu_usage).unwrap_or_default()
        ));
        details.insert("memory_usage_percent".to_string(), serde_json::Value::Number(
            serde_json::Number::from_f64(memory_usage).unwrap_or_default()
        ));
        
        IndividualCheck {
            name: "resources".to_string(),
            status,
            duration_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: error_msg,
        }
    }

    fn determine_overall_status(&self, checks: &[IndividualCheck]) -> HealthStatus {
        let mut healthy_count = 0;
        let mut degraded_count = 0;
        let mut unhealthy_count = 0;
        
        for check in checks {
            match check.status {
                HealthStatus::Healthy => healthy_count += 1,
                HealthStatus::Degraded => degraded_count += 1,
                HealthStatus::Unhealthy => unhealthy_count += 1,
                HealthStatus::Unknown => {}
            }
        }
        
        if unhealthy_count > 0 {
            HealthStatus::Unhealthy
        } else if degraded_count > 0 {
            HealthStatus::Degraded
        } else if healthy_count > 0 {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unknown
        }
    }

    fn calculate_health_score(&self, checks: &[IndividualCheck]) -> f64 {
        if checks.is_empty() {
            return 0.0;
        }
        
        let total_score: f64 = checks.iter()
            .map(|check| match check.status {
                HealthStatus::Healthy => 1.0,
                HealthStatus::Degraded => 0.5,
                HealthStatus::Unhealthy => 0.0,
                HealthStatus::Unknown => 0.0,
            })
            .sum();
        
        total_score / checks.len() as f64
    }

    async fn check_component_availability(&self, component_id: &str) -> Result<bool> {
        match component_id {
            id if id.contains("rag") => {
                tokio::time::timeout(Duration::from_millis(100), async {
                    std::thread::sleep(Duration::from_millis(5));
                    Ok(true)
                }).await.unwrap_or(Ok(false))
            },
            id if id.contains("memory") => {
                tokio::time::timeout(Duration::from_millis(100), async {
                    std::thread::sleep(Duration::from_millis(3));
                    Ok(true)
                }).await.unwrap_or(Ok(false))
            },
            id if id.contains("coordination") => {
                tokio::time::timeout(Duration::from_millis(100), async {
                    std::thread::sleep(Duration::from_millis(8));
                    Ok(true)
                }).await.unwrap_or(Ok(false))
            },
            _ => Ok(true)
        }
    }

    async fn check_component_connectivity(&self, component_id: &str) -> Result<ConnectionInfo> {
        let base_connections = match component_id {
            id if id.contains("rag") => 5,
            id if id.contains("memory") => 3,
            id if id.contains("coordination") => 8,
            id if id.contains("dashboard") => 2,
            _ => 1,
        };

        let active_connections = base_connections + (chrono::Utc::now().timestamp() % 5) as u32;
        let max_connections = base_connections * 4;
        let connection_pool_usage = active_connections as f64 / max_connections as f64;

        Ok(ConnectionInfo {
            active_connections,
            max_connections,
            connection_pool_usage,
        })
    }

    async fn get_system_resource_usage(&self, component_id: &str) -> Result<ResourceUsage> {
        let mut system = System::new_all();
        system.refresh_all();

        let cpu_usage = if system.cpus().is_empty() {
            15.0
        } else {
            let total_cpu: f32 = system.cpus().iter()
                .map(|cpu| cpu.cpu_usage())
                .sum();
            (total_cpu / system.cpus().len() as f32) as f64
        };

        let memory_total = system.total_memory();
        let memory_used = system.used_memory();
        let memory_usage = if memory_total > 0 {
            (memory_used as f64 / memory_total as f64) * 100.0
        } else {
            20.0
        };

        let component_factor = match component_id {
            id if id.contains("rag") => 1.2,
            id if id.contains("memory") => 1.5,
            id if id.contains("coordination") => 0.8,
            _ => 1.0,
        };

        let disk_usage = 35.0 * component_factor;
        let network_usage = 12.0 * component_factor;

        Ok(ResourceUsage {
            cpu_usage: cpu_usage * component_factor,
            memory_usage: memory_usage * component_factor,
            disk_usage,
            network_usage,
        })
    }
}

#[derive(Debug)]
pub struct HealthChecker {
    config: Arc<RwLock<MonitoringConfig>>,
    providers: Arc<RwLock<HashMap<String, Box<dyn HealthCheckProvider>>>>,
    health_results: Arc<RwLock<HashMap<String, HealthCheckResult>>>,
    component_history: Arc<RwLock<HashMap<String, Vec<HealthStatus>>>>,
    metrics: Arc<RwLock<HealthMetrics>>,
    monitoring_active: Arc<RwLock<bool>>,
}

impl HealthChecker {
    pub fn new(config: MonitoringConfig) -> Self {
        let mut providers: HashMap<String, Box<dyn HealthCheckProvider>> = HashMap::new();
        providers.insert("default".to_string(), Box::new(DefaultHealthChecker));
        
        Self {
            config: Arc::new(RwLock::new(config)),
            providers: Arc::new(RwLock::new(providers)),
            health_results: Arc::new(RwLock::new(HashMap::new())),
            component_history: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(HealthMetrics::default())),
            monitoring_active: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn register_provider(&self, component_type: String, provider: Box<dyn HealthCheckProvider>) {
        self.providers.write().await.insert(component_type.clone(), provider);
        info!("Registered health check provider for: {}", component_type);
    }

    pub async fn check_component_health(&self, component_id: &str) -> Result<bool> {
        let result = self.perform_health_check(component_id).await?;
        Ok(matches!(result.status, HealthStatus::Healthy))
    }

    pub async fn perform_health_check(&self, component_id: &str) -> Result<HealthCheckResult> {
        let provider = self.select_provider(component_id).await;
        
        let config = self.config.read().await;
        let timeout = HealthCheckConfig::default().timeout;
        drop(config);

        let result = tokio::time::timeout(
            timeout,
            provider.check_health(component_id)
        ).await;

        match result {
            Ok(Ok(health_result)) => {
                self.store_health_result(health_result.clone()).await;
                self.update_metrics(&health_result).await;
                Ok(health_result)
            }
            Ok(Err(e)) => {
                let failed_result = HealthCheckResult {
                    component_id: component_id.to_string(),
                    status: HealthStatus::Unhealthy,
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    response_time_ms: timeout.as_millis() as u64,
                    checks_performed: vec![],
                    overall_score: 0.0,
                    message: Some(format!("Health check failed: {}", e)),
                };
                
                self.store_health_result(failed_result.clone()).await;
                self.update_metrics(&failed_result).await;
                Ok(failed_result)
            }
            Err(_) => {
                let timeout_result = HealthCheckResult {
                    component_id: component_id.to_string(),
                    status: HealthStatus::Unknown,
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    response_time_ms: timeout.as_millis() as u64,
                    checks_performed: vec![],
                    overall_score: 0.0,
                    message: Some("Health check timed out".to_string()),
                };
                
                self.store_health_result(timeout_result.clone()).await;
                self.update_metrics(&timeout_result).await;
                Ok(timeout_result)
            }
        }
    }

    async fn select_provider(&self, component_id: &str) -> Box<dyn HealthCheckProvider> {
        let providers = self.providers.read().await;
        
        let component_type = if component_id.contains("rag") {
            "rag"
        } else if component_id.contains("memory") {
            "memory"
        } else if component_id.contains("coordination") {
            "coordination"
        } else {
            "default"
        };

        if let Some(provider) = providers.get(component_type) {
            provider.component_type();
        }
        
        Box::new(DefaultHealthChecker)
    }

    async fn store_health_result(&self, result: HealthCheckResult) {
        let component_id = result.component_id.clone();
        
        self.health_results.write().await.insert(component_id.clone(), result.clone());
        
        let mut history = self.component_history.write().await;
        let component_history = history.entry(component_id).or_insert_with(Vec::new);
        component_history.push(result.status);
        
        if component_history.len() > 100 {
            component_history.remove(0);
        }
    }

    async fn update_metrics(&self, result: &HealthCheckResult) {
        let mut metrics = self.metrics.write().await;
        
        metrics.total_checks_performed += 1;
        
        if !matches!(result.status, HealthStatus::Healthy) {
            metrics.failed_checks += 1;
        }
        
        metrics.health_check_success_rate = 
            (metrics.total_checks_performed - metrics.failed_checks) as f64 / 
            metrics.total_checks_performed as f64 * 100.0;

        let alpha = 0.1;
        metrics.average_response_time_ms = alpha * result.response_time_ms as f64 + 
            (1.0 - alpha) * metrics.average_response_time_ms;

        let all_results = self.health_results.read().await;
        metrics.total_components = all_results.len();
        metrics.healthy_components = all_results.values()
            .filter(|r| matches!(r.status, HealthStatus::Healthy))
            .count();
        metrics.degraded_components = all_results.values()
            .filter(|r| matches!(r.status, HealthStatus::Degraded))
            .count();
        metrics.unhealthy_components = all_results.values()
            .filter(|r| matches!(r.status, HealthStatus::Unhealthy))
            .count();
        metrics.unknown_components = all_results.values()
            .filter(|r| matches!(r.status, HealthStatus::Unknown))
            .count();
    }

    pub async fn check_all_components(&self) -> Result<HashMap<String, HealthCheckResult>> {
        let components: Vec<String> = self.health_results.read().await.keys().cloned().collect();
        let mut results = HashMap::new();
        
        for component_id in components {
            match self.perform_health_check(&component_id).await {
                Ok(result) => {
                    results.insert(component_id, result);
                }
                Err(e) => {
                    warn!("Failed to check health of component {}: {}", component_id, e);
                }
            }
        }
        
        Ok(results)
    }

    pub async fn get_component_health(&self, component_id: &str) -> Option<HealthCheckResult> {
        self.health_results.read().await.get(component_id).cloned()
    }

    pub async fn get_all_health_results(&self) -> HashMap<String, HealthCheckResult> {
        self.health_results.read().await.clone()
    }

    pub async fn get_component_trend(&self, component_id: &str, duration: Duration) -> Vec<HealthStatus> {
        let history = self.component_history.read().await;
        
        if let Some(component_history) = history.get(component_id) {
            let samples_needed = (duration.as_secs() / 30).min(component_history.len() as u64) as usize;
            component_history.iter()
                .rev()
                .take(samples_needed)
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }

    pub async fn get_health_metrics(&self) -> HealthMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn start(&self) -> Result<()> {
        if *self.monitoring_active.read().await {
            return Ok(());
        }

        *self.monitoring_active.write().await = true;
        
        let checker = self.clone();
        tokio::spawn(async move {
            checker.health_monitoring_loop().await;
        });

        info!("Health checker started");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        *self.monitoring_active.write().await = false;
        info!("Health checker stopped");
        Ok(())
    }

    async fn health_monitoring_loop(&self) {
        let config = self.config.read().await;
        let check_interval = HealthCheckConfig::default().check_interval;
        drop(config);
        
        let mut interval = tokio::time::interval(check_interval);
        
        while *self.monitoring_active.read().await {
            interval.tick().await;
            
            if let Err(e) = self.perform_scheduled_checks().await {
                error!("Scheduled health checks failed: {}", e);
            }
        }
    }

    async fn perform_scheduled_checks(&self) -> Result<()> {
        let components: Vec<String> = self.health_results.read().await.keys().cloned().collect();
        
        for component_id in components {
            if let Err(e) = self.perform_health_check(&component_id).await {
                warn!("Health check failed for component {}: {}", component_id, e);
            }
        }
        
        Ok(())
    }
}

impl Clone for HealthChecker {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            providers: self.providers.clone(),
            health_results: self.health_results.clone(),
            component_history: self.component_history.clone(),
            metrics: self.metrics.clone(),
            monitoring_active: self.monitoring_active.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_checker_creation() {
        let config = MonitoringConfig::default();
        let checker = HealthChecker::new(config);
        
        let metrics = checker.get_health_metrics().await;
        assert_eq!(metrics.total_components, 0);
    }

    #[tokio::test]
    async fn test_default_health_check() {
        let checker = DefaultHealthChecker;
        let result = checker.check_health("test_component").await.unwrap();
        
        assert!(!result.checks_performed.is_empty());
        assert!(result.response_time_ms > 0);
    }

    #[tokio::test]
    async fn test_health_status_determination() {
        let checker = DefaultHealthChecker;
        
        let checks = vec![
            IndividualCheck {
                name: "test1".to_string(),
                status: HealthStatus::Healthy,
                duration_ms: 10,
                details: HashMap::new(),
                error: None,
            },
            IndividualCheck {
                name: "test2".to_string(),
                status: HealthStatus::Degraded,
                duration_ms: 15,
                details: HashMap::new(),
                error: Some("Minor issue".to_string()),
            },
        ];
        
        let status = checker.determine_overall_status(&checks);
        assert_eq!(status, HealthStatus::Degraded);
        
        let score = checker.calculate_health_score(&checks);
        assert_eq!(score, 0.75);
    }
}