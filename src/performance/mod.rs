use eyre::Result;

pub mod optimization;
pub mod cache;
pub mod throughput;
pub mod batching;

pub use optimization::*;
pub use cache::*;
pub use throughput::*;
pub use batching::*;

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub enable_connection_pooling: bool,
    pub max_connections_per_exex: usize,
    pub connection_timeout: Duration,
    pub message_batch_size: usize,
    pub batch_timeout: Duration,
    pub enable_compression: bool,
    pub compression_threshold: usize,
    pub cache_size_mb: usize,
    pub cache_ttl: Duration,
    pub enable_parallel_processing: bool,
    pub max_parallel_workers: usize,
    pub load_balancing_strategy: LoadBalancingStrategy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadBalancingStrategy {
    RoundRobin,
    LeastConnections,
    WeightedRoundRobin { weights: Vec<f64> },
    AdaptiveLoad,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            enable_connection_pooling: true,
            max_connections_per_exex: 10,
            connection_timeout: Duration::from_secs(30),
            message_batch_size: 100,
            batch_timeout: Duration::from_millis(50),
            enable_compression: true,
            compression_threshold: 1024,
            cache_size_mb: 256,
            cache_ttl: Duration::from_secs(300),
            enable_parallel_processing: true,
            max_parallel_workers: 8,
            load_balancing_strategy: LoadBalancingStrategy::AdaptiveLoad,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_messages_processed: u64,
    pub messages_per_second: f64,
    pub average_latency_ms: f64,
    pub cache_hit_rate: f64,
    pub compression_ratio: f64,
    pub active_connections: usize,
    pub memory_usage_mb: f64,
    pub cpu_usage_percent: f64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            total_messages_processed: 0,
            messages_per_second: 0.0,
            average_latency_ms: 0.0,
            cache_hit_rate: 0.0,
            compression_ratio: 1.0,
            active_connections: 0,
            memory_usage_mb: 0.0,
            cpu_usage_percent: 0.0,
        }
    }
}

pub trait PerformanceOptimizer: Send + Sync {
    async fn optimize_message_processing(&self, messages: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>>;
    async fn get_performance_metrics(&self) -> Result<PerformanceMetrics>;
    async fn adjust_configuration(&self, config: PerformanceConfig) -> Result<()>;
}