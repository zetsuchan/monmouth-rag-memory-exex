use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn, error};

use super::MonitoringConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub description: String,
    pub condition: String,
    pub severity: AlertSeverity,
    pub evaluation_window: Duration,
    pub cooldown: Duration,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
    Emergency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub rule_name: String,
    pub severity: AlertSeverity,
    pub message: String,
    pub details: HashMap<String, serde_json::Value>,
    pub timestamp: u64,
    pub resolved: bool,
    pub resolved_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertMetrics {
    pub total_alerts: u64,
    pub active_alerts: usize,
    pub alerts_by_severity: HashMap<String, u64>,
    pub alert_resolution_time_avg_seconds: f64,
    pub false_positive_rate: f64,
    pub escalations: u64,
}

impl Default for AlertMetrics {
    fn default() -> Self {
        Self {
            total_alerts: 0,
            active_alerts: 0,
            alerts_by_severity: HashMap::new(),
            alert_resolution_time_avg_seconds: 0.0,
            false_positive_rate: 0.0,
            escalations: 0,
        }
    }
}

#[derive(Debug)]
pub struct AlertManager {
    config: Arc<RwLock<MonitoringConfig>>,
    rules: Arc<RwLock<HashMap<String, AlertRule>>>,
    active_alerts: Arc<RwLock<HashMap<String, Alert>>>,
    alert_history: Arc<RwLock<VecDeque<Alert>>>,
    cooldown_tracker: Arc<RwLock<HashMap<String, Instant>>>,
    notification_channels: Arc<RwLock<Vec<Box<dyn AlertChannel + Send + Sync>>>>,
    metrics: Arc<RwLock<AlertMetrics>>,
    alert_sender: mpsc::Sender<Alert>,
    evaluation_active: Arc<RwLock<bool>>,
}

#[async_trait::async_trait]
pub trait AlertChannel: Send + Sync {
    async fn send_alert(&self, alert: &Alert) -> Result<()>;
    fn channel_type(&self) -> String;
}

#[derive(Debug)]
pub struct LogAlertChannel;

#[async_trait::async_trait]
impl AlertChannel for LogAlertChannel {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        match alert.severity {
            AlertSeverity::Emergency | AlertSeverity::Critical => {
                error!("ALERT [{}] {}: {}", alert.severity_string(), alert.rule_name, alert.message);
            }
            AlertSeverity::Warning => {
                warn!("ALERT [{}] {}: {}", alert.severity_string(), alert.rule_name, alert.message);
            }
            AlertSeverity::Info => {
                info!("ALERT [{}] {}: {}", alert.severity_string(), alert.rule_name, alert.message);
            }
        }
        Ok(())
    }

    fn channel_type(&self) -> String {
        "log".to_string()
    }
}

#[derive(Debug)]
pub struct SlackAlertChannel {
    webhook_url: String,
    client: reqwest::Client,
}

impl SlackAlertChannel {
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AlertChannel for SlackAlertChannel {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        let emoji = match alert.severity {
            AlertSeverity::Emergency => "🚨",
            AlertSeverity::Critical => "🔴",
            AlertSeverity::Warning => "🟡",
            AlertSeverity::Info => "🔵",
        };

        let color = match alert.severity {
            AlertSeverity::Emergency | AlertSeverity::Critical => "danger",
            AlertSeverity::Warning => "warning",
            AlertSeverity::Info => "good",
        };

        let payload = serde_json::json!({
            "text": format!("{} Alert: {}", emoji, alert.rule_name),
            "attachments": [{
                "color": color,
                "fields": [
                    {
                        "title": "Severity",
                        "value": alert.severity_string(),
                        "short": true
                    },
                    {
                        "title": "Message",
                        "value": alert.message,
                        "short": false
                    },
                    {
                        "title": "Timestamp",
                        "value": chrono::DateTime::from_timestamp(alert.timestamp as i64, 0)
                            .unwrap_or_default()
                            .format("%Y-%m-%d %H:%M:%S UTC")
                            .to_string(),
                        "short": true
                    }
                ]
            }]
        });

        let response = self.client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(eyre::eyre!("Failed to send Slack alert: {}", response.status()));
        }

        debug!("Sent alert to Slack: {}", alert.rule_name);
        Ok(())
    }

    fn channel_type(&self) -> String {
        "slack".to_string()
    }
}

