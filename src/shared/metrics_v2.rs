use alloy::primitives::{Address, B256};
use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec,
    IntCounter, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::ai_agent::RoutingDecision;
use super::agent_state_manager::LifecycleState;

lazy_static::lazy_static! {
    // AI Decision Metrics
    static ref AI_DECISION_TOTAL: IntCounterVec = IntCounterVec::new(
        Opts::new("ai_decision_total", "Total AI routing decisions"),
        &["model", "decision"]
    ).unwrap();

    static ref AI_DECISION_ACCURACY: GaugeVec = GaugeVec::new(
        Opts::new("ai_decision_accuracy", "AI decision accuracy by model"),
        &["model"]
    ).unwrap();

    static ref AI_CONFIDENCE_HISTOGRAM: HistogramVec = HistogramVec::new(
        HistogramOpts::new("ai_confidence_score", "AI confidence score distribution"),
        &["model", "decision"]
    ).unwrap();

    static ref AI_PREDICTION_ERROR: HistogramVec = HistogramVec::new(
        HistogramOpts::new("ai_prediction_error", "Prediction vs actual execution time error"),
        &["model"]
    ).unwrap();

    // Agent Collaboration Metrics
    static ref AGENT_COLLABORATION_COUNT: IntCounterVec = IntCounterVec::new(
        Opts::new("agent_collaboration_total", "Total agent collaborations"),
        &["agent1", "agent2", "type"]
    ).unwrap();

    static ref AGENT_COLLABORATION_SUCCESS_RATE: GaugeVec = GaugeVec::new(
        Opts::new("agent_collaboration_success_rate", "Success rate of agent collaborations"),
        &["type"]
    ).unwrap();

    static ref CONSENSUS_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("consensus_duration_seconds", "Time to reach consensus")
    ).unwrap();

    static ref CONSENSUS_PARTICIPATION: GaugeVec = GaugeVec::new(
        Opts::new("consensus_participation_rate", "Agent participation rate in consensus"),
        &["agent"]
    ).unwrap();

    // End-to-End Latency Breakdown
    static ref E2E_LATENCY_ROUTING: Histogram = Histogram::with_opts(
        HistogramOpts::new("e2e_latency_routing_ms", "Routing decision latency")
    ).unwrap();

    static ref E2E_LATENCY_PREPROCESSING: Histogram = Histogram::with_opts(
        HistogramOpts::new("e2e_latency_preprocessing_ms", "Context preprocessing latency")
    ).unwrap();

    static ref E2E_LATENCY_CONSENSUS: Histogram = Histogram::with_opts(
        HistogramOpts::new("e2e_latency_consensus_ms", "Consensus latency")
    ).unwrap();

    static ref E2E_LATENCY_EXECUTION: HistogramVec = HistogramVec::new(
        HistogramOpts::new("e2e_latency_execution_ms", "Execution latency by type"),
        &["execution_type"]
    ).unwrap();

    static ref E2E_LATENCY_TOTAL: Histogram = Histogram::with_opts(
        HistogramOpts::new("e2e_latency_total_ms", "Total end-to-end latency")
    ).unwrap();

    // Resource Utilization per Agent
    static ref AGENT_CPU_USAGE: GaugeVec = GaugeVec::new(
        Opts::new("agent_cpu_usage_percent", "CPU usage percentage per agent"),
        &["agent"]
    ).unwrap();

    static ref AGENT_MEMORY_USAGE: IntGaugeVec = IntGaugeVec::new(
        Opts::new("agent_memory_usage_mb", "Memory usage in MB per agent"),
        &["agent"]
    ).unwrap();

    static ref AGENT_TRANSACTION_THROUGHPUT: GaugeVec = GaugeVec::new(
        Opts::new("agent_transaction_throughput", "Transactions per second per agent"),
        &["agent"]
    ).unwrap();

    static ref AGENT_QUEUE_SIZE: IntGaugeVec = IntGaugeVec::new(
        Opts::new("agent_queue_size", "Pending transactions in agent queue"),
        &["agent"]
    ).unwrap();

    static ref AGENT_STATE_TRANSITIONS: IntCounterVec = IntCounterVec::new(
        Opts::new("agent_state_transitions", "Agent state transitions"),
        &["agent", "from_state", "to_state"]
    ).unwrap();

    // Cross-ExEx Metrics
    static ref CROSS_EXEX_MESSAGES: IntCounterVec = IntCounterVec::new(
        Opts::new("cross_exex_messages_total", "Total cross-ExEx messages"),
        &["from_exex", "to_exex", "message_type"]
    ).unwrap();

    static ref CROSS_EXEX_LATENCY: HistogramVec = HistogramVec::new(
        HistogramOpts::new("cross_exex_latency_ms", "Cross-ExEx communication latency"),
        &["from_exex", "to_exex"]
    ).unwrap();

    static ref CROSS_EXEX_FAILURES: IntCounterVec = IntCounterVec::new(
        Opts::new("cross_exex_failures_total", "Failed cross-ExEx communications"),
        &["from_exex", "to_exex", "error_type"]
    ).unwrap();
}

