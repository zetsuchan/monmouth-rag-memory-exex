use alloy::primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::time::timeout;

use crate::context::preprocessing::PreprocessedContext;
use crate::shared::types::AgentAction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    pub max_concurrent_embeddings: usize,
    pub embedding_timeout_ms: u64,
    pub priority_queue_size: usize,
    pub cache_ttl_seconds: u64,
    pub gpu_acceleration: bool,
    pub model_name: String,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            max_concurrent_embeddings: 10,
            embedding_timeout_ms: 100,
            priority_queue_size: 1000,
            cache_ttl_seconds: 300,
            gpu_acceleration: false,
            model_name: "all-MiniLM-L6-v2".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddingRequest {
    pub context: PreprocessedContext,
    pub priority: f64,
    pub requested_at: Instant,
    pub response_tx: mpsc::Sender<Result<Embedding, EmbeddingError>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embedding {
    pub vector: Vec<f32>,
    pub dimension: usize,
    pub context_hash: B256,
    pub agent_address: Address,
    pub generation_time_ms: u64,
    pub model_version: String,
}

pub struct RealtimeEmbeddingPipeline {
    config: StreamingConfig,
    request_queue: Arc<RwLock<priority_queue::PriorityQueue<String, OrderedFloat>>>,
    request_map: Arc<RwLock<HashMap<String, EmbeddingRequest>>>,
    embedding_cache: Arc<RwLock<lru::LruCache<CacheKey, CachedEmbedding>>>,
    semaphore: Arc<Semaphore>,
    metrics: Arc<RwLock<PipelineMetrics>>,
}

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    context_hash: B256,
    agent_address: Address,
}

#[derive(Debug, Clone)]
struct CachedEmbedding {
    embedding: Embedding,
    cached_at: Instant,
}

#[derive(Debug, Default)]
struct PipelineMetrics {
    total_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    total_generation_time_ms: u64,
    timeout_count: u64,
    error_count: u64,
}