#[derive(Debug)]
pub struct HttpAlertChannel {
    endpoint: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

impl HttpAlertChannel {
    pub fn new(endpoint: String, headers: HashMap<String, String>) -> Self {
        Self {
            endpoint,
            headers,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl AlertChannel for HttpAlertChannel {
    async fn send_alert(&self, alert: &Alert) -> Result<()> {
        let mut request = self.client.post(&self.endpoint);
        
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        let response = request
            .json(alert)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(eyre::eyre!("Failed to send HTTP alert: {}", response.status()));
        }

        debug!("Sent alert to HTTP endpoint: {}", alert.rule_name);
        Ok(())
    }

    fn channel_type(&self) -> String {
        "http".to_string()
    }
}

impl AlertManager {
    pub fn new(config: MonitoringConfig) -> Self {
        let (alert_sender, alert_receiver) = mpsc::channel(1000);
        
        let manager = Self {
            config: Arc::new(RwLock::new(config)),
            rules: Arc::new(RwLock::new(HashMap::new())),
            active_alerts: Arc::new(RwLock::new(HashMap::new())),
            alert_history: Arc::new(RwLock::new(VecDeque::new())),
            cooldown_tracker: Arc::new(RwLock::new(HashMap::new())),
            notification_channels: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(RwLock::new(AlertMetrics::default())),
            alert_sender,
            evaluation_active: Arc::new(RwLock::new(false)),
        };

        let processor = manager.clone();
        tokio::spawn(async move {
            processor.process_alerts(alert_receiver).await;
        });

        manager
    }

    pub async fn add_rule(&self, rule: AlertRule) -> Result<()> {
        info!("Adding alert rule: {}", rule.name);
        self.rules.write().await.insert(rule.name.clone(), rule);
        Ok(())
    }

    pub async fn remove_rule(&self, name: &str) -> Result<()> {
        info!("Removing alert rule: {}", name);
        self.rules.write().await.remove(name);
        Ok(())
    }

    pub async fn enable_rule(&self, name: &str) -> Result<()> {
        if let Some(rule) = self.rules.write().await.get_mut(name) {
            rule.enabled = true;
            info!("Enabled alert rule: {}", name);
        }
        Ok(())
    }

    pub async fn disable_rule(&self, name: &str) -> Result<()> {
        if let Some(rule) = self.rules.write().await.get_mut(name) {
            rule.enabled = false;
            info!("Disabled alert rule: {}", name);
        }
        Ok(())
    }

    pub async fn setup_default_channels(&self) -> Result<()> {
        let config = self.config.read().await;
        let mut channels = self.notification_channels.write().await;
        
        for channel_config in &config.alert_channels {
            match channel_config {
                super::AlertChannel::Log => {
                    channels.push(Box::new(LogAlertChannel));
                }
                super::AlertChannel::Slack { webhook_url } => {
                    channels.push(Box::new(SlackAlertChannel::new(webhook_url.clone())));
                }
                super::AlertChannel::HTTP { endpoint, headers } => {
                    channels.push(Box::new(HttpAlertChannel::new(endpoint.clone(), headers.clone())));
                }
                _ => {
                    warn!("Unsupported alert channel type: {:?}", channel_config);
                }
            }
        }

        info!("Configured {} alert channels", channels.len());
        Ok(())
    }

    pub async fn evaluate_metrics(&self, metrics: &super::production::ProductionMetrics) -> Result<()> {
        let rules = self.rules.read().await.clone();
        
        for (name, rule) in rules.iter() {
            if !rule.enabled {
                continue;
            }

            if self.is_in_cooldown(name).await {
                continue;
            }

            if self.evaluate_condition(rule, metrics).await? {
                self.trigger_alert(rule, metrics).await?;
            }
        }

        Ok(())
    }

    async fn is_in_cooldown(&self, rule_name: &str) -> bool {
        let cooldown_tracker = self.cooldown_tracker.read().await;
        
        if let Some(last_trigger) = cooldown_tracker.get(rule_name) {
            let rules = self.rules.read().await;
            if let Some(rule) = rules.get(rule_name) {
                return last_trigger.elapsed() < rule.cooldown;
            }
        }
        
        false
    }

    async fn evaluate_condition(
        &self,
        rule: &AlertRule,
        metrics: &super::production::ProductionMetrics,
    ) -> Result<bool> {
        let condition = &rule.condition;
        
        let result = if condition.contains("error_rate >") {
            let threshold = self.extract_threshold(condition, "error_rate >")?;
            metrics.error_rate > threshold
        } else if condition.contains("latency_p95_ms >") {
            let threshold = self.extract_threshold(condition, "latency_p95_ms >")?;
            metrics.latency_p95_ms > threshold
        } else if condition.contains("cpu_usage_percent >") {
            let threshold = self.extract_threshold(condition, "cpu_usage_percent >")?;
            metrics.cpu_usage_percent > threshold
        } else if condition.contains("memory_usage_mb >") {
            let threshold = self.extract_threshold(condition, "memory_usage_mb >")?;
            metrics.memory_usage_mb as f64 > threshold
        } else if condition.contains("availability_percentage <") {
            let threshold = self.extract_threshold(condition, "availability_percentage <")?;
            let availability = metrics.uptime_seconds as f64 / (24.0 * 3600.0) * 100.0;
            availability < threshold
        } else {
            false
        };

        debug!("Evaluated condition '{}' = {}", condition, result);
        Ok(result)
    }

    fn extract_threshold(&self, condition: &str, prefix: &str) -> Result<f64> {
        if let Some(threshold_str) = condition.strip_prefix(prefix) {
            threshold_str.trim().parse::<f64>()
                .map_err(|e| eyre::eyre!("Invalid threshold in condition: {}", e))
        } else {
            Err(eyre::eyre!("Invalid condition format: {}", condition))
        }
    }

    async fn trigger_alert(
        &self,
        rule: &AlertRule,
        metrics: &super::production::ProductionMetrics,
    ) -> Result<()> {
        let alert_id = uuid::Uuid::new_v4().to_string();
        
        let message = match rule.name.as_str() {
            "high_error_rate" => {
                format!("Error rate is {:.2}%, exceeding threshold", metrics.error_rate)
            }
            "high_latency" => {
                format!("P95 latency is {:.2}ms, exceeding threshold", metrics.latency_p95_ms)
            }
            "high_memory_usage" => {
                format!("Memory usage is {}MB, exceeding threshold", metrics.memory_usage_mb)
            }
            "low_availability" => {
                let availability = metrics.uptime_seconds as f64 / (24.0 * 3600.0) * 100.0;
                format!("System availability is {:.2}%, below SLA", availability)
            }
            _ => {
                format!("Alert condition met for rule: {}", rule.name)
            }
        };

        let mut details = HashMap::new();
        details.insert("error_rate".to_string(), serde_json::Value::Number(
            serde_json::Number::from_f64(metrics.error_rate).unwrap_or_default()
        ));
        details.insert("latency_p95_ms".to_string(), serde_json::Value::Number(
            serde_json::Number::from_f64(metrics.latency_p95_ms).unwrap_or_default()
        ));
        details.insert("memory_usage_mb".to_string(), serde_json::Value::Number(
            serde_json::Number::from(metrics.memory_usage_mb)
        ));
        details.insert("cpu_usage_percent".to_string(), serde_json::Value::Number(
            serde_json::Number::from_f64(metrics.cpu_usage_percent).unwrap_or_default()
        ));

        let alert = Alert {
            id: alert_id.clone(),
            rule_name: rule.name.clone(),
            severity: rule.severity.clone(),
            message,
            details,
            timestamp: chrono::Utc::now().timestamp() as u64,
            resolved: false,
            resolved_at: None,
        };

        self.active_alerts.write().await.insert(alert_id, alert.clone());
        
        self.cooldown_tracker.write().await.insert(rule.name.clone(), Instant::now());

        self.alert_sender.send(alert).await?;

        info!("Triggered alert: {} ({})", rule.name, rule.severity.to_string());
        Ok(())
    }

    async fn process_alerts(&self, mut receiver: mpsc::Receiver<Alert>) {
        while let Some(alert) = receiver.recv().await {
            if let Err(e) = self.send_notifications(&alert).await {
                error!("Failed to send alert notifications: {}", e);
            }

            self.update_metrics(&alert).await;
            self.store_alert_history(alert).await;
        }
    }

    async fn send_notifications(&self, alert: &Alert) -> Result<()> {
        let channels = self.notification_channels.read().await;
        
        for channel in channels.iter() {
            if let Err(e) = channel.send_alert(alert).await {
                error!("Failed to send alert via {}: {}", channel.channel_type(), e);
            }
        }

        Ok(())
    }

    async fn update_metrics(&self, alert: &Alert) {
        let mut metrics = self.metrics.write().await;
        metrics.total_alerts += 1;
        
        let severity_key = alert.severity.to_string();
        *metrics.alerts_by_severity.entry(severity_key).or_insert(0) += 1;
        
        metrics.active_alerts = self.active_alerts.read().await.len();
    }

    async fn store_alert_history(&self, alert: Alert) {
        let mut history = self.alert_history.write().await;
        history.push_back(alert);

        if history.len() > 10000 {
            history.pop_front();
        }
    }

    pub async fn resolve_alert(&self, alert_id: &str, resolution_message: Option<String>) -> Result<()> {
        let mut active_alerts = self.active_alerts.write().await;
        
        if let Some(mut alert) = active_alerts.remove(alert_id) {
            alert.resolved = true;
            alert.resolved_at = Some(chrono::Utc::now().timestamp() as u64);
            
            if let Some(message) = resolution_message {
                alert.details.insert("resolution".to_string(), serde_json::Value::String(message));
            }

            info!("Resolved alert: {} ({})", alert.rule_name, alert.id);
            
            let mut history = self.alert_history.write().await;
            history.push_back(alert);
        }

        Ok(())
    }

    pub async fn get_active_alerts(&self) -> Vec<Alert> {
        self.active_alerts.read().await.values().cloned().collect()
    }

    pub async fn get_alert_history(&self, limit: Option<usize>) -> Vec<Alert> {
        let history = self.alert_history.read().await;
        let take_count = limit.unwrap_or(history.len());
        
        history.iter()
            .rev()
            .take(take_count)
            .cloned()
            .collect()
    }

    pub async fn get_total_alerts_fired(&self) -> u64 {
        self.metrics.read().await.total_alerts
    }

    pub async fn get_active_alerts_count(&self) -> usize {
        self.active_alerts.read().await.len()
    }

    pub async fn get_alert_metrics(&self) -> AlertMetrics {
        let mut metrics = self.metrics.read().await.clone();
        metrics.active_alerts = self.active_alerts.read().await.len();
        metrics
    }

    pub async fn start(&self) -> Result<()> {
        *self.evaluation_active.write().await = true;
        self.setup_default_channels().await?;
        info!("Alert manager started");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        *self.evaluation_active.write().await = false;
        info!("Alert manager stopped");
        Ok(())
    }
}

impl Clone for AlertManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            rules: self.rules.clone(),
            active_alerts: self.active_alerts.clone(),
            alert_history: self.alert_history.clone(),
            cooldown_tracker: self.cooldown_tracker.clone(),
            notification_channels: self.notification_channels.clone(),
            metrics: self.metrics.clone(),
            alert_sender: self.alert_sender.clone(),
            evaluation_active: self.evaluation_active.clone(),
        }
    }
}

