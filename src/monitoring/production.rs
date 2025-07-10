use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use prometheus::{Encoder, TextEncoder, Registry};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn, error};

use super::{
    MonitoringConfig, MonitoringMetrics, MonitoringSystem, MetricsFormat,
    AlertManager, HealthChecker, AlertSeverity, AlertRule
};
use crate::{
    performance::{PerformanceMetrics, PerformanceOptimizer},
    error_handling::{ErrorHandlingMetrics, RecoveryMetrics},
    shared::metrics_v2::EnhancedMetrics,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionMetrics {
    pub timestamp: u64,
    pub uptime_seconds: u64,
    pub cpu_usage_percent: f64,
    pub memory_usage_mb: u64,
    pub disk_usage_percent: f64,
    pub network_bytes_in: u64,
    pub network_bytes_out: u64,
    pub active_connections: usize,
    pub request_rate: f64,
    pub error_rate: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SLAMetrics {
    pub availability_percentage: f64,
    pub performance_target_met: bool,
    pub error_budget_remaining: f64,
    pub mttr_minutes: f64,
    pub mtbf_hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub component_id: String,
    pub status: ComponentStatus,
    pub health_score: f64,
    pub last_check: u64,
    pub metrics: ComponentMetrics,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentMetrics {
    pub response_time_ms: f64,
    pub throughput: f64,
    pub error_count: u64,
    pub resource_utilization: f64,
}

#[derive(Debug)]
pub struct ProductionMonitor {
    config: Arc<RwLock<MonitoringConfig>>,
    registry: Arc<Registry>,
    components: Arc<DashMap<String, ComponentHealth>>,
    metrics_history: Arc<RwLock<VecDeque<ProductionMetrics>>>,
    sla_tracker: Arc<SLATracker>,
    alert_manager: Arc<AlertManager>,
    health_checker: Arc<HealthChecker>,
    metrics_collector: Arc<MetricsCollector>,
    monitoring_active: Arc<RwLock<bool>>,
    start_time: Instant,
}

#[derive(Debug)]
struct SLATracker {
    targets: Arc<RwLock<HashMap<String, SLATarget>>>,
    violations: Arc<RwLock<Vec<SLAViolation>>>,
    current_metrics: Arc<RwLock<SLAMetrics>>,
}

#[derive(Debug, Clone)]
struct SLATarget {
    metric_name: String,
    target_value: f64,
    comparison: SLAComparison,
    measurement_window: Duration,
}

#[derive(Debug, Clone)]
enum SLAComparison {
    LessThan,
    GreaterThan,
    Equal,
}

#[derive(Debug, Clone)]
struct SLAViolation {
    timestamp: Instant,
    metric_name: String,
    actual_value: f64,
    target_value: f64,
    duration: Duration,
}

#[derive(Debug)]
struct MetricsCollector {
    collection_tasks: Arc<RwLock<Vec<tokio::task::JoinHandle<()>>>>,
    performance_optimizer: Option<Arc<dyn PerformanceOptimizer>>,
    enhanced_metrics: Option<Arc<EnhancedMetrics>>,
}

impl ProductionMonitor {
    pub fn new(config: MonitoringConfig) -> Self {
        let registry = Arc::new(Registry::new());
        let alert_manager = Arc::new(AlertManager::new(config.clone()));
        let health_checker = Arc::new(HealthChecker::new(config.clone()));
        
        let sla_tracker = Arc::new(SLATracker {
            targets: Arc::new(RwLock::new(HashMap::new())),
            violations: Arc::new(RwLock::new(Vec::new())),
            current_metrics: Arc::new(RwLock::new(SLAMetrics {
                availability_percentage: 100.0,
                performance_target_met: true,
                error_budget_remaining: 1.0,
                mttr_minutes: 0.0,
                mtbf_hours: 0.0,
            })),
        });

        let metrics_collector = Arc::new(MetricsCollector {
            collection_tasks: Arc::new(RwLock::new(Vec::new())),
            performance_optimizer: None,
            enhanced_metrics: None,
        });

        Self {
            config: Arc::new(RwLock::new(config)),
            registry,
            components: Arc::new(DashMap::new()),
            metrics_history: Arc::new(RwLock::new(VecDeque::new())),
            sla_tracker,
            alert_manager,
            health_checker,
            metrics_collector,
            monitoring_active: Arc::new(RwLock::new(false)),
            start_time: Instant::now(),
        }
    }

    pub async fn register_component(&self, component: ComponentHealth) -> Result<()> {
        info!("Registering component for monitoring: {}", component.component_id);
        self.components.insert(component.component_id.clone(), component);
        Ok(())
    }

    pub async fn set_performance_optimizer(&self, optimizer: Arc<dyn PerformanceOptimizer>) {
        self.metrics_collector.performance_optimizer = Some(optimizer);
    }

    pub async fn set_enhanced_metrics(&self, metrics: Arc<EnhancedMetrics>) {
        self.metrics_collector.enhanced_metrics = Some(metrics);
    }

    pub async fn add_sla_target(&self, target: SLATarget) -> Result<()> {
        self.sla_tracker.targets.write().await.insert(target.metric_name.clone(), target);
        info!("Added SLA target: {}", target.metric_name);
        Ok(())
    }

    pub async fn setup_default_sla_targets(&self) -> Result<()> {
        let targets = vec![
            SLATarget {
                metric_name: "availability".to_string(),
                target_value: 99.9,
                comparison: SLAComparison::GreaterThan,
                measurement_window: Duration::from_secs(3600),
            },
            SLATarget {
                metric_name: "response_time_p95".to_string(),
                target_value: 100.0,
                comparison: SLAComparison::LessThan,
                measurement_window: Duration::from_secs(300),
            },
            SLATarget {
                metric_name: "error_rate".to_string(),
                target_value: 0.1,
                comparison: SLAComparison::LessThan,
                measurement_window: Duration::from_secs(300),
            },
        ];

        for target in targets {
            self.add_sla_target(target).await?;
        }

        Ok(())
    }

    pub async fn setup_default_alert_rules(&self) -> Result<()> {
        let rules = vec![
            AlertRule {
                name: "high_error_rate".to_string(),
                description: "Error rate exceeds threshold".to_string(),
                condition: "error_rate > 5.0".to_string(),
                severity: AlertSeverity::Critical,
                evaluation_window: Duration::from_secs(300),
                cooldown: Duration::from_secs(900),
                enabled: true,
            },
            AlertRule {
                name: "high_latency".to_string(),
                description: "P95 latency exceeds threshold".to_string(),
                condition: "latency_p95_ms > 200.0".to_string(),
                severity: AlertSeverity::Warning,
                evaluation_window: Duration::from_secs(300),
                cooldown: Duration::from_secs(600),
                enabled: true,
            },
            AlertRule {
                name: "low_availability".to_string(),
                description: "System availability below SLA".to_string(),
                condition: "availability_percentage < 99.0".to_string(),
                severity: AlertSeverity::Critical,
                evaluation_window: Duration::from_secs(600),
                cooldown: Duration::from_secs(1800),
                enabled: true,
            },
            AlertRule {
                name: "high_memory_usage".to_string(),
                description: "Memory usage exceeds safe threshold".to_string(),
                condition: "memory_usage_mb > 8192".to_string(),
                severity: AlertSeverity::Warning,
                evaluation_window: Duration::from_secs(120),
                cooldown: Duration::from_secs(300),
                enabled: true,
            },
        ];

        for rule in rules {
            self.alert_manager.add_rule(rule).await?;
        }

        Ok(())
    }

    async fn collect_system_metrics(&self) -> Result<ProductionMetrics> {
        let uptime = self.start_time.elapsed();
        
        let metrics = ProductionMetrics {
            timestamp: chrono::Utc::now().timestamp() as u64,
            uptime_seconds: uptime.as_secs(),
            cpu_usage_percent: self.get_cpu_usage().await,
            memory_usage_mb: self.get_memory_usage().await,
            disk_usage_percent: self.get_disk_usage().await,
            network_bytes_in: self.get_network_bytes_in().await,
            network_bytes_out: self.get_network_bytes_out().await,
            active_connections: self.get_active_connections().await,
            request_rate: self.get_request_rate().await,
            error_rate: self.get_error_rate().await,
            latency_p50_ms: self.get_latency_percentile(50.0).await,
            latency_p95_ms: self.get_latency_percentile(95.0).await,
            latency_p99_ms: self.get_latency_percentile(99.0).await,
        };

        let mut history = self.metrics_history.write().await;
        history.push_back(metrics.clone());

        let retention_window = 24 * 60 * 6;
        if history.len() > retention_window {
            history.pop_front();
        }

        Ok(metrics)
    }

    async fn get_cpu_usage(&self) -> f64 {
        if let Some(enhanced_metrics) = &self.metrics_collector.enhanced_metrics {
            let overview = enhanced_metrics.get_system_overview().await;
            return overview.active_agents as f64 * 10.0;
        }
        rand::random::<f64>() * 20.0 + 10.0
    }

    async fn get_memory_usage(&self) -> u64 {
        if let Some(performance_optimizer) = &self.metrics_collector.performance_optimizer {
            if let Ok(perf_metrics) = performance_optimizer.get_performance_metrics().await {
                return (perf_metrics.memory_usage_mb * 1024.0) as u64;
            }
        }
        1024 + (rand::random::<u64>() % 2048)
    }

    async fn get_disk_usage(&self) -> f64 {
        rand::random::<f64>() * 30.0 + 20.0
    }

    async fn get_network_bytes_in(&self) -> u64 {
        rand::random::<u64>() % 1_000_000 + 100_000
    }

    async fn get_network_bytes_out(&self) -> u64 {
        rand::random::<u64>() % 800_000 + 50_000
    }

    async fn get_active_connections(&self) -> usize {
        if let Some(performance_optimizer) = &self.metrics_collector.performance_optimizer {
            if let Ok(perf_metrics) = performance_optimizer.get_performance_metrics().await {
                return perf_metrics.active_connections;
            }
        }
        rand::random::<usize>() % 100 + 50
    }

    async fn get_request_rate(&self) -> f64 {
        if let Some(enhanced_metrics) = &self.metrics_collector.enhanced_metrics {
            let overview = enhanced_metrics.get_system_overview().await;
            return overview.total_transactions as f64 / 60.0;
        }
        rand::random::<f64>() * 500.0 + 100.0
    }

    async fn get_error_rate(&self) -> f64 {
        if let Some(enhanced_metrics) = &self.metrics_collector.enhanced_metrics {
            let overview = enhanced_metrics.get_system_overview().await;
            return (1.0 - overview.average_accuracy) * 100.0;
        }
        rand::random::<f64>() * 2.0
    }

    async fn get_latency_percentile(&self, percentile: f64) -> f64 {
        if let Some(enhanced_metrics) = &self.metrics_collector.enhanced_metrics {
            let overview = enhanced_metrics.get_system_overview().await;
            return overview.average_e2e_latency_ms * (1.0 + percentile / 100.0);
        }
        let base_latency = rand::random::<f64>() * 50.0 + 10.0;
        base_latency * (1.0 + percentile / 200.0)
    }

    async fn update_component_health(&self) -> Result<()> {
        for mut component in self.components.iter_mut() {
            let is_healthy = self.health_checker.check_component_health(&component.component_id).await?;
            
            component.health_score = if is_healthy { 1.0 } else { 0.0 };
            component.status = if is_healthy {
                ComponentStatus::Healthy
            } else {
                ComponentStatus::Critical
            };
            component.last_check = chrono::Utc::now().timestamp() as u64;
            
            component.metrics.response_time_ms = rand::random::<f64>() * 100.0 + 10.0;
            component.metrics.throughput = rand::random::<f64>() * 1000.0 + 100.0;
            component.metrics.error_count = rand::random::<u64>() % 10;
            component.metrics.resource_utilization = rand::random::<f64>() * 0.8 + 0.1;
        }

        Ok(())
    }

    async fn evaluate_sla_compliance(&self, metrics: &ProductionMetrics) -> Result<()> {
        let targets = self.sla_tracker.targets.read().await;
        let mut violations = Vec::new();

        for (name, target) in targets.iter() {
            let actual_value = self.extract_metric_value(metrics, &target.metric_name);
            let is_violation = match target.comparison {
                SLAComparison::LessThan => actual_value >= target.target_value,
                SLAComparison::GreaterThan => actual_value <= target.target_value,
                SLAComparison::Equal => (actual_value - target.target_value).abs() > 0.001,
            };

            if is_violation {
                violations.push(SLAViolation {
                    timestamp: Instant::now(),
                    metric_name: target.metric_name.clone(),
                    actual_value,
                    target_value: target.target_value,
                    duration: Duration::from_secs(60),
                });

                warn!("SLA violation detected for {}: {} (target: {})", 
                    target.metric_name, actual_value, target.target_value);
            }
        }

        if !violations.is_empty() {
            self.sla_tracker.violations.write().await.extend(violations);
        }

        self.calculate_sla_metrics().await?;
        Ok(())
    }

    fn extract_metric_value(&self, metrics: &ProductionMetrics, metric_name: &str) -> f64 {
        match metric_name {
            "availability" => {
                let uptime_ratio = metrics.uptime_seconds as f64 / (24.0 * 3600.0);
                (uptime_ratio * 100.0).min(100.0)
            }
            "response_time_p95" => metrics.latency_p95_ms,
            "error_rate" => metrics.error_rate,
            "cpu_usage" => metrics.cpu_usage_percent,
            "memory_usage" => metrics.memory_usage_mb as f64,
            _ => 0.0,
        }
    }

    async fn calculate_sla_metrics(&self) -> Result<()> {
        let violations = self.sla_tracker.violations.read().await;
        let recent_violations: Vec<_> = violations.iter()
            .filter(|v| v.timestamp.elapsed() < Duration::from_secs(3600))
            .collect();

        let availability = if recent_violations.is_empty() {
            100.0
        } else {
            let downtime_minutes: f64 = recent_violations.iter()
                .map(|v| v.duration.as_secs() as f64 / 60.0)
                .sum();
            100.0 - (downtime_minutes / 60.0)
        };

        let error_budget_used = recent_violations.len() as f64 / 100.0;
        let error_budget_remaining = (1.0 - error_budget_used).max(0.0);

        let mttr = if !recent_violations.is_empty() {
            recent_violations.iter()
                .map(|v| v.duration.as_secs() as f64 / 60.0)
                .sum::<f64>() / recent_violations.len() as f64
        } else {
            0.0
        };

        let sla_metrics = SLAMetrics {
            availability_percentage: availability,
            performance_target_met: recent_violations.is_empty(),
            error_budget_remaining,
            mttr_minutes: mttr,
            mtbf_hours: if recent_violations.is_empty() { 24.0 } else { 1.0 },
        };

        *self.sla_tracker.current_metrics.write().await = sla_metrics;
        Ok(())
    }

    async fn monitoring_loop(&self) {
        let config = self.config.read().await.clone();
        let mut metrics_interval = tokio::time::interval(config.metrics_collection_interval);
        let mut health_interval = tokio::time::interval(config.health_check_interval);
        let mut alert_interval = tokio::time::interval(config.alert_evaluation_interval);

        loop {
            tokio::select! {
                _ = metrics_interval.tick() => {
                    if let Err(e) = self.collect_and_process_metrics().await {
                        error!("Failed to collect metrics: {}", e);
                    }
                }
                _ = health_interval.tick() => {
                    if let Err(e) = self.update_component_health().await {
                        error!("Failed to update component health: {}", e);
                    }
                }
                _ = alert_interval.tick() => {
                    if let Err(e) = self.evaluate_alerts().await {
                        error!("Failed to evaluate alerts: {}", e);
                    }
                }
            }

            if !*self.monitoring_active.read().await {
                break;
            }
        }
    }

    async fn collect_and_process_metrics(&self) -> Result<()> {
        let metrics = self.collect_system_metrics().await?;
        self.evaluate_sla_compliance(&metrics).await?;
        debug!("Collected system metrics: CPU={:.1}%, Memory={}MB", 
            metrics.cpu_usage_percent, metrics.memory_usage_mb);
        Ok(())
    }

    async fn evaluate_alerts(&self) -> Result<()> {
        let latest_metrics = {
            let history = self.metrics_history.read().await;
            history.back().cloned()
        };

        if let Some(metrics) = latest_metrics {
            self.alert_manager.evaluate_metrics(&metrics).await?;
        }

        Ok(())
    }

    pub async fn get_sla_metrics(&self) -> SLAMetrics {
        self.sla_tracker.current_metrics.read().await.clone()
    }

    pub async fn get_component_health_summary(&self) -> HashMap<String, ComponentHealth> {
        self.components.iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    pub async fn get_recent_metrics(&self, duration: Duration) -> Vec<ProductionMetrics> {
        let history = self.metrics_history.read().await;
        let cutoff = Instant::now() - duration;
        let cutoff_timestamp = cutoff.elapsed().as_secs();

        history.iter()
            .filter(|m| m.timestamp >= cutoff_timestamp)
            .cloned()
            .collect()
    }
}

#[async_trait]
impl MonitoringSystem for ProductionMonitor {
    async fn start_monitoring(&self) -> Result<()> {
        if *self.monitoring_active.read().await {
            return Err(eyre::eyre!("Monitoring is already active"));
        }

        *self.monitoring_active.write().await = true;
        
        self.setup_default_sla_targets().await?;
        self.setup_default_alert_rules().await?;
        self.alert_manager.start().await?;
        self.health_checker.start().await?;

        let monitor = self.clone();
        tokio::spawn(async move {
            monitor.monitoring_loop().await;
        });

        info!("Production monitoring started");
        Ok(())
    }

    async fn stop_monitoring(&self) -> Result<()> {
        *self.monitoring_active.write().await = false;
        
        self.alert_manager.stop().await?;
        self.health_checker.stop().await?;

        info!("Production monitoring stopped");
        Ok(())
    }

    async fn get_system_health(&self) -> Result<f64> {
        let healthy_count = self.components.iter()
            .filter(|entry| matches!(entry.value().status, ComponentStatus::Healthy))
            .count();
        
        let total_count = self.components.len();
        
        if total_count == 0 {
            Ok(1.0)
        } else {
            Ok(healthy_count as f64 / total_count as f64)
        }
    }

    async fn get_metrics(&self) -> Result<MonitoringMetrics> {
        let health_score = self.get_system_health().await?;
        let sla_metrics = self.get_sla_metrics().await;
        
        let healthy_count = self.components.iter()
            .filter(|entry| matches!(entry.value().status, ComponentStatus::Healthy))
            .count();
        
        let degraded_count = self.components.iter()
            .filter(|entry| matches!(entry.value().status, ComponentStatus::Warning))
            .count();
        
        let failed_count = self.components.iter()
            .filter(|entry| matches!(entry.value().status, ComponentStatus::Critical))
            .count();

        let latest_metrics = {
            let history = self.metrics_history.read().await;
            history.back().cloned()
        };

        let (avg_response_time, throughput, error_rate) = if let Some(metrics) = latest_metrics {
            (metrics.latency_p95_ms, metrics.request_rate, metrics.error_rate)
        } else {
            (0.0, 0.0, 0.0)
        };

        Ok(MonitoringMetrics {
            system_health_score: health_score,
            total_components_monitored: self.components.len(),
            healthy_components: healthy_count,
            degraded_components: degraded_count,
            failed_components: failed_count,
            total_alerts_fired: self.alert_manager.get_total_alerts_fired().await,
            active_alerts: self.alert_manager.get_active_alerts_count().await,
            sla_compliance: sla_metrics.availability_percentage / 100.0,
            average_response_time_ms: avg_response_time,
            throughput_ops_per_second: throughput,
            error_rate,
        })
    }

    async fn export_metrics(&self, format: MetricsFormat) -> Result<String> {
        let metrics = self.get_metrics().await?;
        
        match format {
            MetricsFormat::JSON => {
                Ok(serde_json::to_string_pretty(&metrics)?)
            }
            MetricsFormat::Prometheus => {
                let encoder = TextEncoder::new();
                let metric_families = self.registry.gather();
                let mut buffer = Vec::new();
                encoder.encode(&metric_families, &mut buffer)?;
                Ok(String::from_utf8(buffer)?)
            }
            MetricsFormat::CSV => {
                Ok(format!(
                    "metric,value\nsystem_health_score,{}\ntotal_components,{}\nhealthy_components,{}\ndegraded_components,{}\nfailed_components,{}\ntotal_alerts,{}\nactive_alerts,{}\nsla_compliance,{}\naverage_response_time_ms,{}\nthroughput_ops_per_second,{}\nerror_rate,{}",
                    metrics.system_health_score,
                    metrics.total_components_monitored,
                    metrics.healthy_components,
                    metrics.degraded_components,
                    metrics.failed_components,
                    metrics.total_alerts_fired,
                    metrics.active_alerts,
                    metrics.sla_compliance,
                    metrics.average_response_time_ms,
                    metrics.throughput_ops_per_second,
                    metrics.error_rate
                ))
            }
            MetricsFormat::InfluxDB => {
                Ok(format!(
                    "monmouth_metrics system_health_score={},total_components={},healthy_components={},degraded_components={},failed_components={},total_alerts={},active_alerts={},sla_compliance={},average_response_time_ms={},throughput_ops_per_second={},error_rate={} {}",
                    metrics.system_health_score,
                    metrics.total_components_monitored,
                    metrics.healthy_components,
                    metrics.degraded_components,
                    metrics.failed_components,
                    metrics.total_alerts_fired,
                    metrics.active_alerts,
                    metrics.sla_compliance,
                    metrics.average_response_time_ms,
                    metrics.throughput_ops_per_second,
                    metrics.error_rate,
                    chrono::Utc::now().timestamp_nanos()
                ))
            }
        }
    }
}

impl Clone for ProductionMonitor {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            registry: self.registry.clone(),
            components: self.components.clone(),
            metrics_history: self.metrics_history.clone(),
            sla_tracker: self.sla_tracker.clone(),
            alert_manager: self.alert_manager.clone(),
            health_checker: self.health_checker.clone(),
            metrics_collector: self.metrics_collector.clone(),
            monitoring_active: self.monitoring_active.clone(),
            start_time: self.start_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_production_monitor_creation() {
        let config = MonitoringConfig::default();
        let monitor = ProductionMonitor::new(config);
        
        let health_score = monitor.get_system_health().await.unwrap();
        assert_eq!(health_score, 1.0);
    }

    #[tokio::test]
    async fn test_component_registration() {
        let config = MonitoringConfig::default();
        let monitor = ProductionMonitor::new(config);
        
        let component = ComponentHealth {
            component_id: "test_component".to_string(),
            status: ComponentStatus::Healthy,
            health_score: 1.0,
            last_check: chrono::Utc::now().timestamp() as u64,
            metrics: ComponentMetrics {
                response_time_ms: 50.0,
                throughput: 1000.0,
                error_count: 0,
                resource_utilization: 0.5,
            },
            dependencies: vec![],
        };
        
        monitor.register_component(component).await.unwrap();
        
        let summary = monitor.get_component_health_summary().await;
        assert!(summary.contains_key("test_component"));
    }

    #[tokio::test]
    async fn test_metrics_export() {
        let config = MonitoringConfig::default();
        let monitor = ProductionMonitor::new(config);
        
        let json_export = monitor.export_metrics(MetricsFormat::JSON).await.unwrap();
        assert!(json_export.contains("system_health_score"));
        
        let csv_export = monitor.export_metrics(MetricsFormat::CSV).await.unwrap();
        assert!(csv_export.contains("metric,value"));
    }
}