#[derive(Debug)]
pub struct EnhancedMetrics {
    registry: Registry,
    latency_tracker: Arc<RwLock<LatencyTracker>>,
    accuracy_tracker: Arc<RwLock<AccuracyTracker>>,
    resource_tracker: Arc<RwLock<ResourceTracker>>,
    collaboration_tracker: Arc<RwLock<CollaborationTracker>>,
}

#[derive(Debug, Default)]
struct LatencyTracker {
    transaction_stages: HashMap<B256, TransactionLatency>,
    stage_averages: HashMap<String, f64>,
}

#[derive(Debug, Clone)]
struct TransactionLatency {
    tx_hash: B256,
    start_time: Instant,
    routing_time: Option<Duration>,
    preprocessing_time: Option<Duration>,
    consensus_time: Option<Duration>,
    execution_time: Option<Duration>,
    total_time: Option<Duration>,
}

#[derive(Debug, Default)]
struct AccuracyTracker {
    model_predictions: HashMap<String, ModelAccuracy>,
    routing_accuracy: HashMap<RoutingDecision, RoutingAccuracy>,
}

#[derive(Debug, Clone, Default)]
struct ModelAccuracy {
    correct_predictions: u64,
    total_predictions: u64,
    confidence_sum: f64,
    error_sum: f64,
}

#[derive(Debug, Clone, Default)]
struct RoutingAccuracy {
    optimal_routes: u64,
    total_routes: u64,
    average_execution_time: f64,
}

#[derive(Debug, Default)]
struct ResourceTracker {
    agent_resources: HashMap<Address, AgentResourceUsage>,
    system_resources: SystemResourceUsage,
}

#[derive(Debug, Clone, Default)]
struct AgentResourceUsage {
    cpu_percent: f64,
    memory_mb: u64,
    transactions_processed: u64,
    queue_size: usize,
    last_updated: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
struct SystemResourceUsage {
    total_cpu_percent: f64,
    total_memory_mb: u64,
    active_agents: usize,
    total_throughput: f64,
}

#[derive(Debug, Default)]
struct CollaborationTracker {
    collaboration_pairs: HashMap<(Address, Address), CollaborationStats>,
    consensus_rounds: Vec<ConsensusRound>,
}

#[derive(Debug, Clone, Default)]
struct CollaborationStats {
    total_collaborations: u64,
    successful_collaborations: u64,
    average_duration: Duration,
    last_collaboration: Option<Instant>,
}

#[derive(Debug, Clone)]
struct ConsensusRound {
    round_id: String,
    participants: Vec<Address>,
    duration: Duration,
    success: bool,
    timestamp: Instant,
}

impl EnhancedMetrics {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // Register all metrics
        registry.register(Box::new(AI_DECISION_TOTAL.clone()))?;
        registry.register(Box::new(AI_DECISION_ACCURACY.clone()))?;
        registry.register(Box::new(AI_CONFIDENCE_HISTOGRAM.clone()))?;
        registry.register(Box::new(AI_PREDICTION_ERROR.clone()))?;
        
