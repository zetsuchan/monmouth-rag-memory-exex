use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};

use crate::context::preprocessing::PreprocessedContext;
use super::realtime::{Embedding, EmbeddingError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    pub batch_size: usize,
    pub batch_timeout_ms: u64,
    pub max_concurrent_batches: usize,
    pub priority_threshold: f64,
    pub compression_enabled: bool,
    pub parallel_workers: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_size: 32,
            batch_timeout_ms: 500,
            max_concurrent_batches: 4,
            priority_threshold: 0.5,
            compression_enabled: true,
            parallel_workers: 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub contexts: Vec<PreprocessedContext>,
    pub priority: f64,
    pub requested_at: Instant,
    pub response_tx: mpsc::Sender<BatchResult>,
}

#[derive(Debug, Clone)]
pub struct BatchResult {
    pub embeddings: Vec<Embedding>,
    pub failed_indices: Vec<(usize, EmbeddingError)>,
    pub batch_id: String,
    pub processing_time_ms: u64,
}

pub struct BatchProcessor {
    config: BatchConfig,
    pending_batches: Arc<RwLock<VecDeque<PendingBatch>>>,
    active_batches: Arc<RwLock<HashMap<String, ActiveBatch>>>,
    semaphore: Arc<Semaphore>,
    metrics: Arc<RwLock<BatchMetrics>>,
}

#[derive(Debug)]
struct PendingBatch {
    contexts: Vec<PreprocessedContext>,
    priority: f64,
    created_at: Instant,
    response_txs: Vec<mpsc::Sender<BatchResult>>,
}

#[derive(Debug)]
struct ActiveBatch {
    batch_id: String,
    contexts: Vec<PreprocessedContext>,
    started_at: Instant,
}

#[derive(Debug, Default)]
struct BatchMetrics {
    total_batches: u64,
    total_contexts: u64,
    total_failures: u64,
    average_batch_size: f64,
    average_processing_time_ms: f64,
}

