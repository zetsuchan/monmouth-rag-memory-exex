use alloy::primitives::{Address, B256};
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{debug, info, warn, error};

use super::PerformanceConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputConfig {
    pub target_tps: f64,
    pub max_batch_size: usize,
    pub batch_timeout: Duration,
    pub queue_size: usize,
    pub worker_count: usize,
    pub backpressure_threshold: f64,
    pub adaptive_batching: bool,
    pub priority_levels: usize,
}

impl Default for ThroughputConfig {
    fn default() -> Self {
        Self {
            target_tps: 1000.0,
            max_batch_size: 100,
            batch_timeout: Duration::from_millis(10),
            queue_size: 10000,
            worker_count: 8,
            backpressure_threshold: 0.8,
            adaptive_batching: true,
            priority_levels: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputMetrics {
    pub current_tps: f64,
    pub average_tps: f64,
    pub peak_tps: f64,
    pub queue_depth: usize,
    pub worker_utilization: f64,
    pub batch_efficiency: f64,
    pub backpressure_events: u64,
    pub dropped_messages: u64,
    pub processing_latency_ms: f64,
}

impl Default for ThroughputMetrics {
    fn default() -> Self {
        Self {
            current_tps: 0.0,
            average_tps: 0.0,
            peak_tps: 0.0,
            queue_depth: 0,
            worker_utilization: 0.0,
            batch_efficiency: 0.0,
            backpressure_events: 0,
            dropped_messages: 0,
            processing_latency_ms: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrioritizedMessage<T> {
    pub data: T,
    pub priority: u8,
    pub created_at: Instant,
    pub requester: Address,
    pub message_id: B256,
}

impl<T> PrioritizedMessage<T> {
    pub fn new(data: T, priority: u8, requester: Address) -> Self {
        Self {
            data,
            priority,
            created_at: Instant::now(),
            requester,
            message_id: B256::random(),
        }
    }

    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

impl<T> PartialEq for PrioritizedMessage<T> {
    fn eq(&self, other: &Self) -> bool {
        self.message_id == other.message_id
    }
}

impl<T> Eq for PrioritizedMessage<T> {}

impl<T> PartialOrd for PrioritizedMessage<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for PrioritizedMessage<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.priority.cmp(&self.priority)
            .then_with(|| self.created_at.cmp(&other.created_at))
    }
}

#[derive(Debug)]
pub struct ThroughputOptimizer<T> 
where 
    T: Clone + Send + Sync + 'static,
{
    config: Arc<RwLock<ThroughputConfig>>,
    message_queues: Vec<Arc<RwLock<VecDeque<PrioritizedMessage<T>>>>>,
    worker_semaphore: Arc<Semaphore>,
    metrics: Arc<RwLock<ThroughputMetrics>>,
    throughput_history: Arc<RwLock<VecDeque<ThroughputSample>>>,
    adaptive_controller: Arc<AdaptiveBatchController>,
}

#[derive(Debug, Clone)]
struct ThroughputSample {
    timestamp: Instant,
    tps: f64,
    latency_ms: f64,
    queue_depth: usize,
}

#[derive(Debug)]
struct AdaptiveBatchController {
    current_batch_size: Arc<RwLock<usize>>,
    performance_history: Arc<RwLock<VecDeque<BatchPerformance>>>,
    adjustment_factor: Arc<RwLock<f64>>,
}

#[derive(Debug, Clone)]
struct BatchPerformance {
    batch_size: usize,
    throughput: f64,
    latency: Duration,
    timestamp: Instant,
}

impl<T> ThroughputOptimizer<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(config: ThroughputConfig) -> Self {
        let queues = (0..config.priority_levels)
            .map(|_| Arc::new(RwLock::new(VecDeque::new())))
            .collect();

        let adaptive_controller = Arc::new(AdaptiveBatchController {
            current_batch_size: Arc::new(RwLock::new(config.max_batch_size)),
            performance_history: Arc::new(RwLock::new(VecDeque::new())),
            adjustment_factor: Arc::new(RwLock::new(1.0)),
        });

        Self {
            config: Arc::new(RwLock::new(config.clone())),
            message_queues: queues,
            worker_semaphore: Arc::new(Semaphore::new(config.worker_count)),
            metrics: Arc::new(RwLock::new(ThroughputMetrics::default())),
            throughput_history: Arc::new(RwLock::new(VecDeque::new())),
            adaptive_controller,
        }
    }

    pub async fn submit_message(
        &self,
        message: PrioritizedMessage<T>,
    ) -> Result<()> {
        let priority = message.priority.min((self.message_queues.len() - 1) as u8);
        let queue_index = priority as usize;
        
        let mut queue = self.message_queues[queue_index].write().await;
        let config = self.config.read().await;

        if queue.len() >= config.queue_size / self.message_queues.len() {
            if self.should_apply_backpressure().await {
                self.metrics.write().await.backpressure_events += 1;
                return Err(eyre::eyre!("Queue full, backpressure applied"));
            } else {
                queue.pop_front();
                self.metrics.write().await.dropped_messages += 1;
                warn!("Dropped oldest message due to queue overflow");
            }
        }

        queue.push_back(message);
        debug!("Message submitted to priority queue {}", priority);
        
        Ok(())
    }

    pub async fn start_processing<F, Fut>(
        &self,
        processor: F,
    ) -> Result<()>
    where
        F: Fn(Vec<T>) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        info!("Starting throughput optimizer with {} workers", 
            self.config.read().await.worker_count);

        for worker_id in 0..self.config.read().await.worker_count {
            let worker = ThroughputWorker::new(
                worker_id,
                self.clone(),
                processor.clone(),
            );
            
            tokio::spawn(async move {
                if let Err(e) = worker.run().await {
                    error!("Worker {} failed: {}", worker_id, e);
                }
            });
        }

        let metrics_updater = self.clone();
        tokio::spawn(async move {
            metrics_updater.update_metrics_loop().await;
        });

        if self.config.read().await.adaptive_batching {
            let adaptive_controller = self.clone();
            tokio::spawn(async move {
                adaptive_controller.adaptive_batch_control_loop().await;
            });
        }

        Ok(())
    }

    async fn collect_batch(&self) -> Vec<T> {
        let config = self.config.read().await;
        let batch_size = if config.adaptive_batching {
            *self.adaptive_controller.current_batch_size.read().await
        } else {
            config.max_batch_size
        };

        let mut batch = Vec::with_capacity(batch_size);
        let batch_timeout = config.batch_timeout;
        drop(config);

        let start_time = Instant::now();

        'collection: loop {
            for queue in &self.message_queues {
                if batch.len() >= batch_size {
                    break 'collection;
                }

                if let Some(message) = queue.write().await.pop_front() {
                    batch.push(message.data);
                }
            }

            if batch.is_empty() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }

            if start_time.elapsed() >= batch_timeout && !batch.is_empty() {
                break;
            }

            if batch.is_empty() && start_time.elapsed() >= batch_timeout * 2 {
                break;
            }
        }

        debug!("Collected batch of {} messages in {:?}", 
            batch.len(), start_time.elapsed());
        
        batch
    }

    async fn should_apply_backpressure(&self) -> bool {
        let config = self.config.read().await;
        let total_queue_depth: usize = self.message_queues.iter()
            .map(|queue| queue.blocking_read().len())
            .sum();
        
        let total_capacity = config.queue_size;
        let utilization = total_queue_depth as f64 / total_capacity as f64;
        
        utilization > config.backpressure_threshold
    }

    async fn update_metrics_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut last_processed = 0u64;
        
        loop {
            interval.tick().await;
            
            let current_processed = self.get_total_processed().await;
            let current_tps = (current_processed - last_processed) as f64;
            last_processed = current_processed;

            let queue_depth: usize = self.message_queues.iter()
                .map(|queue| queue.blocking_read().len())
                .sum();

            let worker_utilization = self.calculate_worker_utilization().await;
            
            let mut metrics = self.metrics.write().await;
            metrics.current_tps = current_tps;
            metrics.queue_depth = queue_depth;
            metrics.worker_utilization = worker_utilization;
            
            let alpha = 0.1;
            metrics.average_tps = alpha * current_tps + (1.0 - alpha) * metrics.average_tps;
            
            if current_tps > metrics.peak_tps {
                metrics.peak_tps = current_tps;
            }

            let mut history = self.throughput_history.write().await;
            history.push_back(ThroughputSample {
                timestamp: Instant::now(),
                tps: current_tps,
                latency_ms: metrics.processing_latency_ms,
                queue_depth,
            });

            if history.len() > 3600 {
                history.pop_front();
            }
        }
    }

