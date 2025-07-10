use eyre::Result;

pub mod production;
pub mod alerts;
pub mod health;
pub mod dashboard;

pub use production::*;
pub use alerts::*;
pub use health::*;
pub use dashboard::*;

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub enable_production_monitoring: bool,
    pub metrics_collection_interval: Duration,
    pub health_check_interval: Duration,
    pub alert_evaluation_interval: Duration,
    pub dashboard_refresh_interval: Duration,
    pub retention_period: Duration,
    pub enable_remote_export: bool,
    pub export_endpoints: Vec<String>,
    pub enable_real_time_alerts: bool,
    pub alert_channels: Vec<AlertChannel>,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            enable_production_monitoring: true,
            metrics_collection_interval: Duration::from_secs(10),
            health_check_interval: Duration::from_secs(30),
            alert_evaluation_interval: Duration::from_secs(60),
            dashboard_refresh_interval: Duration::from_secs(5),
            retention_period: Duration::from_secs(86400 * 7),
            enable_remote_export: false,
            export_endpoints: vec![],
            enable_real_time_alerts: true,
            alert_channels: vec![AlertChannel::Log],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertChannel {
    Log,
    Email { recipients: Vec<String> },
    Slack { webhook_url: String },
    Discord { webhook_url: String },
    HTTP { endpoint: String, headers: std::collections::HashMap<String, String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringMetrics {
    pub system_health_score: f64,
    pub total_components_monitored: usize,
    pub healthy_components: usize,
    pub degraded_components: usize,
    pub failed_components: usize,
    pub total_alerts_fired: u64,
    pub active_alerts: usize,
    pub sla_compliance: f64,
    pub average_response_time_ms: f64,
    pub throughput_ops_per_second: f64,
    pub error_rate: f64,
}

impl Default for MonitoringMetrics {
    fn default() -> Self {
        Self {
            system_health_score: 1.0,
            total_components_monitored: 0,
            healthy_components: 0,
            degraded_components: 0,
            failed_components: 0,
            total_alerts_fired: 0,
            active_alerts: 0,
            sla_compliance: 1.0,
            average_response_time_ms: 0.0,
            throughput_ops_per_second: 0.0,
            error_rate: 0.0,
        }
    }
}

pub trait MonitoringSystem: Send + Sync {
    async fn start_monitoring(&self) -> Result<()>;
    async fn stop_monitoring(&self) -> Result<()>;
    async fn get_system_health(&self) -> Result<f64>;
    async fn get_metrics(&self) -> Result<MonitoringMetrics>;
    async fn export_metrics(&self, format: MetricsFormat) -> Result<String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricsFormat {
    Prometheus,
    JSON,
    CSV,
    InfluxDB,
}