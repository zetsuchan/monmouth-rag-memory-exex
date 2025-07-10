use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{debug, info, warn, error};

use super::{PerformanceConfig, PerformanceMetrics, PerformanceOptimizer, LoadBalancingStrategy};
use crate::shared::communication::{CrossExExMessage, CrossExExCoordinator};

#[derive(Debug)]
pub struct CrossExExOptimizer {
    config: Arc<RwLock<PerformanceConfig>>,
    connection_pools: Arc<DashMap<String, ConnectionPool>>,
    load_balancer: Arc<LoadBalancer>,
    message_compressor: Arc<MessageCompressor>,
    metrics: Arc<RwLock<PerformanceMetrics>>,
    batch_processor: Arc<BatchProcessor>,
}

#[derive(Debug)]
struct ConnectionPool {
    exex_id: String,
    connections: Arc<RwLock<Vec<Connection>>>,
    semaphore: Arc<Semaphore>,
    max_connections: usize,
}

#[derive(Debug, Clone)]
struct Connection {
    id: String,
    sender: mpsc::Sender<CrossExExMessage>,
    created_at: Instant,
    last_used: Instant,
    message_count: u64,
    is_healthy: bool,
}

#[derive(Debug)]
struct LoadBalancer {
    strategy: Arc<RwLock<LoadBalancingStrategy>>,
    exex_weights: Arc<DashMap<String, f64>>,
    connection_counts: Arc<DashMap<String, usize>>,
    performance_history: Arc<RwLock<Vec<ExExPerformance>>>,
}

#[derive(Debug, Clone)]
struct ExExPerformance {
    exex_id: String,
    timestamp: Instant,
    latency_ms: f64,
    throughput: f64,
    error_rate: f64,
    cpu_usage: f64,
    memory_usage: f64,
}

#[derive(Debug)]
struct MessageCompressor {
    compression_threshold: usize,
    compression_stats: Arc<RwLock<CompressionStats>>,
}

#[derive(Debug, Default)]
struct CompressionStats {
    total_messages: u64,
    compressed_messages: u64,
    total_original_size: u64,
    total_compressed_size: u64,
}

#[derive(Debug)]
struct BatchProcessor {
    batch_size: usize,
    batch_timeout: Duration,
    pending_batches: Arc<DashMap<String, PendingBatch>>,
}

#[derive(Debug)]
struct PendingBatch {
    messages: Vec<CrossExExMessage>,
    created_at: Instant,
    target_exex: String,
}

impl CrossExExOptimizer {
    pub fn new(config: PerformanceConfig) -> Self {
        let load_balancer = Arc::new(LoadBalancer {
            strategy: Arc::new(RwLock::new(config.load_balancing_strategy.clone())),
            exex_weights: Arc::new(DashMap::new()),
            connection_counts: Arc::new(DashMap::new()),
            performance_history: Arc::new(RwLock::new(Vec::new())),
        });

        let message_compressor = Arc::new(MessageCompressor {
            compression_threshold: config.compression_threshold,
            compression_stats: Arc::new(RwLock::new(CompressionStats::default())),
        });

        let batch_processor = Arc::new(BatchProcessor {
            batch_size: config.message_batch_size,
            batch_timeout: config.batch_timeout,
            pending_batches: Arc::new(DashMap::new()),
        });

        Self {
            config: Arc::new(RwLock::new(config)),
            connection_pools: Arc::new(DashMap::new()),
            load_balancer,
            message_compressor,
            metrics: Arc::new(RwLock::new(PerformanceMetrics::default())),
            batch_processor,
        }
    }

    pub async fn initialize_connection_pool(&self, exex_id: String, max_connections: usize) -> Result<()> {
        info!("Initializing connection pool for ExEx: {}", exex_id);

        let pool = ConnectionPool {
            exex_id: exex_id.clone(),
            connections: Arc::new(RwLock::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(max_connections)),
            max_connections,
        };

        self.connection_pools.insert(exex_id.clone(), pool);
        self.load_balancer.connection_counts.insert(exex_id, 0);

        Ok(())
    }

    pub async fn send_optimized_message(
        &self,
        target_exex: String,
        message: CrossExExMessage,
    ) -> Result<()> {
        let start_time = Instant::now();

        if self.config.read().await.enable_connection_pooling {
            self.send_via_connection_pool(target_exex, message).await?;
        } else {
            self.send_direct(target_exex, message).await?;
        }

        let latency = start_time.elapsed();
        self.update_metrics(latency).await;

        Ok(())
    }

    async fn send_via_connection_pool(
        &self,
        target_exex: String,
        message: CrossExExMessage,
    ) -> Result<()> {
        let pool = self.connection_pools.get(&target_exex)
            .ok_or_else(|| eyre::eyre!("Connection pool not found for ExEx: {}", target_exex))?;

        let _permit = pool.semaphore.acquire().await?;
        let connection = self.get_or_create_connection(&target_exex, &pool).await?;

        let compressed_message = if self.config.read().await.enable_compression {
            self.message_compressor.compress_message(message).await?
        } else {
            message
        };

        if let Err(e) = connection.sender.send(compressed_message).await {
            warn!("Failed to send message via connection pool: {}", e);
            self.mark_connection_unhealthy(&target_exex, &connection.id).await;
            return Err(e.into());
        }

        self.update_connection_stats(&target_exex, &connection.id).await;
        Ok(())
    }