    async fn adaptive_batch_control_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.adjust_batch_size().await {
                warn!("Failed to adjust batch size: {}", e);
            }
        }
    }

    async fn adjust_batch_size(&self) -> Result<()> {
        let history = self.adaptive_controller.performance_history.read().await;
        if history.len() < 3 {
            return Ok(());
        }

        let recent_performances: Vec<_> = history.iter()
            .rev()
            .take(5)
            .collect();

        let avg_throughput: f64 = recent_performances.iter()
            .map(|p| p.throughput)
            .sum::<f64>() / recent_performances.len() as f64;

        let avg_latency: Duration = Duration::from_nanos(
            (recent_performances.iter()
                .map(|p| p.latency.as_nanos())
                .sum::<u128>() / recent_performances.len() as u128) as u64
        );

        let current_batch_size = *self.adaptive_controller.current_batch_size.read().await;
        let config = self.config.read().await;
        
        let target_latency = Duration::from_millis(50);
        let target_throughput = config.target_tps;

        let mut new_batch_size = current_batch_size;

        if avg_throughput < target_throughput * 0.9 && avg_latency < target_latency {
            new_batch_size = (current_batch_size as f64 * 1.1) as usize;
        } else if avg_latency > target_latency * 2 {
            new_batch_size = (current_batch_size as f64 * 0.9) as usize;
        }

        new_batch_size = new_batch_size
            .max(1)
            .min(config.max_batch_size);

        if new_batch_size != current_batch_size {
            *self.adaptive_controller.current_batch_size.write().await = new_batch_size;
            info!("Adjusted batch size from {} to {}", current_batch_size, new_batch_size);
        }

        Ok(())
    }

    async fn calculate_worker_utilization(&self) -> f64 {
        let available_permits = self.worker_semaphore.available_permits();
        let total_workers = self.config.read().await.worker_count;
        
        1.0 - (available_permits as f64 / total_workers as f64)
    }

    async fn get_total_processed(&self) -> u64 {
        0
    }

    pub async fn get_metrics(&self) -> ThroughputMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn get_throughput_trend(&self, duration: Duration) -> Vec<ThroughputSample> {
        let history = self.throughput_history.read().await;
        let cutoff = Instant::now() - duration;
        
        history.iter()
            .filter(|sample| sample.timestamp > cutoff)
            .cloned()
            .collect()
    }
}