impl Alert {
    pub fn severity_string(&self) -> String {
        self.severity.to_string()
    }
}

impl AlertSeverity {
    pub fn to_string(&self) -> String {
        match self {
            AlertSeverity::Info => "INFO".to_string(),
            AlertSeverity::Warning => "WARNING".to_string(),
            AlertSeverity::Critical => "CRITICAL".to_string(),
            AlertSeverity::Emergency => "EMERGENCY".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_alert_manager_creation() {
        let config = MonitoringConfig::default();
        let manager = AlertManager::new(config);
        
        let metrics = manager.get_alert_metrics().await;
        assert_eq!(metrics.total_alerts, 0);
    }

    #[tokio::test]
    async fn test_alert_rule_management() {
        let config = MonitoringConfig::default();
        let manager = AlertManager::new(config);
        
        let rule = AlertRule {
            name: "test_rule".to_string(),
            description: "Test rule".to_string(),
            condition: "error_rate > 5.0".to_string(),
            severity: AlertSeverity::Warning,
            evaluation_window: Duration::from_secs(300),
            cooldown: Duration::from_secs(600),
            enabled: true,
        };
        
        manager.add_rule(rule).await.unwrap();
        
        let rules = manager.rules.read().await;
        assert!(rules.contains_key("test_rule"));
    }

    #[tokio::test]
    async fn test_alert_resolution() {
        let config = MonitoringConfig::default();
        let manager = AlertManager::new(config);
        
        let alert = Alert {
            id: "test_alert".to_string(),
            rule_name: "test_rule".to_string(),
            severity: AlertSeverity::Warning,
            message: "Test alert".to_string(),
            details: HashMap::new(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            resolved: false,
            resolved_at: None,
        };
        
        manager.active_alerts.write().await.insert("test_alert".to_string(), alert);
        
        manager.resolve_alert("test_alert", Some("Manual resolution".to_string())).await.unwrap();
        
        let active_alerts = manager.get_active_alerts().await;
        assert!(active_alerts.is_empty());
    }
}