use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use serde_json::json;

use super::{
    MonitoringConfig, MonitoringMetrics, 
    production::{ProductionMetrics, SLAMetrics, ComponentHealth},
    alerts::{Alert, AlertMetrics},
    health::{HealthMetrics, HealthCheckResult},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub refresh_interval: Duration,
    pub data_retention: Duration,
    pub max_chart_points: usize,
    pub enable_real_time_updates: bool,
    pub enable_export: bool,
    pub export_formats: Vec<String>,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_interval: Duration::from_secs(5),
            data_retention: Duration::from_secs(3600),
            max_chart_points: 300,
            enable_real_time_updates: true,
            enable_export: true,
            export_formats: vec!["json".to_string(), "csv".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub timestamp: u64,
    pub system_overview: SystemOverview,
    pub performance_metrics: Vec<TimeSeriesPoint>,
    pub component_status: HashMap<String, ComponentStatus>,
    pub active_alerts: Vec<Alert>,
    pub health_summary: HealthSummary,
    pub sla_metrics: SLAMetrics,
    pub trend_analysis: TrendAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOverview {
    pub health_score: f64,
    pub uptime_seconds: u64,
    pub total_requests: u64,
    pub success_rate: f64,
    pub avg_response_time: f64,
    pub active_components: usize,
    pub critical_alerts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub name: String,
    pub status: String,
    pub health_score: f64,
    pub last_check: u64,
    pub response_time: f64,
    pub error_count: u64,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: u64,
    pub values: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSummary {
    pub total_components: usize,
    pub healthy: usize,
    pub degraded: usize,
    pub unhealthy: usize,
    pub unknown: usize,
    pub average_response_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    pub performance_trend: String,
    pub health_trend: String,
    pub error_trend: String,
    pub predictions: HashMap<String, f64>,
}

#[derive(Debug)]
pub struct MetricsDashboard {
    config: Arc<RwLock<DashboardConfig>>,
    data_history: Arc<RwLock<Vec<DashboardData>>>,
    current_data: Arc<RwLock<Option<DashboardData>>>,
    dashboard_active: Arc<RwLock<bool>>,
    start_time: Instant,
}

impl MetricsDashboard {
    pub fn new(config: DashboardConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            data_history: Arc::new(RwLock::new(Vec::new())),
            current_data: Arc::new(RwLock::new(None)),
            dashboard_active: Arc::new(RwLock::new(false)),
            start_time: Instant::now(),
        }
    }

    pub async fn update_dashboard_data(
        &self,
        monitoring_metrics: MonitoringMetrics,
        production_metrics: Option<ProductionMetrics>,
        alert_metrics: AlertMetrics,
        health_metrics: HealthMetrics,
        sla_metrics: SLAMetrics,
        active_alerts: Vec<Alert>,
        component_health: HashMap<String, ComponentHealth>,
    ) -> Result<()> {
        let dashboard_data = self.create_dashboard_data(
            monitoring_metrics,
            production_metrics,
            alert_metrics,
            health_metrics,
            sla_metrics,
            active_alerts,
            component_health,
        ).await?;

        *self.current_data.write().await = Some(dashboard_data.clone());
        
        let mut history = self.data_history.write().await;
        history.push(dashboard_data);

        let config = self.config.read().await;
        let retention_points = (config.data_retention.as_secs() / config.refresh_interval.as_secs()) as usize;
        
        if history.len() > retention_points {
            history.remove(0);
        }

        Ok(())
    }

    async fn create_dashboard_data(
        &self,
        monitoring_metrics: MonitoringMetrics,
        production_metrics: Option<ProductionMetrics>,
        alert_metrics: AlertMetrics,
        health_metrics: HealthMetrics,
        sla_metrics: SLAMetrics,
        active_alerts: Vec<Alert>,
        component_health: HashMap<String, ComponentHealth>,
    ) -> Result<DashboardData> {
        let timestamp = chrono::Utc::now().timestamp() as u64;
        
        let system_overview = SystemOverview {
            health_score: monitoring_metrics.system_health_score,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            total_requests: if let Some(ref pm) = production_metrics {
                pm.timestamp
            } else {
                0
            },
            success_rate: 100.0 - monitoring_metrics.error_rate,
            avg_response_time: monitoring_metrics.average_response_time_ms,
            active_components: monitoring_metrics.total_components_monitored,
            critical_alerts: active_alerts.iter()
                .filter(|a| matches!(a.severity, super::alerts::AlertSeverity::Critical | super::alerts::AlertSeverity::Emergency))
                .count(),
        };

        let performance_metrics = self.create_performance_time_series(&production_metrics).await;
        
        let component_status = self.create_component_status_map(component_health).await;
        
        let health_summary = HealthSummary {
            total_components: health_metrics.total_components,
            healthy: health_metrics.healthy_components,
            degraded: health_metrics.degraded_components,
            unhealthy: health_metrics.unhealthy_components,
            unknown: health_metrics.unknown_components,
            average_response_time: health_metrics.average_response_time_ms,
        };

        let trend_analysis = self.analyze_trends().await;

        Ok(DashboardData {
            timestamp,
            system_overview,
            performance_metrics,
            component_status,
            active_alerts,
            health_summary,
            sla_metrics,
            trend_analysis,
        })
    }

    async fn create_performance_time_series(&self, production_metrics: &Option<ProductionMetrics>) -> Vec<TimeSeriesPoint> {
        if let Some(pm) = production_metrics {
            let mut values = HashMap::new();
            values.insert("cpu_usage".to_string(), pm.cpu_usage_percent);
            values.insert("memory_usage".to_string(), pm.memory_usage_mb as f64);
            values.insert("request_rate".to_string(), pm.request_rate);
            values.insert("error_rate".to_string(), pm.error_rate);
            values.insert("latency_p95".to_string(), pm.latency_p95_ms);
            values.insert("active_connections".to_string(), pm.active_connections as f64);

            vec![TimeSeriesPoint {
                timestamp: pm.timestamp,
                values,
            }]
        } else {
            vec![]
        }
    }

    async fn create_component_status_map(&self, component_health: HashMap<String, ComponentHealth>) -> HashMap<String, ComponentStatus> {
        component_health.into_iter()
            .map(|(name, health)| {
                let status = ComponentStatus {
                    name: name.clone(),
                    status: format!("{:?}", health.status),
                    health_score: health.health_score,
                    last_check: health.last_check,
                    response_time: health.metrics.response_time_ms,
                    error_count: health.metrics.error_count,
                    dependencies: health.dependencies,
                };
                (name, status)
            })
            .collect()
    }

    async fn analyze_trends(&self) -> TrendAnalysis {
        let history = self.data_history.read().await;
        
        if history.len() < 2 {
            return TrendAnalysis {
                performance_trend: "stable".to_string(),
                health_trend: "stable".to_string(),
                error_trend: "stable".to_string(),
                predictions: HashMap::new(),
            };
        }

        let recent_data = &history[history.len() - 10..];
        
        let performance_trend = self.calculate_trend(
            recent_data.iter().map(|d| d.system_overview.avg_response_time).collect()
        );
        
        let health_trend = self.calculate_trend(
            recent_data.iter().map(|d| d.system_overview.health_score).collect()
        );
        
        let error_rates: Vec<f64> = recent_data.iter()
            .map(|d| 100.0 - d.system_overview.success_rate)
            .collect();
        let error_trend = self.calculate_trend(error_rates);

        let mut predictions = HashMap::new();
        predictions.insert("health_score_5min".to_string(), 
            self.predict_next_value(recent_data.iter().map(|d| d.system_overview.health_score).collect()));
        predictions.insert("response_time_5min".to_string(), 
            self.predict_next_value(recent_data.iter().map(|d| d.system_overview.avg_response_time).collect()));

        TrendAnalysis {
            performance_trend,
            health_trend,
            error_trend,
            predictions,
        }
    }

    fn calculate_trend(&self, values: Vec<f64>) -> String {
        if values.len() < 2 {
            return "stable".to_string();
        }

        let first_half = &values[..values.len()/2];
        let second_half = &values[values.len()/2..];

        let first_avg = first_half.iter().sum::<f64>() / first_half.len() as f64;
        let second_avg = second_half.iter().sum::<f64>() / second_half.len() as f64;

        let change = (second_avg - first_avg) / first_avg * 100.0;

        if change > 5.0 {
            "improving".to_string()
        } else if change < -5.0 {
            "degrading".to_string()
        } else {
            "stable".to_string()
        }
    }

    fn predict_next_value(&self, values: Vec<f64>) -> f64 {
        if values.len() < 2 {
            return values.last().copied().unwrap_or(0.0);
        }

        let recent_values = &values[values.len().saturating_sub(5)..];
        let sum: f64 = recent_values.iter().sum();
        let avg = sum / recent_values.len() as f64;

        if recent_values.len() >= 3 {
            let trend = (recent_values[recent_values.len()-1] - recent_values[0]) / (recent_values.len() - 1) as f64;
            avg + trend
        } else {
            avg
        }
    }

    pub async fn get_current_dashboard(&self) -> Option<DashboardData> {
        self.current_data.read().await.clone()
    }

    pub async fn get_dashboard_history(&self, duration: Duration) -> Vec<DashboardData> {
        let history = self.data_history.read().await;
        let cutoff = chrono::Utc::now().timestamp() as u64 - duration.as_secs();
        
        history.iter()
            .filter(|data| data.timestamp >= cutoff)
            .cloned()
            .collect()
    }

    pub async fn export_dashboard_data(&self, format: &str) -> Result<String> {
        let current_data = self.get_current_dashboard().await
            .ok_or_else(|| eyre::eyre!("No dashboard data available"))?;

        match format.to_lowercase().as_str() {
            "json" => {
                Ok(serde_json::to_string_pretty(&current_data)?)
            }
            "csv" => {
                Ok(self.export_to_csv(&current_data).await)
            }
            "html" => {
                Ok(self.export_to_html(&current_data).await)
            }
            _ => {
                Err(eyre::eyre!("Unsupported export format: {}", format))
            }
        }
    }

    async fn export_to_csv(&self, data: &DashboardData) -> String {
        let mut csv = String::new();
        
        csv.push_str("metric,value\n");
        csv.push_str(&format!("timestamp,{}\n", data.timestamp));
        csv.push_str(&format!("health_score,{}\n", data.system_overview.health_score));
        csv.push_str(&format!("uptime_seconds,{}\n", data.system_overview.uptime_seconds));
        csv.push_str(&format!("success_rate,{}\n", data.system_overview.success_rate));
        csv.push_str(&format!("avg_response_time,{}\n", data.system_overview.avg_response_time));
        csv.push_str(&format!("active_components,{}\n", data.system_overview.active_components));
        csv.push_str(&format!("critical_alerts,{}\n", data.system_overview.critical_alerts));
        csv.push_str(&format!("healthy_components,{}\n", data.health_summary.healthy));
        csv.push_str(&format!("degraded_components,{}\n", data.health_summary.degraded));
        csv.push_str(&format!("unhealthy_components,{}\n", data.health_summary.unhealthy));
        csv.push_str(&format!("sla_availability,{}\n", data.sla_metrics.availability_percentage));
        csv.push_str(&format!("error_budget_remaining,{}\n", data.sla_metrics.error_budget_remaining));

        csv
    }

    async fn export_to_html(&self, data: &DashboardData) -> String {
        format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <title>Monmouth ExEx Dashboard</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        .metric {{ margin: 10px 0; padding: 10px; border: 1px solid #ccc; border-radius: 5px; }}
        .healthy {{ background-color: #d4edda; }}
        .warning {{ background-color: #fff3cd; }}
        .critical {{ background-color: #f8d7da; }}
        .header {{ font-size: 24px; font-weight: bold; margin-bottom: 20px; }}
        .section {{ margin: 20px 0; }}
        .section-title {{ font-size: 18px; font-weight: bold; margin-bottom: 10px; }}
    </style>
</head>
<body>
    <div class="header">Monmouth ExEx Monitoring Dashboard</div>
    <div class="metric">Generated at: {}</div>
    
    <div class="section">
        <div class="section-title">System Overview</div>
        <div class="metric {}">Health Score: {:.2}%</div>
        <div class="metric">Uptime: {} seconds</div>
        <div class="metric">Success Rate: {:.2}%</div>
        <div class="metric">Average Response Time: {:.2}ms</div>
        <div class="metric">Active Components: {}</div>
        <div class="metric {}">Critical Alerts: {}</div>
    </div>
    
    <div class="section">
        <div class="section-title">Component Health</div>
        <div class="metric healthy">Healthy: {}</div>
        <div class="metric warning">Degraded: {}</div>
        <div class="metric critical">Unhealthy: {}</div>
        <div class="metric">Unknown: {}</div>
    </div>
    
    <div class="section">
        <div class="section-title">SLA Metrics</div>
        <div class="metric">Availability: {:.2}%</div>
        <div class="metric">Error Budget Remaining: {:.2}%</div>
        <div class="metric">MTTR: {:.2} minutes</div>
        <div class="metric">MTBF: {:.2} hours</div>
    </div>
    
    <div class="section">
        <div class="section-title">Trends</div>
        <div class="metric">Performance Trend: {}</div>
        <div class="metric">Health Trend: {}</div>
        <div class="metric">Error Trend: {}</div>
    </div>
</body>
</html>"#,
            chrono::DateTime::from_timestamp(data.timestamp as i64, 0).unwrap_or_default(),
            if data.system_overview.health_score > 0.8 { "healthy" } else if data.system_overview.health_score > 0.5 { "warning" } else { "critical" },
            data.system_overview.health_score * 100.0,
            data.system_overview.uptime_seconds,
            data.system_overview.success_rate,
            data.system_overview.avg_response_time,
            data.system_overview.active_components,
            if data.system_overview.critical_alerts == 0 { "healthy" } else { "critical" },
            data.system_overview.critical_alerts,
            data.health_summary.healthy,
            data.health_summary.degraded,
            data.health_summary.unhealthy,
            data.health_summary.unknown,
            data.sla_metrics.availability_percentage,
            data.sla_metrics.error_budget_remaining * 100.0,
            data.sla_metrics.mttr_minutes,
            data.sla_metrics.mtbf_hours,
            data.trend_analysis.performance_trend,
            data.trend_analysis.health_trend,
            data.trend_analysis.error_trend,
        )
    }

    pub async fn get_performance_chart_data(&self, metric: &str, duration: Duration) -> Vec<(u64, f64)> {
        let history = self.get_dashboard_history(duration).await;
        
        history.iter()
            .filter_map(|data| {
                if let Some(point) = data.performance_metrics.first() {
                    point.values.get(metric).map(|value| (data.timestamp, *value))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn get_component_health_chart(&self) -> HashMap<String, usize> {
        if let Some(data) = self.get_current_dashboard().await {
            let mut chart_data = HashMap::new();
            chart_data.insert("Healthy".to_string(), data.health_summary.healthy);
            chart_data.insert("Degraded".to_string(), data.health_summary.degraded);
            chart_data.insert("Unhealthy".to_string(), data.health_summary.unhealthy);
            chart_data.insert("Unknown".to_string(), data.health_summary.unknown);
            chart_data
        } else {
            HashMap::new()
        }
    }

    pub async fn get_alert_summary(&self) -> HashMap<String, usize> {
        if let Some(data) = self.get_current_dashboard().await {
            let mut summary = HashMap::new();
            
            for alert in &data.active_alerts {
                let severity = alert.severity.to_string();
                *summary.entry(severity).or_insert(0) += 1;
            }
            
            summary
        } else {
            HashMap::new()
        }
    }

    pub async fn start(&self) -> Result<()> {
        *self.dashboard_active.write().await = true;
        info!("Metrics dashboard started");
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        *self.dashboard_active.write().await = false;
        info!("Metrics dashboard stopped");
        Ok(())
    }
}

impl Clone for MetricsDashboard {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            data_history: self.data_history.clone(),
            current_data: self.current_data.clone(),
            dashboard_active: self.dashboard_active.clone(),
            start_time: self.start_time,
        }
    }
}

#[derive(Debug)]
pub struct DashboardServer {
    dashboard: Arc<MetricsDashboard>,
    server_port: u16,
    server_active: Arc<RwLock<bool>>,
}

impl DashboardServer {
    pub fn new(dashboard: Arc<MetricsDashboard>, port: u16) -> Self {
        Self {
            dashboard,
            server_port: port,
            server_active: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn start_server(&self) -> Result<()> {
        if *self.server_active.read().await {
            return Err(eyre::eyre!("Dashboard server is already running"));
        }

        *self.server_active.write().await = true;
        
        let app = self.create_router().await;
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.server_port)).await?;
        
        info!("Dashboard server starting on port {}", self.server_port);
        
        let dashboard_clone = self.dashboard.clone();
        let server_active = self.server_active.clone();
        
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("Dashboard server error: {}", e);
                *server_active.write().await = false;
            }
        });
        
        info!("Dashboard server started successfully on port {}", self.server_port);
        
        Ok(())
    }

    async fn create_router(&self) -> Router {
        Router::new()
            .route("/", get(Self::dashboard_home))
            .route("/health", get(Self::health_check))
            .route("/metrics", get(Self::metrics_endpoint))
            .route("/api/dashboard", get(Self::api_dashboard_data))
            .route("/api/dashboard/export/:format", get(Self::api_export_data))
            .route("/api/performance/:metric", get(Self::api_performance_data))
            .route("/api/components", get(Self::api_component_status))
            .route("/api/alerts", get(Self::api_active_alerts))
            .route("/api/health", get(Self::api_health_summary))
            .route("/ws/dashboard", get(Self::websocket_handler))
            .layer(CorsLayer::permissive())
            .with_state(self.dashboard.clone())
    }

    async fn dashboard_home(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Monmouth ExEx Dashboard</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body { font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif; margin: 0; padding: 20px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; }
        .header { background: #2c3e50; color: white; padding: 20px; border-radius: 8px; margin-bottom: 20px; }
        .metric-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 20px; }
        .metric-card { background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .metric-value { font-size: 2em; font-weight: bold; color: #27ae60; }
        .metric-label { color: #7f8c8d; margin-bottom: 10px; }
        .status-good { color: #27ae60; }
        .status-warning { color: #f39c12; }
        .status-critical { color: #e74c3c; }
        .refresh-btn { background: #3498db; color: white; border: none; padding: 10px 20px; border-radius: 4px; cursor: pointer; }
        .nav { margin-bottom: 20px; }
        .nav a { margin-right: 15px; text-decoration: none; color: #3498db; }
    </style>
    <script>
        async function loadDashboardData() {
            try {
                const response = await fetch('/api/dashboard');
                const data = await response.json();
                updateDashboard(data);
            } catch (error) {
                console.error('Failed to load dashboard data:', error);
            }
        }
        
        function updateDashboard(data) {
            document.getElementById('health-score').textContent = (data.system_overview.health_score * 100).toFixed(1) + '%';
            document.getElementById('uptime').textContent = formatUptime(data.system_overview.uptime_seconds);
            document.getElementById('success-rate').textContent = data.system_overview.success_rate.toFixed(1) + '%';
            document.getElementById('response-time').textContent = data.system_overview.avg_response_time.toFixed(1) + 'ms';
            document.getElementById('active-components').textContent = data.system_overview.active_components;
            document.getElementById('critical-alerts').textContent = data.system_overview.critical_alerts;
            
            document.getElementById('healthy-components').textContent = data.health_summary.healthy;
            document.getElementById('degraded-components').textContent = data.health_summary.degraded;
            document.getElementById('unhealthy-components').textContent = data.health_summary.unhealthy;
            
            document.getElementById('last-updated').textContent = new Date().toLocaleTimeString();
        }
        
        function formatUptime(seconds) {
            const hours = Math.floor(seconds / 3600);
            const minutes = Math.floor((seconds % 3600) / 60);
            return `${hours}h ${minutes}m`;
        }
        
        function autoRefresh() {
            loadDashboardData();
            setTimeout(autoRefresh, 5000); // Refresh every 5 seconds
        }
        
        window.onload = function() {
            loadDashboardData();
            autoRefresh();
        };
    </script>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>Monmouth ExEx Monitoring Dashboard</h1>
            <p>Real-time monitoring and performance metrics</p>
        </div>
        
        <div class="nav">
            <a href="/">Dashboard</a>
            <a href="/health">Health Check</a>
            <a href="/metrics">Prometheus Metrics</a>
            <a href="/api/dashboard/export/json">Export JSON</a>
            <a href="/api/dashboard/export/csv">Export CSV</a>
            <button class="refresh-btn" onclick="loadDashboardData()">Refresh Now</button>
            <span style="float: right;">Last updated: <span id="last-updated">Loading...</span></span>
        </div>
        
        <div class="metric-grid">
            <div class="metric-card">
                <div class="metric-label">System Health Score</div>
                <div class="metric-value status-good" id="health-score">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">System Uptime</div>
                <div class="metric-value" id="uptime">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Success Rate</div>
                <div class="metric-value status-good" id="success-rate">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Average Response Time</div>
                <div class="metric-value" id="response-time">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Active Components</div>
                <div class="metric-value" id="active-components">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Critical Alerts</div>
                <div class="metric-value status-critical" id="critical-alerts">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Healthy Components</div>
                <div class="metric-value status-good" id="healthy-components">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Degraded Components</div>
                <div class="metric-value status-warning" id="degraded-components">--</div>
            </div>
            
            <div class="metric-card">
                <div class="metric-label">Unhealthy Components</div>
                <div class="metric-value status-critical" id="unhealthy-components">--</div>
            </div>
        </div>
    </div>
</body>
</html>
        "#;
        Html(html)
    }

    async fn health_check(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        if let Some(data) = dashboard.get_current_dashboard().await {
            let health = json!({
                "status": "healthy",
                "health_score": data.system_overview.health_score,
                "uptime_seconds": data.system_overview.uptime_seconds,
                "active_components": data.system_overview.active_components,
                "critical_alerts": data.system_overview.critical_alerts,
                "timestamp": data.timestamp
            });
            (StatusCode::OK, Json(health))
        } else {
            let health = json!({
                "status": "initializing",
                "message": "Dashboard data not yet available"
            });
            (StatusCode::SERVICE_UNAVAILABLE, Json(health))
        }
    }

    async fn metrics_endpoint(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        if let Some(data) = dashboard.get_current_dashboard().await {
            let prometheus_metrics = format!(
                "# HELP monmouth_system_health_score Overall system health score\n\
                 # TYPE monmouth_system_health_score gauge\n\
                 monmouth_system_health_score {}\n\
                 # HELP monmouth_uptime_seconds System uptime in seconds\n\
                 # TYPE monmouth_uptime_seconds counter\n\
                 monmouth_uptime_seconds {}\n\
                 # HELP monmouth_success_rate Request success rate percentage\n\
                 # TYPE monmouth_success_rate gauge\n\
                 monmouth_success_rate {}\n\
                 # HELP monmouth_response_time_ms Average response time in milliseconds\n\
                 # TYPE monmouth_response_time_ms gauge\n\
                 monmouth_response_time_ms {}\n\
                 # HELP monmouth_active_components Number of active components\n\
                 # TYPE monmouth_active_components gauge\n\
                 monmouth_active_components {}\n\
                 # HELP monmouth_critical_alerts Number of critical alerts\n\
                 # TYPE monmouth_critical_alerts gauge\n\
                 monmouth_critical_alerts {}\n\
                 # HELP monmouth_healthy_components Number of healthy components\n\
                 # TYPE monmouth_healthy_components gauge\n\
                 monmouth_healthy_components {}\n\
                 # HELP monmouth_degraded_components Number of degraded components\n\
                 # TYPE monmouth_degraded_components gauge\n\
                 monmouth_degraded_components {}\n\
                 # HELP monmouth_unhealthy_components Number of unhealthy components\n\
                 # TYPE monmouth_unhealthy_components gauge\n\
                 monmouth_unhealthy_components {}\n",
                data.system_overview.health_score,
                data.system_overview.uptime_seconds,
                data.system_overview.success_rate,
                data.system_overview.avg_response_time,
                data.system_overview.active_components,
                data.system_overview.critical_alerts,
                data.health_summary.healthy,
                data.health_summary.degraded,
                data.health_summary.unhealthy,
            );
            (StatusCode::OK, prometheus_metrics)
        } else {
            (StatusCode::SERVICE_UNAVAILABLE, "# Metrics not available yet\n".to_string())
        }
    }

    async fn api_dashboard_data(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        match dashboard.get_current_dashboard().await {
            Some(data) => (StatusCode::OK, Json(data)),
            None => {
                let error = json!({"error": "Dashboard data not available"});
                (StatusCode::SERVICE_UNAVAILABLE, Json(error))
            }
        }
    }

    async fn api_export_data(
        Path(format): Path<String>,
        State(dashboard): State<Arc<MetricsDashboard>>
    ) -> impl IntoResponse {
        match dashboard.export_dashboard_data(&format).await {
            Ok(data) => {
                let content_type = match format.as_str() {
                    "json" => "application/json",
                    "csv" => "text/csv",
                    "html" => "text/html",
                    _ => "text/plain",
                };
                (StatusCode::OK, [(axum::http::header::CONTENT_TYPE, content_type)], data)
            }
            Err(e) => {
                let error = format!("Export failed: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, [(axum::http::header::CONTENT_TYPE, "text/plain")], error)
            }
        }
    }

    async fn api_performance_data(
        Path(metric): Path<String>,
        Query(params): Query<HashMap<String, String>>,
        State(dashboard): State<Arc<MetricsDashboard>>
    ) -> impl IntoResponse {
        let duration = params.get("duration")
            .and_then(|d| d.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(3600));
            
        let data = dashboard.get_performance_chart_data(&metric, duration).await;
        (StatusCode::OK, Json(data))
    }

    async fn api_component_status(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        if let Some(data) = dashboard.get_current_dashboard().await {
            (StatusCode::OK, Json(data.component_status))
        } else {
            let error = json!({"error": "Component status not available"});
            (StatusCode::SERVICE_UNAVAILABLE, Json(error))
        }
    }

    async fn api_active_alerts(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        if let Some(data) = dashboard.get_current_dashboard().await {
            (StatusCode::OK, Json(data.active_alerts))
        } else {
            let error = json!({"error": "Alert data not available"});
            (StatusCode::SERVICE_UNAVAILABLE, Json(error))
        }
    }

    async fn api_health_summary(State(dashboard): State<Arc<MetricsDashboard>>) -> impl IntoResponse {
        if let Some(data) = dashboard.get_current_dashboard().await {
            (StatusCode::OK, Json(data.health_summary))
        } else {
            let error = json!({"error": "Health summary not available"});
            (StatusCode::SERVICE_UNAVAILABLE, Json(error))
        }
    }

    async fn websocket_handler() -> impl IntoResponse {
        StatusCode::NOT_IMPLEMENTED
    }

    pub async fn stop_server(&self) -> Result<()> {
        *self.server_active.write().await = false;
        info!("Dashboard server stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dashboard_creation() {
        let config = DashboardConfig::default();
        let dashboard = MetricsDashboard::new(config);
        
        let current = dashboard.get_current_dashboard().await;
        assert!(current.is_none());
    }

    #[tokio::test]
    async fn test_trend_calculation() {
        let config = DashboardConfig::default();
        let dashboard = MetricsDashboard::new(config);
        
        let improving_values = vec![10.0, 15.0, 20.0, 25.0, 30.0];
        let trend = dashboard.calculate_trend(improving_values);
        assert_eq!(trend, "improving");
        
        let degrading_values = vec![30.0, 25.0, 20.0, 15.0, 10.0];
        let trend = dashboard.calculate_trend(degrading_values);
        assert_eq!(trend, "degrading");
        
        let stable_values = vec![20.0, 19.0, 21.0, 20.0, 20.0];
        let trend = dashboard.calculate_trend(stable_values);
        assert_eq!(trend, "stable");
    }

    #[tokio::test]
    async fn test_prediction() {
        let config = DashboardConfig::default();
        let dashboard = MetricsDashboard::new(config);
        
        let values = vec![10.0, 12.0, 14.0, 16.0, 18.0];
        let prediction = dashboard.predict_next_value(values);
        assert!(prediction > 18.0);
    }

    #[tokio::test]
    async fn test_export_formats() {
        let config = DashboardConfig::default();
        let dashboard = MetricsDashboard::new(config);
        
        let test_data = DashboardData {
            timestamp: chrono::Utc::now().timestamp() as u64,
            system_overview: SystemOverview {
                health_score: 0.95,
                uptime_seconds: 3600,
                total_requests: 1000,
                success_rate: 99.5,
                avg_response_time: 50.0,
                active_components: 5,
                critical_alerts: 0,
            },
            performance_metrics: vec![],
            component_status: HashMap::new(),
            active_alerts: vec![],
            health_summary: HealthSummary {
                total_components: 5,
                healthy: 5,
                degraded: 0,
                unhealthy: 0,
                unknown: 0,
                average_response_time: 30.0,
            },
            sla_metrics: SLAMetrics {
                availability_percentage: 99.9,
                performance_target_met: true,
                error_budget_remaining: 0.8,
                mttr_minutes: 5.0,
                mtbf_hours: 48.0,
            },
            trend_analysis: TrendAnalysis {
                performance_trend: "stable".to_string(),
                health_trend: "improving".to_string(),
                error_trend: "stable".to_string(),
                predictions: HashMap::new(),
            },
        };
        
        *dashboard.current_data.write().await = Some(test_data);
        
        let json_export = dashboard.export_dashboard_data("json").await.unwrap();
        assert!(json_export.contains("health_score"));
        
        let csv_export = dashboard.export_dashboard_data("csv").await.unwrap();
        assert!(csv_export.contains("metric,value"));
        
        let html_export = dashboard.export_dashboard_data("html").await.unwrap();
        assert!(html_export.contains("<html>"));
    }
}