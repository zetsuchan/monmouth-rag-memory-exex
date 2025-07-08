use prometheus::{Registry, Counter, Histogram, HistogramOpts, Gauge};
use eyre::Result;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Metrics {
    registry: Arc<Registry>,
    
    pub blocks_processed: Counter,
    pub transactions_processed: Counter,
    
    pub rag_queries: Counter,
    pub rag_query_latency: Histogram,
    pub embeddings_generated: Counter,
    pub embedding_latency: Histogram,
    pub vector_store_size: Gauge,
    
    pub memories_stored: Counter,
    pub memories_retrieved: Counter,
    pub memory_store_latency: Histogram,
    pub memory_retrieve_latency: Histogram,
    pub active_sessions: Gauge,
    pub total_memory_size: Gauge,
    
    pub agent_active_count: Gauge,
    pub anps_scores: Histogram,
}

impl Metrics {
    pub fn new() -> Result<Self> {
        let registry = Arc::new(Registry::new());
        
        let blocks_processed = Counter::new("blocks_processed", "Total blocks processed")?;
        let transactions_processed = Counter::new("transactions_processed", "Total transactions processed")?;
        
        let rag_queries = Counter::new("rag_queries", "Total RAG queries executed")?;
        let rag_query_latency = Histogram::with_opts(
            HistogramOpts::new("rag_query_latency", "RAG query latency in milliseconds")
        )?;
        let embeddings_generated = Counter::new("embeddings_generated", "Total embeddings generated")?;
        let embedding_latency = Histogram::with_opts(
            HistogramOpts::new("embedding_latency", "Embedding generation latency in milliseconds")
        )?;
        let vector_store_size = Gauge::new("vector_store_size", "Number of vectors in store")?;
        
        let memories_stored = Counter::new("memories_stored", "Total memories stored")?;
        let memories_retrieved = Counter::new("memories_retrieved", "Total memories retrieved")?;
        let memory_store_latency = Histogram::with_opts(
            HistogramOpts::new("memory_store_latency", "Memory store latency in milliseconds")
        )?;
        let memory_retrieve_latency = Histogram::with_opts(
            HistogramOpts::new("memory_retrieve_latency", "Memory retrieve latency in milliseconds")
        )?;
        let active_sessions = Gauge::new("active_sessions", "Number of active ephemeral sessions")?;
        let total_memory_size = Gauge::new("total_memory_size", "Total memory size in bytes")?;
        
        let agent_active_count = Gauge::new("agent_active_count", "Number of active agents")?;
        let anps_scores = Histogram::with_opts(
            HistogramOpts::new("anps_scores", "Agent Neural Performance Scores")
        )?;
        
        registry.register(Box::new(blocks_processed.clone()))?;
        registry.register(Box::new(transactions_processed.clone()))?;
        registry.register(Box::new(rag_queries.clone()))?;
        registry.register(Box::new(rag_query_latency.clone()))?;
        registry.register(Box::new(embeddings_generated.clone()))?;
        registry.register(Box::new(embedding_latency.clone()))?;
        registry.register(Box::new(vector_store_size.clone()))?;
        registry.register(Box::new(memories_stored.clone()))?;
        registry.register(Box::new(memories_retrieved.clone()))?;
        registry.register(Box::new(memory_store_latency.clone()))?;
        registry.register(Box::new(memory_retrieve_latency.clone()))?;
        registry.register(Box::new(active_sessions.clone()))?;
        registry.register(Box::new(total_memory_size.clone()))?;
        registry.register(Box::new(agent_active_count.clone()))?;
        registry.register(Box::new(anps_scores.clone()))?;
        
        Ok(Self {
            registry,
            blocks_processed,
            transactions_processed,
            rag_queries,
            rag_query_latency,
            embeddings_generated,
            embedding_latency,
            vector_store_size,
            memories_stored,
            memories_retrieved,
            memory_store_latency,
            memory_retrieve_latency,
            active_sessions,
            total_memory_size,
            agent_active_count,
            anps_scores,
        })
    }
    
    pub fn record_block_processed(&self) {
        self.blocks_processed.inc();
    }
    
    pub fn record_transaction_processed(&self) {
        self.transactions_processed.inc();
    }
    
    pub fn record_rag_query(&self) {
        self.rag_queries.inc();
    }
    
    pub fn record_rag_query_time(&self, duration_ms: f64) {
        self.rag_query_latency.observe(duration_ms);
    }
    
    pub fn record_embedding_generated(&self) {
        self.embeddings_generated.inc();
    }
    
    pub fn record_embedding_time(&self, duration_ms: f64) {
        self.embedding_latency.observe(duration_ms);
    }
    
    pub fn update_vector_store_size(&self, size: f64) {
        self.vector_store_size.set(size);
    }
    
    pub fn record_memory_stored(&self) {
        self.memories_stored.inc();
    }
    
    pub fn record_memory_retrieved(&self) {
        self.memories_retrieved.inc();
    }
    
    pub fn record_memory_store_time(&self, duration_ms: f64) {
        self.memory_store_latency.observe(duration_ms);
    }
    
    pub fn record_memory_retrieve_time(&self, duration_ms: f64) {
        self.memory_retrieve_latency.observe(duration_ms);
    }
    
    pub fn update_active_sessions(&self, count: f64) {
        self.active_sessions.set(count);
    }
    
    pub fn update_total_memory_size(&self, size: f64) {
        self.total_memory_size.set(size);
    }
    
    pub fn update_agent_count(&self, count: f64) {
        self.agent_active_count.set(count);
    }
    
    pub fn record_anps_score(&self, score: f64) {
        self.anps_scores.observe(score);
    }
    
    pub fn get_registry(&self) -> Arc<Registry> {
        self.registry.clone()
    }
}