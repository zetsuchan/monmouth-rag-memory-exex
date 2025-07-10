use alloy::primitives::{Address, B256};
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;
use tracing::{debug, info, warn, error};

use super::ThroughputConfig;
use crate::shared::communication::CrossExExMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchingConfig {
    pub min_batch_size: usize,
    pub max_batch_size: usize,
    pub batch_timeout: Duration,
    pub enable_adaptive_sizing: bool,
    pub enable_compression: bool,
    pub compression_threshold: usize,
    pub max_pending_batches: usize,
    pub batch_priority_levels: usize,
}

impl Default for BatchingConfig {
    fn default() -> Self {
        Self {
            min_batch_size: 1,
            max_batch_size: 100,
            batch_timeout: Duration::from_millis(50),
            enable_adaptive_sizing: true,
            enable_compression: true,
            compression_threshold: 1024,
            max_pending_batches: 1000,
            batch_priority_levels: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMetrics {
    pub total_batches_created: u64,
    pub total_messages_batched: u64,
    pub average_batch_size: f64,
    pub batch_efficiency: f64,
    pub compression_ratio: f64,
    pub average_batch_latency_ms: f64,
    pub timeout_batches: u64,
    pub oversized_batches: u64,
}

impl Default for BatchMetrics {
    fn default() -> Self {
        Self {
            total_batches_created: 0,
            total_messages_batched: 0,
            average_batch_size: 0.0,
            batch_efficiency: 0.0,
            compression_ratio: 1.0,
            average_batch_latency_ms: 0.0,
            timeout_batches: 0,
            oversized_batches: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatchedMessage<T> {
    pub data: T,
    pub priority: u8,
    pub message_id: B256,
    pub created_at: Instant,
    pub requester: Address,
    pub size_bytes: usize,
}

impl<T> BatchedMessage<T> {
    pub fn new(data: T, priority: u8, requester: Address, size_bytes: usize) -> Self {
        Self {
            data,
            priority,
            message_id: B256::random(),
            created_at: Instant::now(),
            requester,
            size_bytes,
        }
    }

    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

#[derive(Debug, Clone)]
pub struct Batch<T> {
    pub batch_id: B256,
    pub messages: Vec<BatchedMessage<T>>,
    pub created_at: Instant,
    pub priority: u8,
    pub total_size_bytes: usize,
    pub is_compressed: bool,
}

impl<T> Batch<T> {
    pub fn new(priority: u8) -> Self {
        Self {
            batch_id: B256::random(),
            messages: Vec::new(),
            created_at: Instant::now(),
            priority,
            total_size_bytes: 0,
            is_compressed: false,
        }
    }

    pub fn add_message(&mut self, message: BatchedMessage<T>) {
        self.total_size_bytes += message.size_bytes;
        self.messages.push(message);
    }

    pub fn is_full(&self, max_size: usize) -> bool {
        self.messages.len() >= max_size
    }

    pub fn is_oversize(&self, max_bytes: usize) -> bool {
        self.total_size_bytes > max_bytes
    }

    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    pub fn average_message_age(&self) -> Duration {
        if self.messages.is_empty() {
            return Duration::ZERO;
        }

        let total_age: Duration = self.messages.iter()
            .map(|msg| msg.age())
            .sum();
        
        total_age / self.messages.len() as u32
    }
}

#[derive(Debug)]
pub struct BatchProcessor<T> 
where 
    T: Clone + Send + Sync + 'static,
{
    config: Arc<RwLock<BatchingConfig>>,
    pending_batches: Arc<RwLock<HashMap<u8, VecDeque<Batch<T>>>>>,
    active_batches: Arc<RwLock<HashMap<u8, Batch<T>>>>,
    metrics: Arc<RwLock<BatchMetrics>>,
    batch_sender: mpsc::Sender<Batch<T>>,
    batch_receiver: Arc<RwLock<Option<mpsc::Receiver<Batch<T>>>>>,
    adaptive_controller: Arc<AdaptiveBatchSizer>,
}

#[derive(Debug)]
struct AdaptiveBatchSizer {
    optimal_sizes: Arc<RwLock<HashMap<u8, usize>>>,
    performance_history: Arc<RwLock<VecDeque<BatchPerformanceRecord>>>,
    adjustment_window: Duration,
}

#[derive(Debug, Clone)]
struct BatchPerformanceRecord {
    priority: u8,
    batch_size: usize,
    processing_time: Duration,
    throughput: f64,
    timestamp: Instant,
}

impl<T> BatchProcessor<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(config: BatchingConfig) -> Self {
        let (batch_sender, batch_receiver) = mpsc::channel(config.max_pending_batches);
        
        let adaptive_controller = Arc::new(AdaptiveBatchSizer {
            optimal_sizes: Arc::new(RwLock::new(HashMap::new())),
            performance_history: Arc::new(RwLock::new(VecDeque::new())),
            adjustment_window: Duration::from_secs(60),
        });

        for priority in 0..config.batch_priority_levels {
            adaptive_controller.optimal_sizes.blocking_write()
                .insert(priority as u8, config.max_batch_size / 2);
        }

        Self {
            config: Arc::new(RwLock::new(config)),
            pending_batches: Arc::new(RwLock::new(HashMap::new())),
            active_batches: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(BatchMetrics::default())),
            batch_sender,
            batch_receiver: Arc::new(RwLock::new(Some(batch_receiver))),
            adaptive_controller,
        }
    }

    pub async fn add_message(&self, message: BatchedMessage<T>) -> Result<()> {
        let priority = message.priority;
        let mut active_batches = self.active_batches.write().await;
        
        let batch = active_batches.entry(priority).or_insert_with(|| {
            debug!("Creating new batch for priority {}", priority);
            Batch::new(priority)
        });

        batch.add_message(message);

        let config = self.config.read().await;
        let should_complete = self.should_complete_batch(batch, &config).await;
        
        if should_complete {
            let completed_batch = active_batches.remove(&priority).unwrap();
            self.complete_batch(completed_batch).await?;
        }

        Ok(())
    }

    async fn should_complete_batch(&self, batch: &Batch<T>, config: &BatchingConfig) -> bool {
        if batch.messages.is_empty() {
            return false;
        }

        let optimal_size = if config.enable_adaptive_sizing {
            self.adaptive_controller.optimal_sizes.read().await
                .get(&batch.priority)
                .copied()
                .unwrap_or(config.max_batch_size)
        } else {
            config.max_batch_size
        };

        batch.is_full(optimal_size) || 
        batch.age() >= config.batch_timeout ||
        batch.is_oversize(1024 * 1024)
    }

    async fn complete_batch(&self, mut batch: Batch<T>) -> Result<()> {
        let config = self.config.read().await;
        
        if config.enable_compression && batch.total_size_bytes > config.compression_threshold {
            batch.is_compressed = true;
        }

        let batch_size = batch.messages.len();
        let was_timeout = batch.age() >= config.batch_timeout;
        
        self.update_metrics(&batch, was_timeout).await;
        
        if let Err(e) = self.batch_sender.send(batch).await {
            error!("Failed to send completed batch: {}", e);
            return Err(e.into());
        }

        debug!("Completed batch with {} messages (timeout: {})", batch_size, was_timeout);
        Ok(())
    }

    async fn update_metrics(&self, batch: &Batch<T>, was_timeout: bool) {
        let mut metrics = self.metrics.write().await;
        
        metrics.total_batches_created += 1;
        metrics.total_messages_batched += batch.messages.len() as u64;
        
        let alpha = 0.1;
        let batch_size = batch.messages.len() as f64;
        metrics.average_batch_size = alpha * batch_size + (1.0 - alpha) * metrics.average_batch_size;
        
        let config = self.config.read().await;
        let efficiency = batch_size / config.max_batch_size as f64;
        metrics.batch_efficiency = alpha * efficiency + (1.0 - alpha) * metrics.batch_efficiency;
        
        let latency_ms = batch.age().as_millis() as f64;
        metrics.average_batch_latency_ms = alpha * latency_ms + (1.0 - alpha) * metrics.average_batch_latency_ms;
        
        if was_timeout {
            metrics.timeout_batches += 1;
        }
        
        if batch.is_oversize(1024 * 1024) {
            metrics.oversized_batches += 1;
        }
    }

    pub async fn start_batch_completion_monitor(&self) {
        let processor = self.clone();
        let config = self.config.read().await.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.batch_timeout / 2);
            
            loop {
                interval.tick().await;
                if let Err(e) = processor.check_and_complete_stale_batches().await {
                    warn!("Error checking stale batches: {}", e);
                }
            }
        });
    }

    async fn check_and_complete_stale_batches(&self) -> Result<()> {
        let config = self.config.read().await;
        let mut active_batches = self.active_batches.write().await;
        let mut completed_batches = Vec::new();
        
        for (priority, batch) in active_batches.iter() {
            if batch.age() >= config.batch_timeout && !batch.messages.is_empty() {
                completed_batches.push(*priority);
            }
        }
        
        for priority in completed_batches {
            if let Some(batch) = active_batches.remove(&priority) {
                drop(active_batches);
                self.complete_batch(batch).await?;
                active_batches = self.active_batches.write().await;
            }
        }
        
        Ok(())
    }

    pub async fn start_adaptive_sizing(&self) {
        if !self.config.read().await.enable_adaptive_sizing {
            return;
        }

        let processor = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                if let Err(e) = processor.adjust_batch_sizes().await {
                    warn!("Error adjusting batch sizes: {}", e);
                }
            }
        });
    }

    async fn adjust_batch_sizes(&self) -> Result<()> {
        let history = self.adaptive_controller.performance_history.read().await;
        let cutoff = Instant::now() - self.adaptive_controller.adjustment_window;
        
        let mut priority_performances: HashMap<u8, Vec<&BatchPerformanceRecord>> = HashMap::new();
        
        for record in history.iter() {
            if record.timestamp > cutoff {
                priority_performances.entry(record.priority)
                    .or_insert_with(Vec::new)
                    .push(record);
            }
        }

        let mut optimal_sizes = self.adaptive_controller.optimal_sizes.write().await;
        let config = self.config.read().await;
        
        for (priority, records) in priority_performances {
            if records.len() < 3 {
                continue;
            }

            let avg_throughput: f64 = records.iter()
                .map(|r| r.throughput)
                .sum::<f64>() / records.len() as f64;

            let best_record = records.iter()
                .max_by(|a, b| a.throughput.partial_cmp(&b.throughput).unwrap_or(std::cmp::Ordering::Equal));

            if let Some(best) = best_record {
                let current_optimal = optimal_sizes.get(&priority).copied()
                    .unwrap_or(config.max_batch_size / 2);
                
                let new_optimal = if best.throughput > avg_throughput * 1.1 {
                    (best.batch_size as f64 * 1.1) as usize
                } else {
                    (current_optimal as f64 * 0.95) as usize
                };

                let clamped_optimal = new_optimal
                    .max(config.min_batch_size)
                    .min(config.max_batch_size);

                optimal_sizes.insert(priority, clamped_optimal);
                
                debug!("Adjusted optimal batch size for priority {} from {} to {}", 
                    priority, current_optimal, clamped_optimal);
            }
        }

        Ok(())
    }

    pub async fn record_batch_performance(
        &self,
        batch_id: B256,
        priority: u8,
        batch_size: usize,
        processing_time: Duration,
    ) {
        let throughput = batch_size as f64 / processing_time.as_secs_f64();
        
        let record = BatchPerformanceRecord {
            priority,
            batch_size,
            processing_time,
            throughput,
            timestamp: Instant::now(),
        };

        let mut history = self.adaptive_controller.performance_history.write().await;
        history.push_back(record);

        if history.len() > 10000 {
            history.pop_front();
        }

        debug!("Recorded batch performance: {} msgs in {:?} ({:.2} msg/s)", 
            batch_size, processing_time, throughput);
    }

    pub async fn get_batch_receiver(&self) -> Option<mpsc::Receiver<Batch<T>>> {
        self.batch_receiver.write().await.take()
    }

    pub async fn get_metrics(&self) -> BatchMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn get_pending_batch_count(&self) -> usize {
        self.pending_batches.read().await.values()
            .map(|queue| queue.len())
            .sum()
    }

    pub async fn get_active_batch_info(&self) -> HashMap<u8, (usize, Duration)> {
        let active_batches = self.active_batches.read().await;
        
        active_batches.iter()
            .map(|(priority, batch)| (*priority, (batch.messages.len(), batch.age())))
            .collect()
    }
}