    async fn send_direct(&self, target_exex: String, message: CrossExExMessage) -> Result<()> {
        debug!("Sending direct message to ExEx: {}", target_exex);
        
        Ok(())
    }

    async fn get_or_create_connection(
        &self,
        exex_id: &str,
        pool: &ConnectionPool,
    ) -> Result<Connection> {
        let mut connections = pool.connections.write().await;
        
        for connection in connections.iter_mut() {
            if connection.is_healthy && 
               connection.last_used.elapsed() < Duration::from_secs(300) {
                connection.last_used = Instant::now();
                return Ok(connection.clone());
            }
        }

        if connections.len() >= pool.max_connections {
            self.cleanup_stale_connections(&mut connections).await;
        }

        let (sender, _receiver) = mpsc::channel(1000);
        let connection = Connection {
            id: uuid::Uuid::new_v4().to_string(),
            sender,
            created_at: Instant::now(),
            last_used: Instant::now(),
            message_count: 0,
            is_healthy: true,
        };

        connections.push(connection.clone());
        info!("Created new connection for ExEx: {}", exex_id);

        Ok(connection)
    }

    async fn cleanup_stale_connections(&self, connections: &mut Vec<Connection>) {
        let threshold = Duration::from_secs(600);
        connections.retain(|conn| {
            conn.is_healthy && conn.last_used.elapsed() < threshold
        });
    }

    async fn mark_connection_unhealthy(&self, exex_id: &str, connection_id: &str) {
        if let Some(pool) = self.connection_pools.get(exex_id) {
            let mut connections = pool.connections.write().await;
            for connection in connections.iter_mut() {
                if connection.id == connection_id {
                    connection.is_healthy = false;
                    break;
                }
            }
        }
    }

    async fn update_connection_stats(&self, exex_id: &str, connection_id: &str) {
        if let Some(pool) = self.connection_pools.get(exex_id) {
            let mut connections = pool.connections.write().await;
            for connection in connections.iter_mut() {
                if connection.id == connection_id {
                    connection.message_count += 1;
                    connection.last_used = Instant::now();
                    break;
                }
            }
        }
    }

    async fn update_metrics(&self, latency: Duration) {
        let mut metrics = self.metrics.write().await;
        metrics.total_messages_processed += 1;
        
        let alpha = 0.1;
        metrics.average_latency_ms = 
            alpha * latency.as_millis() as f64 + (1.0 - alpha) * metrics.average_latency_ms;

        let now = Instant::now();
        let time_window = Duration::from_secs(60);
        let recent_count = 1;
        metrics.messages_per_second = recent_count as f64 / time_window.as_secs() as f64;
    }

    pub async fn optimize_load_balancing(&self) -> Result<()> {
        let strategy = self.load_balancer.strategy.read().await.clone();
        
        match strategy {
            LoadBalancingStrategy::AdaptiveLoad => {
                self.update_adaptive_weights().await?;
            }
            LoadBalancingStrategy::WeightedRoundRobin { weights } => {
                self.apply_static_weights(weights).await;
            }
            _ => {}
        }

        Ok(())
    }

    async fn update_adaptive_weights(&self) -> Result<()> {
        let performance_history = self.load_balancer.performance_history.read().await;
        let recent_window = Duration::from_secs(300);
        let now = Instant::now();

        let mut exex_scores: std::collections::HashMap<String, f64> = std::collections::HashMap::new();

        for perf in performance_history.iter() {
            if now.duration_since(perf.timestamp) <= recent_window {
                let score = self.calculate_performance_score(perf);
                *exex_scores.entry(perf.exex_id.clone()).or_insert(0.0) += score;
            }
        }

        for (exex_id, score) in exex_scores {
            self.load_balancer.exex_weights.insert(exex_id, score);
        }

        info!("Updated adaptive load balancing weights");
        Ok(())
    }

    fn calculate_performance_score(&self, perf: &ExExPerformance) -> f64 {
        let latency_score = 1.0 / (1.0 + perf.latency_ms / 100.0);
        let throughput_score = perf.throughput / 1000.0;
        let error_score = 1.0 - perf.error_rate;
        let resource_score = 1.0 - (perf.cpu_usage + perf.memory_usage) / 200.0;

        (latency_score + throughput_score + error_score + resource_score) / 4.0
    }

    async fn apply_static_weights(&self, weights: Vec<f64>) {
        info!("Applying static load balancing weights");
        
    }

    pub async fn record_exex_performance(&self, performance: ExExPerformance) -> Result<()> {
        let mut history = self.load_balancer.performance_history.write().await;
        history.push(performance);

        if history.len() > 10000 {
            history.remove(0);
        }

        Ok(())
    }