        registry.register(Box::new(AGENT_COLLABORATION_COUNT.clone()))?;
        registry.register(Box::new(AGENT_COLLABORATION_SUCCESS_RATE.clone()))?;
        registry.register(Box::new(CONSENSUS_DURATION.clone()))?;
        registry.register(Box::new(CONSENSUS_PARTICIPATION.clone()))?;
        
        registry.register(Box::new(E2E_LATENCY_ROUTING.clone()))?;
        registry.register(Box::new(E2E_LATENCY_PREPROCESSING.clone()))?;
        registry.register(Box::new(E2E_LATENCY_CONSENSUS.clone()))?;
        registry.register(Box::new(E2E_LATENCY_EXECUTION.clone()))?;
        registry.register(Box::new(E2E_LATENCY_TOTAL.clone()))?;
        
        registry.register(Box::new(AGENT_CPU_USAGE.clone()))?;
        registry.register(Box::new(AGENT_MEMORY_USAGE.clone()))?;
        registry.register(Box::new(AGENT_TRANSACTION_THROUGHPUT.clone()))?;
        registry.register(Box::new(AGENT_QUEUE_SIZE.clone()))?;
        registry.register(Box::new(AGENT_STATE_TRANSITIONS.clone()))?;
        
        registry.register(Box::new(CROSS_EXEX_MESSAGES.clone()))?;
        registry.register(Box::new(CROSS_EXEX_LATENCY.clone()))?;
        registry.register(Box::new(CROSS_EXEX_FAILURES.clone()))?;

        Ok(Self {
            registry,
            latency_tracker: Arc::new(RwLock::new(LatencyTracker::default())),
            accuracy_tracker: Arc::new(RwLock::new(AccuracyTracker::default())),
            resource_tracker: Arc::new(RwLock::new(ResourceTracker::default())),
            collaboration_tracker: Arc::new(RwLock::new(CollaborationTracker::default())),
        })
    }

    // AI Decision Metrics
    pub async fn record_ai_decision(
        &self,
        model: &str,
        decision: &RoutingDecision,
        confidence: f64,
    ) {
        AI_DECISION_TOTAL.with_label_values(&[model, decision.as_str()]).inc();
        AI_CONFIDENCE_HISTOGRAM.with_label_values(&[model, decision.as_str()]).observe(confidence);
    }

    pub async fn update_ai_accuracy(
        &self,
        model: &str,
        correct: bool,
        prediction_error: f64,
    ) {
        let mut tracker = self.accuracy_tracker.write().await;
        let accuracy = tracker.model_predictions.entry(model.to_string())
            .or_insert_with(ModelAccuracy::default);
        
        accuracy.total_predictions += 1;
        if correct {
            accuracy.correct_predictions += 1;
        }
        accuracy.error_sum += prediction_error;
        
        let accuracy_rate = accuracy.correct_predictions as f64 / accuracy.total_predictions as f64;
        AI_DECISION_ACCURACY.with_label_values(&[model]).set(accuracy_rate);
        AI_PREDICTION_ERROR.with_label_values(&[model]).observe(prediction_error);
    }

    // Latency Tracking
    pub async fn start_transaction_tracking(&self, tx_hash: B256) {
        let mut tracker = self.latency_tracker.write().await;
        tracker.transaction_stages.insert(tx_hash, TransactionLatency {
            tx_hash,
            start_time: Instant::now(),
            routing_time: None,
            preprocessing_time: None,
            consensus_time: None,
            execution_time: None,
            total_time: None,
        });
    }