impl<T> Clone for BatchProcessor<T>
where
    T: Clone + Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            pending_batches: self.pending_batches.clone(),
            active_batches: self.active_batches.clone(),
            metrics: self.metrics.clone(),
            batch_sender: self.batch_sender.clone(),
            batch_receiver: self.batch_receiver.clone(),
            adaptive_controller: self.adaptive_controller.clone(),
        }
    }
}

pub struct BatchedMessageProcessor<T>
where
    T: Clone + Send + Sync + 'static,
{
    batch_processor: BatchProcessor<T>,
}

impl<T> BatchedMessageProcessor<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(config: BatchingConfig) -> Self {
        Self {
            batch_processor: BatchProcessor::new(config),
        }
    }

    pub async fn start_processing<F, Fut>(&self, processor: F) -> Result<()>
    where
        F: Fn(Vec<T>) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let mut batch_receiver = self.batch_processor.get_batch_receiver().await
            .ok_or_else(|| eyre::eyre!("Batch receiver already taken"))?;

        self.batch_processor.start_batch_completion_monitor().await;
        self.batch_processor.start_adaptive_sizing().await;

        let batch_processor = self.batch_processor.clone();
        
        tokio::spawn(async move {
            while let Some(batch) = batch_receiver.recv().await {
                let start_time = Instant::now();
                let batch_id = batch.batch_id;
                let priority = batch.priority;
                let batch_size = batch.messages.len();
                
                let messages: Vec<T> = batch.messages.into_iter()
                    .map(|msg| msg.data)
                    .collect();

                match timeout(Duration::from_secs(60), processor(messages)).await {
                    Ok(Ok(())) => {
                        let processing_time = start_time.elapsed();
                        batch_processor.record_batch_performance(
                            batch_id,
                            priority,
                            batch_size,
                            processing_time,
                        ).await;
                        
                        info!("Successfully processed batch {} with {} messages in {:?}",
                            batch_id, batch_size, processing_time);
                    }
                    Ok(Err(e)) => {
                        error!("Batch processing failed for batch {}: {}", batch_id, e);
                    }
                    Err(_) => {
                        error!("Batch processing timed out for batch {}", batch_id);
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn submit_message(&self, message: BatchedMessage<T>) -> Result<()> {
        self.batch_processor.add_message(message).await
    }

    pub async fn get_metrics(&self) -> BatchMetrics {
        self.batch_processor.get_metrics().await
    }

    pub async fn get_status(&self) -> BatchProcessorStatus {
        let metrics = self.get_metrics().await;
        let pending_count = self.batch_processor.get_pending_batch_count().await;
        let active_batches = self.batch_processor.get_active_batch_info().await;

        BatchProcessorStatus {
            metrics,
            pending_batch_count: pending_count,
            active_batches,
            is_healthy: pending_count < 1000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProcessorStatus {
    pub metrics: BatchMetrics,
    pub pending_batch_count: usize,
    pub active_batches: HashMap<u8, (usize, Duration)>,
    pub is_healthy: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batch_processor_creation() {
        let config = BatchingConfig::default();
        let processor: BatchProcessor<String> = BatchProcessor::new(config);
        
        let metrics = processor.get_metrics().await;
        assert_eq!(metrics.total_batches_created, 0);
    }

    #[tokio::test]
    async fn test_message_batching() {
        let config = BatchingConfig {
            max_batch_size: 3,
            batch_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let processor: BatchProcessor<String> = BatchProcessor::new(config);
        
        let message1 = BatchedMessage::new(
            "test1".to_string(),
            1,
            Address::random(),
            10,
        );
        
        let message2 = BatchedMessage::new(
            "test2".to_string(),
            1,
            Address::random(),
            10,
        );

        processor.add_message(message1).await.unwrap();
        processor.add_message(message2).await.unwrap();
        
        let active_batches = processor.get_active_batch_info().await;
        assert!(active_batches.contains_key(&1));
    }

    #[tokio::test]
    async fn test_batch_completion() {
        let config = BatchingConfig {
            max_batch_size: 2,
            batch_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let processor: BatchProcessor<String> = BatchProcessor::new(config);
        
        let message1 = BatchedMessage::new(
            "test1".to_string(),
            1,
            Address::random(),
            10,
        );
        
        let message2 = BatchedMessage::new(
            "test2".to_string(),
            1,
            Address::random(),
            10,
        );

        processor.add_message(message1).await.unwrap();
        processor.add_message(message2).await.unwrap();
        
        let metrics = processor.get_metrics().await;
        assert!(metrics.total_batches_created > 0);
    }
}