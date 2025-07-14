//! Configuration module for RAG-Memory ExEx
//! 
//! Provides configuration structures compatible with SVM ExEx expectations

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Main ExEx configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExExConfiguration {
    /// Individual ExEx configuration
    pub individual: IndividualExExConfig,
    /// Shared configuration across ExEx instances
    pub shared: SharedExExConfig,
    /// Performance configuration
    pub performance: PerformanceConfig,
}

/// Individual ExEx configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndividualExExConfig {
    /// Unique instance ID
    pub instance_id: String,
    /// ExEx type
    pub exex_type: ExExType,
    /// RAG-specific configuration
    pub rag_config: Option<RagConfig>,
    /// Memory-specific configuration
    pub memory_config: Option<MemoryConfig>,
    /// Network configuration
    pub network: NetworkConfig,
    /// Resource limits
    pub resources: ResourceConfig,
}

/// Shared configuration across ExEx instances
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedExExConfig {
    /// Chain ID
    pub chain_id: u64,
    /// Consensus parameters
    pub consensus: ConsensusConfig,
    /// Inter-ExEx communication settings
    pub communication: CommunicationConfig,
    /// Monitoring configuration
    pub monitoring: MonitoringConfig,
}

/// ExEx type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExExType {
    /// SVM transaction processor
    SVM,
    /// RAG context provider
    RAG,
    /// Hybrid node with multiple capabilities
    Hybrid,
}

/// RAG-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagConfig {
    /// Vector store settings
    pub vector_store: VectorStoreConfig,
    /// Embedding model settings
    pub embedding: EmbeddingConfig,
    /// Context retrieval settings
    pub retrieval: RetrievalSettings,
    /// Knowledge graph configuration
    pub knowledge_graph: KnowledgeGraphConfig,
}

/// Memory-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Database path
    pub db_path: PathBuf,
    /// Maximum database size in bytes
    pub max_db_size: u64,
    /// Cache configuration
    pub cache: CacheConfig,
    /// Checkpoint settings
    pub checkpoint: CheckpointConfig,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Bind address for inter-ExEx communication
    pub bind_address: String,
    /// Discovery method
    pub discovery: DiscoveryConfig,
    /// Maximum connections
    pub max_connections: usize,
    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,
}

/// Resource configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Maximum memory usage in bytes
    pub max_memory: u64,
    /// CPU cores allocated
    pub cpu_cores: usize,
    /// Maximum concurrent operations
    pub max_concurrent_ops: usize,
    /// Operation timeout in seconds
    pub operation_timeout_secs: u64,
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// BLS threshold
    pub bls_threshold: f64,
    /// Minimum validators
    pub min_validators: usize,
    /// Consensus timeout in milliseconds
    pub consensus_timeout_ms: u64,
}

/// Communication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationConfig {
    /// Message bus settings
    pub message_bus: MessageBusSettings,
    /// Protocol version
    pub protocol_version: u8,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,
}

/// Monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Metrics endpoint
    pub metrics_endpoint: String,
    /// Dashboard port
    pub dashboard_port: u16,
    /// Enable detailed logging
    pub detailed_logging: bool,
    /// Alert thresholds
    pub alert_thresholds: HashMap<String, f64>,
}

/// Vector store configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStoreConfig {
    /// Storage backend type
    pub backend: VectorStoreBackend,
    /// Index settings
    pub index_settings: HashMap<String, serde_json::Value>,
}

/// Vector store backend options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VectorStoreBackend {
    /// In-memory store
    InMemory,
    /// ChromaDB
    ChromaDB { host: String, port: u16 },
    /// Pinecone
    Pinecone { api_key: String, environment: String },
}

/// Embedding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Model name
    pub model: String,
    /// Embedding dimensions
    pub dimensions: usize,
    /// Batch size
    pub batch_size: usize,
    /// Use GPU acceleration
    pub use_gpu: bool,
}

/// Retrieval settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSettings {
    /// Maximum results to return
    pub max_results: usize,
    /// Similarity threshold
    pub similarity_threshold: f32,
    /// Include metadata in results
    pub include_metadata: bool,
}

/// Knowledge graph configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraphConfig {
    /// Enable graph building
    pub enabled: bool,
    /// Maximum nodes
    pub max_nodes: usize,
    /// Pruning interval in seconds
    pub pruning_interval_secs: u64,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// LRU cache size
    pub lru_size: usize,
    /// TTL in seconds
    pub ttl_secs: u64,
    /// Enable compression
    pub enable_compression: bool,
}