    pub async fn record_routing_latency(&self, tx_hash: B256, duration: Duration) {
        let mut tracker = self.latency_tracker.write().await;
        if let Some(tx_latency) = tracker.transaction_stages.get_mut(&tx_hash) {
            tx_latency.routing_time = Some(duration);
            E2E_LATENCY_ROUTING.observe(duration.as_millis() as f64);
        }
    }

    pub async fn record_preprocessing_latency(&self, tx_hash: B256, duration: Duration) {
        let mut tracker = self.latency_tracker.write().await;
        if let Some(tx_latency) = tracker.transaction_stages.get_mut(&tx_hash) {
            tx_latency.preprocessing_time = Some(duration);
            E2E_LATENCY_PREPROCESSING.observe(duration.as_millis() as f64);
        }
    }

    pub async fn record_consensus_latency(&self, tx_hash: B256, duration: Duration) {
        let mut tracker = self.latency_tracker.write().await;
        if let Some(tx_latency) = tracker.transaction_stages.get_mut(&tx_hash) {
            tx_latency.consensus_time = Some(duration);
            E2E_LATENCY_CONSENSUS.observe(duration.as_millis() as f64);
        }
        CONSENSUS_DURATION.observe(duration.as_secs_f64());
    }

    pub async fn record_execution_latency(
        &self,
        tx_hash: B256,
        execution_type: &str,
        duration: Duration,
    ) {
        let mut tracker = self.latency_tracker.write().await;
        if let Some(tx_latency) = tracker.transaction_stages.get_mut(&tx_hash) {
            tx_latency.execution_time = Some(duration);
            let total = tx_latency.start_time.elapsed();
            tx_latency.total_time = Some(total);
            
            E2E_LATENCY_EXECUTION.with_label_values(&[execution_type]).observe(duration.as_millis() as f64);
            E2E_LATENCY_TOTAL.observe(total.as_millis() as f64);
        }
    }

    // Agent Resource Metrics
    pub async fn update_agent_resources(
        &self,
        agent: Address,
        cpu_percent: f64,
        memory_mb: u64,
        queue_size: usize,
    ) {
        let mut tracker = self.resource_tracker.write().await;
        let usage = tracker.agent_resources.entry(agent)
            .or_insert_with(AgentResourceUsage::default);
        
        usage.cpu_percent = cpu_percent;
        usage.memory_mb = memory_mb;
        usage.queue_size = queue_size;
        usage.last_updated = Some(Instant::now());
        
        let agent_str = format!("{:?}", agent);
        AGENT_CPU_USAGE.with_label_values(&[&agent_str]).set(cpu_percent);
        AGENT_MEMORY_USAGE.with_label_values(&[&agent_str]).set(memory_mb as i64);
        AGENT_QUEUE_SIZE.with_label_values(&[&agent_str]).set(queue_size as i64);
    }

    pub async fn update_agent_throughput(&self, agent: Address, tps: f64) {
        let agent_str = format!("{:?}", agent);
        AGENT_TRANSACTION_THROUGHPUT.with_label_values(&[&agent_str]).set(tps);
        
        let mut tracker = self.resource_tracker.write().await;
        if let Some(usage) = tracker.agent_resources.get_mut(&agent) {
            usage.transactions_processed += 1;
        }
    }

    pub async fn record_agent_state_transition(
        &self,
        agent: Address,
        from_state: &LifecycleState,
        to_state: &LifecycleState,
    ) {
        let agent_str = format!("{:?}", agent);
        let from_str = format!("{:?}", from_state);
        let to_str = format!("{:?}", to_state);
        
        AGENT_STATE_TRANSITIONS.with_label_values(&[&agent_str, &from_str, &to_str]).inc();
    }