impl BatchProcessor {
    pub fn new(config: BatchConfig) -> Self {
        Self {
            pending_batches: Arc::new(RwLock::new(VecDeque::new())),
            active_batches: Arc::new(RwLock::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_batches)),
            metrics: Arc::new(RwLock::new(BatchMetrics::default())),
            config,
        }
    }

    pub async fn start(&self) {
        // Start batch collection task
        self.spawn_batch_collector();

        // Start processing workers
        for i in 0..self.config.parallel_workers {
            self.spawn_processor(i);
        }
    }

    pub async fn submit_contexts(
        &self,
        contexts: Vec<PreprocessedContext>,
    ) -> Result<BatchResult, BatchError> {
        if contexts.is_empty() {
            return Err(BatchError::EmptyBatch);
        }

        let (response_tx, mut response_rx) = mpsc::channel(1);
        
        // Calculate batch priority
        let priority = contexts.iter()
            .map(|c| c.priority_score)
            .sum::<f64>() / contexts.len() as f64;

        // Add to pending batches
        {
            let mut pending = self.pending_batches.write().await;
            
            // Check if we can merge with existing batch
            let merged = self.try_merge_batch(&mut pending, contexts.clone(), priority, response_tx.clone()).await;
            
            if !merged {
                // Create new pending batch
                let batch = PendingBatch {
                    contexts,
                    priority,
                    created_at: Instant::now(),
                    response_txs: vec![response_tx],
                };
                
                // Insert based on priority
                let insert_pos = pending.iter()
                    .position(|b| b.priority < priority)
                    .unwrap_or(pending.len());
                
                pending.insert(insert_pos, batch);
            }
        }

        // Wait for result
        match response_rx.recv().await {
            Some(result) => Ok(result),
            None => Err(BatchError::ProcessingFailed("Channel closed".to_string())),
        }
    }

    pub async fn submit_stream(
        &self,
        mut stream: impl futures::Stream<Item = PreprocessedContext> + Unpin,
    ) -> Result<Vec<BatchResult>, BatchError> {
        use futures::StreamExt;
        
        let mut current_batch = Vec::new();
        let mut results = Vec::new();

        while let Some(context) = stream.next().await {
            current_batch.push(context);

            if current_batch.len() >= self.config.batch_size {
                let batch = std::mem::take(&mut current_batch);
                let result = self.submit_contexts(batch).await?;
                results.push(result);
            }
        }

        // Process remaining contexts
        if !current_batch.is_empty() {
            let result = self.submit_contexts(current_batch).await?;
            results.push(result);
        }

        Ok(results)
    }

    async fn try_merge_batch(
        &self,
        pending: &mut VecDeque<PendingBatch>,
        contexts: Vec<PreprocessedContext>,
        priority: f64,
        response_tx: mpsc::Sender<BatchResult>,
    ) -> bool {
        for batch in pending.iter_mut() {
            // Check if we can merge
            if batch.contexts.len() + contexts.len() <= self.config.batch_size &&
               (batch.priority - priority).abs() < 0.1 &&
               batch.created_at.elapsed() < Duration::from_millis(self.config.batch_timeout_ms / 2) {
                
                batch.contexts.extend(contexts);
                batch.response_txs.push(response_tx);
                batch.priority = (batch.priority + priority) / 2.0;
                return true;
            }
        }
        false
    }

    fn spawn_batch_collector(&self) {
        let pending_batches = self.pending_batches.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(config.batch_timeout_ms / 2));
            
            loop {
                interval.tick().await;
                
                let mut pending = pending_batches.write().await;
                let now = Instant::now();
                
                // Collect batches that are ready
                let mut ready_batches = Vec::new();
                
                while let Some(batch) = pending.front() {
                    let age = now.duration_since(batch.created_at);
                    
                    if batch.contexts.len() >= config.batch_size || 
                       age >= Duration::from_millis(config.batch_timeout_ms) {
                        ready_batches.push(pending.pop_front().unwrap());
                    } else {
                        break; // Batches are ordered by priority, so we can stop
                    }
                }
                
                drop(pending); // Release lock
                
                // Process ready batches
                for batch in ready_batches {
                    // Notify processor
                    // In a real implementation, this would send to a processing queue
                }
            }
        });
    }

    fn spawn_processor(&self, worker_id: usize) {
        let pending_batches = self.pending_batches.clone();
        let active_batches = self.active_batches.clone();
        let semaphore = self.semaphore.clone();
        let config = self.config.clone();
        let metrics = self.metrics.clone();

        tokio::spawn(async move {
            loop {
                // Acquire permit
                let _permit = semaphore.acquire().await.unwrap();

                // Get next batch
                let batch = {
                    let mut pending = pending_batches.write().await;
                    pending.pop_front()
                };

                if let Some(batch) = batch {
                    let batch_id = format!("batch_{}_{}_{}", 
                        worker_id, 
                        chrono::Utc::now().timestamp_millis(),
                        batch.priority
                    );

                    // Mark as active
                    {
                        let mut active = active_batches.write().await;
                        active.insert(batch_id.clone(), ActiveBatch {
                            batch_id: batch_id.clone(),
                            contexts: batch.contexts.clone(),
                            started_at: Instant::now(),
                        });
                    }

                    // Process batch
                    let result = Self::process_batch_internal(
                        batch_id.clone(),
                        batch.contexts,
                        &config,
                    ).await;

                    // Remove from active
                    {
                        let mut active = active_batches.write().await;
                        active.remove(&batch_id);
                    }

                    // Update metrics
                    Self::update_metrics(&metrics, &result).await;

                    // Send results to all waiting channels
                    for tx in batch.response_txs {
                        let _ = tx.send(result.clone()).await;
                    }
                } else {
                    // No batches, sleep briefly
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        });
    }

    async fn process_batch_internal(
        batch_id: String,
        contexts: Vec<PreprocessedContext>,
        config: &BatchConfig,
    ) -> BatchResult {
        let start = Instant::now();
        let mut embeddings = Vec::with_capacity(contexts.len());
        let mut failed_indices = Vec::new();

        // Process contexts in parallel chunks
        let chunk_size = contexts.len() / config.parallel_workers.max(1);
        let mut handles = Vec::new();

        for (chunk_idx, chunk) in contexts.chunks(chunk_size).enumerate() {
            let chunk = chunk.to_vec();
            let config = config.clone();
            
            let handle = tokio::spawn(async move {
                let mut chunk_embeddings = Vec::new();
                let mut chunk_failures = Vec::new();

                for (idx, context) in chunk.iter().enumerate() {
                    match Self::generate_embedding_for_context(context, &config).await {
                        Ok(embedding) => chunk_embeddings.push((chunk_idx * chunk_size + idx, embedding)),
                        Err(e) => chunk_failures.push((chunk_idx * chunk_size + idx, e)),
                    }
                }

                (chunk_embeddings, chunk_failures)
            });

            handles.push(handle);
        }

        // Collect results
        for handle in handles {
            if let Ok((chunk_embeddings, chunk_failures)) = handle.await {
                for (idx, embedding) in chunk_embeddings {
                    embeddings.push(embedding);
                }
                failed_indices.extend(chunk_failures);
            }
        }

        let processing_time_ms = start.elapsed().as_millis() as u64;

        BatchResult {
            embeddings,
            failed_indices,
            batch_id,
            processing_time_ms,
        }
    }

    async fn generate_embedding_for_context(
        context: &PreprocessedContext,
        config: &BatchConfig,
    ) -> Result<Embedding, EmbeddingError> {
        // Simulate embedding generation
        // In production, this would use the actual embedding model
        
        let start = Instant::now();
        
        // Apply compression if enabled
        let processed_features = if config.compression_enabled {
            Self::compress_features(&context.extracted_features)
        } else {
            context.extracted_features.clone()
        };

        // Generate embedding vector (simplified)
        let mut vector = Vec::with_capacity(384);
        
        // Use context data to generate deterministic embeddings
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(context.transaction_hash.as_bytes());
        hasher.update(context.agent_address.as_bytes());
        
        for tag in &context.semantic_tags {
            hasher.update(tag.as_bytes());
        }
        
        let hash = hasher.finalize();
        
        for i in 0..384 {
            let byte_idx = i % 32;
            let value = (hash[byte_idx] as f32 / 255.0) * (context.priority_score as f32);
            vector.push(value);
        }

        let generation_time_ms = start.elapsed().as_millis() as u64;

        Ok(Embedding {
            vector,
            dimension: 384,
            context_hash: context.transaction_hash,
            agent_address: context.agent_address,
            generation_time_ms,
            model_version: "batch-v1".to_string(),
        })
    }

    fn compress_features(features: &HashMap<String, f64>) -> HashMap<String, f64> {
        // Simple feature compression: keep only significant features
        features.iter()
            .filter(|(_, &value)| value.abs() > 0.01)
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    async fn update_metrics(metrics: &Arc<RwLock<BatchMetrics>>, result: &BatchResult) {
        let mut m = metrics.write().await;
        
        m.total_batches += 1;
        m.total_contexts += result.embeddings.len() as u64;
        m.total_failures += result.failed_indices.len() as u64;
        
        // Update averages
        let batch_size = result.embeddings.len() as f64;
        m.average_batch_size = (m.average_batch_size * (m.total_batches - 1) as f64 + batch_size) / m.total_batches as f64;
        
        let processing_time = result.processing_time_ms as f64;
        m.average_processing_time_ms = (m.average_processing_time_ms * (m.total_batches - 1) as f64 + processing_time) / m.total_batches as f64;
    }

    pub async fn get_metrics(&self) -> BatchMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn get_active_batches(&self) -> Vec<String> {
        self.active_batches.read().await
            .keys()
            .cloned()
            .collect()
    }

    pub async fn get_pending_count(&self) -> usize {
        self.pending_batches.read().await.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error("Empty batch submitted")]
    EmptyBatch,
    #[error("Batch processing failed: {0}")]
    ProcessingFailed(String),
    #[error("Batch timeout")]
    Timeout,
    #[error("Resource exhausted")]
    ResourceExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::types::AgentAction;
    use alloy::primitives::U256;

    #[tokio::test]
    async fn test_batch_processing() {
        let config = BatchConfig {
            batch_size: 2,
            batch_timeout_ms: 100,
            ..Default::default()
        };
        
        let processor = BatchProcessor::new(config);
        processor.start().await;

        let contexts = vec![
            PreprocessedContext {
                transaction_hash: B256::from([1u8; 32]),
                agent_address: Address::from([2u8; 20]),
                action_type: AgentAction::Transfer {
                    to: Address::from([3u8; 20]),
                    amount: U256::from(1000),
                },
                extracted_features: Default::default(),
                semantic_tags: vec!["transfer".to_string()],
                priority_score: 0.8,
                timestamp: 12345,
                compressed_data: None,
            },
            PreprocessedContext {
                transaction_hash: B256::from([2u8; 32]),
                agent_address: Address::from([3u8; 20]),
                action_type: AgentAction::Swap {
                    token_in: Address::from([4u8; 20]),
                    token_out: Address::from([5u8; 20]),
                    amount_in: U256::from(500),
                    amount_out: U256::from(450),
                },
                extracted_features: Default::default(),
                semantic_tags: vec!["swap".to_string()],
                priority_score: 0.9,
                timestamp: 12346,
                compressed_data: None,
            },
        ];

        let result = processor.submit_contexts(contexts).await.unwrap();
        
        assert_eq!(result.embeddings.len(), 2);
        assert!(result.failed_indices.is_empty());
        assert!(!result.batch_id.is_empty());
    }
}