impl<T> Clone for ThroughputOptimizer<T>
where
    T: Clone + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            message_queues: self.message_queues.clone(),
            worker_semaphore: self.worker_semaphore.clone(),
            metrics: self.metrics.clone(),
            throughput_history: self.throughput_history.clone(),
            adaptive_controller: self.adaptive_controller.clone(),
        }
    }
}

#[derive(Debug)]
struct ThroughputWorker<T, F, Fut>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(Vec<T>) -> Fut + Send + Sync + Clone + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    worker_id: usize,
    optimizer: ThroughputOptimizer<T>,
    processor: F,
}

impl<T, F, Fut> ThroughputWorker<T, F, Fut>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(Vec<T>) -> Fut + Send + Sync + Clone + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    fn new(
        worker_id: usize,
        optimizer: ThroughputOptimizer<T>,
        processor: F,
    ) -> Self {
        Self {
            worker_id,
            optimizer,
            processor,
        }
    }

    async fn run(&self) -> Result<()> {
        info!("Starting throughput worker {}", self.worker_id);
        
        loop {
            let _permit = self.optimizer.worker_semaphore.acquire().await?;
            
            let batch = self.optimizer.collect_batch().await;
            if batch.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }

            let start_time = Instant::now();
            let batch_size = batch.len();
            
            match timeout(Duration::from_secs(30), (self.processor)(batch)).await {
                Ok(Ok(())) => {
                    let processing_time = start_time.elapsed();
                    self.record_batch_performance(batch_size, processing_time).await;
                    debug!("Worker {} processed batch of {} in {:?}", 
                        self.worker_id, batch_size, processing_time);
                }
                Ok(Err(e)) => {
                    error!("Worker {} processing failed: {}", self.worker_id, e);
                }
                Err(_) => {
                    error!("Worker {} processing timed out", self.worker_id);
                }
            }
        }
    }

    async fn record_batch_performance(&self, batch_size: usize, processing_time: Duration) {
        let throughput = batch_size as f64 / processing_time.as_secs_f64();
        
        let performance = BatchPerformance {
            batch_size,
            throughput,
            latency: processing_time,
            timestamp: Instant::now(),
        };

        let mut history = self.optimizer.adaptive_controller.performance_history.write().await;
        history.push_back(performance);

        if history.len() > 1000 {
            history.pop_front();
        }

        let mut metrics = self.optimizer.metrics.write().await;
        let alpha = 0.1;
        metrics.processing_latency_ms = alpha * processing_time.as_millis() as f64 + 
            (1.0 - alpha) * metrics.processing_latency_ms;
        
        if batch_size > 0 {
            metrics.batch_efficiency = alpha * (batch_size as f64 / 
                self.optimizer.config.blocking_read().max_batch_size as f64) + 
                (1.0 - alpha) * metrics.batch_efficiency;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_throughput_optimizer_creation() {
        let config = ThroughputConfig::default();
        let optimizer: ThroughputOptimizer<String> = ThroughputOptimizer::new(config);
        
        let metrics = optimizer.get_metrics().await;
        assert_eq!(metrics.current_tps, 0.0);
    }

    #[tokio::test]
    async fn test_message_submission() {
        let config = ThroughputConfig::default();
        let optimizer: ThroughputOptimizer<String> = ThroughputOptimizer::new(config);
        
        let message = PrioritizedMessage::new(
            "test".to_string(),
            1,
            Address::random(),
        );
        
        let result = optimizer.submit_message(message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_batch_collection() {
        let config = ThroughputConfig {
            max_batch_size: 5,
            batch_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let optimizer: ThroughputOptimizer<String> = ThroughputOptimizer::new(config);
        
        for i in 0..3 {
            let message = PrioritizedMessage::new(
                format!("test_{}", i),
                1,
                Address::random(),
            );
            optimizer.submit_message(message).await.unwrap();
        }
        
        let batch = optimizer.collect_batch().await;
        assert_eq!(batch.len(), 3);
    }
}