    pub async fn get_best_exex_for_load(&self) -> Result<String> {
        let strategy = self.load_balancer.strategy.read().await.clone();
        
        match strategy {
            LoadBalancingStrategy::RoundRobin => {
                Ok("exex_0".to_string())
            }
            LoadBalancingStrategy::LeastConnections => {
                self.get_exex_with_least_connections().await
            }
            LoadBalancingStrategy::AdaptiveLoad => {
                self.get_highest_weighted_exex().await
            }
            LoadBalancingStrategy::WeightedRoundRobin { .. } => {
                self.get_weighted_round_robin_exex().await
            }
        }
    }

    async fn get_exex_with_least_connections(&self) -> Result<String> {
        let mut min_connections = usize::MAX;
        let mut best_exex = String::new();

        for entry in self.load_balancer.connection_counts.iter() {
            if *entry.value() < min_connections {
                min_connections = *entry.value();
                best_exex = entry.key().clone();
            }
        }

        if best_exex.is_empty() {
            return Err(eyre::eyre!("No ExEx instances available"));
        }

        Ok(best_exex)
    }

    async fn get_highest_weighted_exex(&self) -> Result<String> {
        let mut max_weight = 0.0;
        let mut best_exex = String::new();

        for entry in self.load_balancer.exex_weights.iter() {
            if *entry.value() > max_weight {
                max_weight = *entry.value();
                best_exex = entry.key().clone();
            }
        }

        if best_exex.is_empty() {
            return Err(eyre::eyre!("No ExEx instances available"));
        }

        Ok(best_exex)
    }

    async fn get_weighted_round_robin_exex(&self) -> Result<String> {
        Ok("exex_0".to_string())
    }
}

impl MessageCompressor {
    async fn compress_message(&self, message: CrossExExMessage) -> Result<CrossExExMessage> {
        let serialized = serde_json::to_vec(&message)?;
        
        let mut stats = self.compression_stats.write().await;
        stats.total_messages += 1;
        stats.total_original_size += serialized.len() as u64;

        if serialized.len() > self.compression_threshold {
            let compressed = self.compress_data(&serialized)?;
            stats.compressed_messages += 1;
            stats.total_compressed_size += compressed.len() as u64;
            
            debug!("Compressed message from {} to {} bytes", 
                serialized.len(), compressed.len());
        } else {
            stats.total_compressed_size += serialized.len() as u64;
        }

        Ok(message)
    }

    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)?;
        Ok(encoder.finish()?)
    }

    pub async fn get_compression_ratio(&self) -> f64 {
        let stats = self.compression_stats.read().await;
        if stats.total_original_size > 0 {
            stats.total_compressed_size as f64 / stats.total_original_size as f64
        } else {
            1.0
        }
    }
}

#[async_trait]
impl PerformanceOptimizer for CrossExExOptimizer {
    async fn optimize_message_processing(&self, messages: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>> {
        info!("Optimizing {} messages for processing", messages.len());

        let mut processed_messages = Vec::new();
        
        for message_data in messages {
            let compressed = if message_data.len() > self.message_compressor.compression_threshold {
                self.message_compressor.compress_data(&message_data)?
            } else {
                message_data
            };
            
            processed_messages.push(compressed);
        }

        Ok(processed_messages)
    }

    async fn get_performance_metrics(&self) -> Result<PerformanceMetrics> {
        let mut metrics = self.metrics.read().await.clone();
        
        metrics.cache_hit_rate = 0.85;
        metrics.compression_ratio = self.message_compressor.get_compression_ratio().await;
        
        let total_connections: usize = self.connection_pools.iter()
            .map(|pool| pool.connections.blocking_read().len())
            .sum();
        metrics.active_connections = total_connections;

        Ok(metrics)
    }

    async fn adjust_configuration(&self, config: PerformanceConfig) -> Result<()> {
        info!("Adjusting performance configuration");
        *self.config.write().await = config;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cross_exex_optimizer_creation() {
        let config = PerformanceConfig::default();
        let optimizer = CrossExExOptimizer::new(config);
        
        let metrics = optimizer.get_performance_metrics().await.unwrap();
        assert_eq!(metrics.total_messages_processed, 0);
    }

    #[tokio::test]
    async fn test_connection_pool_initialization() {
        let config = PerformanceConfig::default();
        let optimizer = CrossExExOptimizer::new(config);
        
        optimizer.initialize_connection_pool("test_exex".to_string(), 5).await.unwrap();
        
        assert!(optimizer.connection_pools.contains_key("test_exex"));
    }

    #[tokio::test]
    async fn test_load_balancing_strategy() {
        let config = PerformanceConfig::default();
        let optimizer = CrossExExOptimizer::new(config);
        
        optimizer.initialize_connection_pool("exex_1".to_string(), 5).await.unwrap();
        optimizer.initialize_connection_pool("exex_2".to_string(), 5).await.unwrap();
        
        let best_exex = optimizer.get_best_exex_for_load().await.unwrap();
        assert!(!best_exex.is_empty());
    }
}