    // Collaboration Metrics
    pub async fn record_agent_collaboration(
        &self,
        agent1: Address,
        agent2: Address,
        collaboration_type: &str,
        success: bool,
        duration: Duration,
    ) {
        let agent1_str = format!("{:?}", agent1);
        let agent2_str = format!("{:?}", agent2);
        
        AGENT_COLLABORATION_COUNT.with_label_values(&[&agent1_str, &agent2_str, collaboration_type]).inc();
        
        let mut tracker = self.collaboration_tracker.write().await;
        let key = if agent1 < agent2 { (agent1, agent2) } else { (agent2, agent1) };
        let stats = tracker.collaboration_pairs.entry(key)
            .or_insert_with(CollaborationStats::default);
        
        stats.total_collaborations += 1;
        if success {
            stats.successful_collaborations += 1;
        }
        
        // Update average duration
        let total_duration = stats.average_duration.as_millis() as u64 * (stats.total_collaborations - 1)
            + duration.as_millis() as u64;
        stats.average_duration = Duration::from_millis(total_duration / stats.total_collaborations);
        stats.last_collaboration = Some(Instant::now());
        
        // Update success rate
        let success_rate = stats.successful_collaborations as f64 / stats.total_collaborations as f64;
        AGENT_COLLABORATION_SUCCESS_RATE.with_label_values(&[collaboration_type]).set(success_rate);
    }

    pub async fn record_consensus_round(
        &self,
        round_id: String,
        participants: Vec<Address>,
        duration: Duration,
        success: bool,
    ) {
        let mut tracker = self.collaboration_tracker.write().await;
        tracker.consensus_rounds.push(ConsensusRound {
            round_id,
            participants: participants.clone(),
            duration,
            success,
            timestamp: Instant::now(),
        });
        
        // Keep only recent rounds
        if tracker.consensus_rounds.len() > 1000 {
            tracker.consensus_rounds.remove(0);
        }
        
        // Update participation metrics
        for participant in participants {
            let agent_str = format!("{:?}", participant);
            CONSENSUS_PARTICIPATION.with_label_values(&[&agent_str]).inc();
        }
    }

    // Cross-ExEx Communication Metrics
    pub async fn record_cross_exex_message(
        &self,
        from_exex: &str,
        to_exex: &str,
        message_type: &str,
        latency: Duration,
    ) {
        CROSS_EXEX_MESSAGES.with_label_values(&[from_exex, to_exex, message_type]).inc();
        CROSS_EXEX_LATENCY.with_label_values(&[from_exex, to_exex]).observe(latency.as_millis() as f64);
    }

    pub async fn record_cross_exex_failure(
        &self,
        from_exex: &str,
        to_exex: &str,
        error_type: &str,
    ) {
        CROSS_EXEX_FAILURES.with_label_values(&[from_exex, to_exex, error_type]).inc();
    }

    // Aggregate Metrics
    pub async fn get_system_overview(&self) -> SystemMetricsOverview {
        let latency_tracker = self.latency_tracker.read().await;
        let accuracy_tracker = self.accuracy_tracker.read().await;
        let resource_tracker = self.resource_tracker.read().await;
        let collaboration_tracker = self.collaboration_tracker.read().await;

        let total_accuracy = accuracy_tracker.model_predictions.values()
            .map(|a| a.correct_predictions as f64 / a.total_predictions.max(1) as f64)
            .sum::<f64>() / accuracy_tracker.model_predictions.len().max(1) as f64;

        let avg_e2e_latency = latency_tracker.transaction_stages.values()
            .filter_map(|l| l.total_time)
            .map(|d| d.as_millis() as f64)
            .sum::<f64>() / latency_tracker.transaction_stages.len().max(1) as f64;

        SystemMetricsOverview {
            total_transactions: latency_tracker.transaction_stages.len() as u64,
            average_accuracy: total_accuracy,
            average_e2e_latency_ms: avg_e2e_latency,
            active_agents: resource_tracker.agent_resources.len(),
            total_collaborations: collaboration_tracker.collaboration_pairs.values()
                .map(|s| s.total_collaborations)
                .sum(),
            consensus_rounds: collaboration_tracker.consensus_rounds.len() as u64,
        }
    }