/// Checkpoint configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Checkpoint interval in blocks
    pub interval_blocks: u64,
    /// Maximum checkpoints to retain
    pub max_checkpoints: usize,
    /// Checkpoint compression
    pub compression: CompressionType,
}

/// Compression type options
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompressionType {
    None,
    Zstd,
    Lz4,
    Snappy,
}

/// Discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Discovery method
    pub method: DiscoveryMethod,
    /// Bootstrap nodes
    pub bootstrap_nodes: Vec<String>,
}

/// Discovery method options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryMethod {
    /// Multicast discovery
    Multicast,
    /// Static peer list
    Static,
    /// DNS-based discovery
    DNS { domain: String },
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Maximum concurrent block processing
    pub max_concurrent_processing: usize,
    /// Enable batch processing
    pub enable_batching: bool,
    /// Batch size for processing
    pub batch_size: usize,
    /// Batch timeout
    pub batch_timeout: Duration,
    /// Enable caching
    pub enable_caching: bool,
    /// Cache size in MB
    pub cache_size_mb: usize,
}

/// Message bus settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBusSettings {
    /// Channel buffer size
    pub channel_buffer_size: usize,
    /// Maximum message size in bytes
    pub max_message_size: usize,
}

impl Default for ExExConfiguration {
    fn default() -> Self {
        Self {
            individual: IndividualExExConfig::default(),
            shared: SharedExExConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

impl Default for IndividualExExConfig {
    fn default() -> Self {
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            exex_type: ExExType::RAG,
            rag_config: Some(RagConfig::default()),
            memory_config: Some(MemoryConfig::default()),
            network: NetworkConfig::default(),
            resources: ResourceConfig::default(),
        }
    }
}

impl Default for SharedExExConfig {
    fn default() -> Self {
        Self {
            chain_id: 1,
            consensus: ConsensusConfig::default(),
            communication: CommunicationConfig::default(),
            monitoring: MonitoringConfig::default(),
        }
    }
}

impl Default for RagConfig {
    fn default() -> Self {
        Self {
            vector_store: VectorStoreConfig::default(),
            embedding: EmbeddingConfig::default(),
            retrieval: RetrievalSettings::default(),
            knowledge_graph: KnowledgeGraphConfig::default(),
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("./memory_store"),
            max_db_size: 10 * 1024 * 1024 * 1024, // 10GB
            cache: CacheConfig::default(),
            checkpoint: CheckpointConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:0".to_string(),
            discovery: DiscoveryConfig::default(),
            max_connections: 100,
            connection_timeout_secs: 30,
        }
    }
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            max_memory: 8 * 1024 * 1024 * 1024, // 8GB
            cpu_cores: 4,
            max_concurrent_ops: 1000,
            operation_timeout_secs: 60,
        }
    }
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            bls_threshold: 0.67,
            min_validators: 3,
            consensus_timeout_ms: 5000,
        }
    }
}

impl Default for CommunicationConfig {
    fn default() -> Self {
        Self {
            message_bus: MessageBusSettings::default(),
            protocol_version: 1,
            heartbeat_interval_secs: 10,
        }
    }
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            metrics_endpoint: "/metrics".to_string(),
            dashboard_port: 3030,
            detailed_logging: false,
            alert_thresholds: HashMap::new(),
        }
    }
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            backend: VectorStoreBackend::InMemory,
            index_settings: HashMap::new(),
        }
    }
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "all-MiniLM-L6-v2".to_string(),
            dimensions: 384,
            batch_size: 32,
            use_gpu: false,
        }
    }
}

impl Default for RetrievalSettings {
    fn default() -> Self {
        Self {
            max_results: 10,
            similarity_threshold: 0.7,
            include_metadata: true,
        }
    }
}

impl Default for KnowledgeGraphConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_nodes: 10000,
            pruning_interval_secs: 3600,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            lru_size: 1000,
            ttl_secs: 3600,
            enable_compression: true,
        }
    }
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            interval_blocks: 1000,
            max_checkpoints: 10,
            compression: CompressionType::Zstd,
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            method: DiscoveryMethod::Multicast,
            bootstrap_nodes: vec![],
        }
    }
}

impl Default for MessageBusSettings {
    fn default() -> Self {
        Self {
            channel_buffer_size: 1000,
            max_message_size: 1024 * 1024, // 1MB
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            max_concurrent_processing: 10,
            enable_batching: true,
            batch_size: 100,
            batch_timeout: Duration::from_millis(100),
            enable_caching: true,
            cache_size_mb: 256,
        }
    }
}