impl RealtimeEmbeddingPipeline {
    pub fn new(config: StreamingConfig) -> Self {
        let cache_size = std::num::NonZeroUsize::new(10000).unwrap();
        
        Self {
            request_queue: Arc::new(RwLock::new(PriorityQueue::new())),
            request_map: Arc::new(RwLock::new(HashMap::new())),
            embedding_cache: Arc::new(RwLock::new(lru::LruCache::new(cache_size))),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_embeddings)),
            metrics: Arc::new(RwLock::new(PipelineMetrics::default())),
            config,
        }
    }

    pub async fn start(&self) {
        let workers = (0..self.config.max_concurrent_embeddings)
            .map(|i| self.spawn_worker(i))
            .collect::<Vec<_>>();

        // Start cache cleanup task
        self.spawn_cache_cleanup();

        // Wait for all workers
        futures::future::join_all(workers).await;
    }

    pub async fn generate_embedding(
        &self,
        context: PreprocessedContext,
    ) -> Result<Embedding, EmbeddingError> {
        let start_time = Instant::now();
        
        // Check cache first
        let cache_key = CacheKey {
            context_hash: context.transaction_hash,
            agent_address: context.agent_address,
        };

        if let Some(cached) = self.get_cached_embedding(&cache_key).await {
            self.record_cache_hit().await;
            return Ok(cached.embedding);
        }

        self.record_cache_miss().await;

        // Create response channel
        let (response_tx, mut response_rx) = mpsc::channel(1);

        // Create request
        let request = EmbeddingRequest {
            priority: context.priority_score,
            context,
            requested_at: Instant::now(),
            response_tx,
        };

        // Add to queue
        self.enqueue_request(request).await?;

        // Wait for response with timeout
        match timeout(
            Duration::from_millis(self.config.embedding_timeout_ms),
            response_rx.recv(),
        ).await {
            Ok(Some(result)) => {
                if let Ok(ref embedding) = result {
                    // Cache the result
                    self.cache_embedding(cache_key, embedding.clone()).await;
                    self.record_generation_time(start_time.elapsed().as_millis() as u64).await;
                }
                result
            }
            Ok(None) => {
                self.record_error().await;
                Err(EmbeddingError::ChannelClosed)
            }
            Err(_) => {
                self.record_timeout().await;
                Err(EmbeddingError::Timeout)
            }
        }
    }

    pub async fn generate_streaming(
        &self,
        contexts: impl futures::Stream<Item = PreprocessedContext>,
    ) -> impl futures::Stream<Item = Result<Embedding, EmbeddingError>> {
        use futures::StreamExt;
        
        contexts
            .map(|context| async move {
                self.generate_embedding(context).await
            })
            .buffer_unordered(self.config.max_concurrent_embeddings)
    }

    async fn enqueue_request(&self, request: EmbeddingRequest) -> Result<(), EmbeddingError> {
        let request_id = format!("{}-{}", 
            request.context.transaction_hash, 
            request.context.agent_address
        );

        let mut queue = self.request_queue.write().await;
        let mut map = self.request_map.write().await;

        if queue.len() >= self.config.priority_queue_size {
            return Err(EmbeddingError::QueueFull);
        }

        queue.push(request_id.clone(), OrderedFloat(request.priority));
        map.insert(request_id, request);

        Ok(())
    }

    fn spawn_worker(&self, worker_id: usize) -> tokio::task::JoinHandle<()> {
        let queue = self.request_queue.clone();
        let map = self.request_map.clone();
        let semaphore = self.semaphore.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            loop {
                // Acquire permit
                let _permit = semaphore.acquire().await.unwrap();

                // Get highest priority request
                let request_id = {
                    let mut queue = queue.write().await;
                    queue.pop().map(|(id, _)| id)
                };

                if let Some(request_id) = request_id {
                    let request = {
                        let mut map = map.write().await;
                        map.remove(&request_id)
                    };

                    if let Some(request) = request {
                        // Generate embedding
                        let result = Self::generate_embedding_internal(
                            &request.context,
                            &config,
                        ).await;

                        // Send response
                        let _ = request.response_tx.send(result).await;
                    }
                } else {
                    // No requests, sleep briefly
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        })
    }

    async fn generate_embedding_internal(
        context: &PreprocessedContext,
        config: &StreamingConfig,
    ) -> Result<Embedding, EmbeddingError> {
        let start = Instant::now();

        // Prepare text for embedding
        let text = Self::prepare_text(context);

        // Generate embedding vector
        let vector = if config.gpu_acceleration {
            Self::generate_gpu_embedding(&text, &config.model_name).await?
        } else {
            Self::generate_cpu_embedding(&text, &config.model_name).await?
        };

        let generation_time_ms = start.elapsed().as_millis() as u64;

        Ok(Embedding {
            dimension: vector.len(),
            vector,
            context_hash: context.transaction_hash,
            agent_address: context.agent_address,
            generation_time_ms,
            model_version: config.model_name.clone(),
        })
    }

    fn prepare_text(context: &PreprocessedContext) -> String {
        let mut parts = vec![];

        // Add action type
        match &context.action_type {
            AgentAction::Swap { token_in, token_out, .. } => {
                parts.push(format!("swap {} to {}", token_in, token_out));
            }
            AgentAction::AddLiquidity { token_a, token_b, .. } => {
                parts.push(format!("add liquidity {} {}", token_a, token_b));
            }
            AgentAction::RemoveLiquidity { .. } => {
                parts.push("remove liquidity".to_string());
            }
            AgentAction::Stake { .. } => {
                parts.push("stake tokens".to_string());
            }
            AgentAction::Unstake { .. } => {
                parts.push("unstake tokens".to_string());
            }
            AgentAction::Transfer { to, .. } => {
                parts.push(format!("transfer to {}", to));
            }
        }

        // Add semantic tags
        parts.extend(context.semantic_tags.clone());

        // Add feature information
        for (key, value) in &context.extracted_features {
            if *value > 0.0 {
                parts.push(format!("{}: {:.2}", key, value));
            }
        }

        parts.join(" ")
    }

    async fn generate_cpu_embedding(
        text: &str,
        model_name: &str,
    ) -> Result<Vec<f32>, EmbeddingError> {
        // Simplified embedding generation
        // In production, this would use a real embedding model
        use sha2::{Sha256, Digest};
        
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        hasher.update(model_name.as_bytes());
        let hash = hasher.finalize();

        // Generate deterministic pseudo-embeddings
        let mut embedding = Vec::with_capacity(384);
        for chunk in hash.chunks(4) {
            if chunk.len() == 4 {
                let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                embedding.push((value % 1.0).abs());
            }
        }

        // Pad to 384 dimensions
        while embedding.len() < 384 {
            embedding.push(0.0);
        }

        Ok(embedding)
    }

    async fn generate_gpu_embedding(
        text: &str,
        model_name: &str,
    ) -> Result<Vec<f32>, EmbeddingError> {
        // GPU acceleration would be implemented here
        // For now, fallback to CPU
        Self::generate_cpu_embedding(text, model_name).await
    }

    async fn get_cached_embedding(&self, key: &CacheKey) -> Option<CachedEmbedding> {
        let mut cache = self.embedding_cache.write().await;
        
        if let Some(cached) = cache.get(key) {
            let age = cached.cached_at.elapsed();
            if age < Duration::from_secs(self.config.cache_ttl_seconds) {
                return Some(cached.clone());
            } else {
                cache.pop(key);
            }
        }
        
        None
    }

    async fn cache_embedding(&self, key: CacheKey, embedding: Embedding) {
        let mut cache = self.embedding_cache.write().await;
        cache.put(key, CachedEmbedding {
            embedding,
            cached_at: Instant::now(),
        });
    }

    fn spawn_cache_cleanup(&self) {
        let cache = self.embedding_cache.clone();
        let ttl = self.config.cache_ttl_seconds;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            
            loop {
                interval.tick().await;
                
                let mut cache = cache.write().await;
                let now = Instant::now();
                let ttl_duration = Duration::from_secs(ttl);
                
                // Collect expired keys
                let expired_keys: Vec<_> = cache.iter()
                    .filter(|(_, v)| now.duration_since(v.cached_at) > ttl_duration)
                    .map(|(k, _)| k.clone())
                    .collect();
                
                // Remove expired entries
                for key in expired_keys {
                    cache.pop(&key);
                }
            }
        });
    }

    async fn record_cache_hit(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.cache_hits += 1;
    }

    async fn record_cache_miss(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.cache_misses += 1;
    }

    async fn record_generation_time(&self, time_ms: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.total_generation_time_ms += time_ms;
        metrics.total_requests += 1;
    }

    async fn record_timeout(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.timeout_count += 1;
    }

    async fn record_error(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.error_count += 1;
    }

    pub async fn get_metrics(&self) -> PipelineMetrics {
        self.metrics.read().await.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("Queue is full")]
    QueueFull,
    #[error("Embedding generation timeout")]
    Timeout,
    #[error("Channel closed")]
    ChannelClosed,
    #[error("Model error: {0}")]
    ModelError(String),
    #[error("GPU error: {0}")]
    GpuError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::types::AgentAction;
    use alloy::primitives::U256;

    #[tokio::test]
    async fn test_realtime_embedding_generation() {
        let config = StreamingConfig::default();
        let pipeline = RealtimeEmbeddingPipeline::new(config);

        // Start pipeline in background
        tokio::spawn(async move {
            pipeline.start().await;
        });

        // Give it time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        let context = PreprocessedContext {
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
        };

        let pipeline2 = RealtimeEmbeddingPipeline::new(StreamingConfig::default());
        let result = pipeline2.generate_embedding(context).await;
        
        assert!(result.is_ok());
        let embedding = result.unwrap();
        assert_eq!(embedding.dimension, 384);
        assert_eq!(embedding.vector.len(), 384);
    }
}