    pub fn get_registry(&self) -> &Registry {
        &self.registry
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricsOverview {
    pub total_transactions: u64,
    pub average_accuracy: f64,
    pub average_e2e_latency_ms: f64,
    pub active_agents: usize,
    pub total_collaborations: u64,
    pub consensus_rounds: u64,
}

// Helper functions for metric aggregation
impl EnhancedMetrics {
    pub async fn generate_performance_report(&self) -> PerformanceReport {
        let overview = self.get_system_overview().await;
        let accuracy_tracker = self.accuracy_tracker.read().await;
        let resource_tracker = self.resource_tracker.read().await;

        let model_performance: HashMap<String, ModelPerformanceReport> = accuracy_tracker
            .model_predictions
            .iter()
            .map(|(model, accuracy)| {
                let perf = ModelPerformanceReport {
                    accuracy_rate: accuracy.correct_predictions as f64 / accuracy.total_predictions.max(1) as f64,
                    average_confidence: accuracy.confidence_sum / accuracy.total_predictions.max(1) as f64,
                    average_error: accuracy.error_sum / accuracy.total_predictions.max(1) as f64,
                    total_predictions: accuracy.total_predictions,
                };
                (model.clone(), perf)
            })
            .collect();

        let agent_utilization: HashMap<String, AgentUtilizationReport> = resource_tracker
            .agent_resources
            .iter()
            .map(|(agent, usage)| {
                let report = AgentUtilizationReport {
                    cpu_usage: usage.cpu_percent,
                    memory_usage_mb: usage.memory_mb,
                    queue_depth: usage.queue_size,
                    transactions_processed: usage.transactions_processed,
                };
                (format!("{:?}", agent), report)
            })
            .collect();

        PerformanceReport {
            timestamp: chrono::Utc::now(),
            overview,
            model_performance,
            agent_utilization,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub overview: SystemMetricsOverview,
    pub model_performance: HashMap<String, ModelPerformanceReport>,
    pub agent_utilization: HashMap<String, AgentUtilizationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformanceReport {
    pub accuracy_rate: f64,
    pub average_confidence: f64,
    pub average_error: f64,
    pub total_predictions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentUtilizationReport {
    pub cpu_usage: f64,
    pub memory_usage_mb: u64,
    pub queue_depth: usize,
    pub transactions_processed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enhanced_metrics_creation() {
        let metrics = EnhancedMetrics::new().unwrap();
        assert!(metrics.registry.gather().len() > 0);
    }

    #[tokio::test]
    async fn test_latency_tracking() {
        let metrics = EnhancedMetrics::new().unwrap();
        let tx_hash = B256::random();
        
        metrics.start_transaction_tracking(tx_hash).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        metrics.record_routing_latency(tx_hash, Duration::from_millis(5)).await;
        metrics.record_preprocessing_latency(tx_hash, Duration::from_millis(3)).await;
        metrics.record_execution_latency(tx_hash, "rag", Duration::from_millis(20)).await;
        
        let overview = metrics.get_system_overview().await;
        assert_eq!(overview.total_transactions, 1);
    }

    #[tokio::test]
    async fn test_ai_accuracy_tracking() {
        let metrics = EnhancedMetrics::new().unwrap();
        
        metrics.record_ai_decision("model1", &RoutingDecision::ProcessWithRAG, 0.85).await;
        metrics.update_ai_accuracy("model1", true, 5.0).await;
        metrics.update_ai_accuracy("model1", false, 15.0).await;
        
        let report = metrics.generate_performance_report().await;
        assert!(report.model_performance.contains_key("model1"));
        assert_eq!(report.model_performance["model1"].accuracy_rate, 0.5);
    }
}