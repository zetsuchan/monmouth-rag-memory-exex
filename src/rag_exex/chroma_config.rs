use serde::{Deserialize, Serialize};
use std::env;

/// ChromaDB configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChromaConfig {
    /// ChromaDB server URL
    pub url: String,
    /// Collection name for RAG embeddings
    pub collection_name: String,
    /// Embedding dimension
    pub embedding_dimension: usize,
    /// Distance metric (cosine, l2, ip)
    pub distance_metric: String,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
    /// Maximum retries for failed requests
    pub max_retries: u32,
    /// Cache size for embeddings
    pub cache_size: usize,
}

impl Default for ChromaConfig {
    fn default() -> Self {
        Self {
            url: env::var("CHROMA_URL").unwrap_or_else(|_| "http://localhost:8000".to_string()),
            collection_name: env::var("CHROMA_COLLECTION").unwrap_or_else(|_| "rag_embeddings".to_string()),
            embedding_dimension: 384,
            distance_metric: "cosine".to_string(),
            timeout_secs: 30,
            max_retries: 3,
            cache_size: 10000,
        }
    }
}

impl ChromaConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();
        
        if let Ok(dim) = env::var("EMBEDDING_DIMENSION") {
            if let Ok(d) = dim.parse() {
                config.embedding_dimension = d;
            }
        }
        
        if let Ok(metric) = env::var("DISTANCE_METRIC") {
            config.distance_metric = metric;
        }
        
        if let Ok(timeout) = env::var("CHROMA_TIMEOUT") {
            if let Ok(t) = timeout.parse() {
                config.timeout_secs = t;
            }
        }
        
        if let Ok(retries) = env::var("CHROMA_MAX_RETRIES") {
            if let Ok(r) = retries.parse() {
                config.max_retries = r;
            }
        }
        
        if let Ok(cache) = env::var("EMBEDDING_CACHE_SIZE") {
            if let Ok(c) = cache.parse() {
                config.cache_size = c;
            }
        }
        
        config
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.embedding_dimension == 0 {
            return Err("Embedding dimension must be greater than 0".to_string());
        }
        
        if !["cosine", "l2", "ip"].contains(&self.distance_metric.as_str()) {
            return Err("Invalid distance metric. Must be one of: cosine, l2, ip".to_string());
        }
        
        if self.timeout_secs == 0 {
            return Err("Timeout must be greater than 0".to_string());
        }
        
        Ok(())
    }
}

/// Environment variables for ChromaDB configuration
pub const CHROMA_ENV_VARS: &[(&str, &str)] = &[
    ("CHROMA_URL", "ChromaDB server URL (default: http://localhost:8000)"),
    ("CHROMA_COLLECTION", "Collection name for RAG embeddings (default: rag_embeddings)"),
    ("EMBEDDING_DIMENSION", "Dimension of embeddings (default: 384)"),
    ("DISTANCE_METRIC", "Distance metric: cosine, l2, or ip (default: cosine)"),
    ("CHROMA_TIMEOUT", "Connection timeout in seconds (default: 30)"),
    ("CHROMA_MAX_RETRIES", "Maximum retries for failed requests (default: 3)"),
    ("EMBEDDING_CACHE_SIZE", "Size of embedding cache (default: 10000)"),
];

/// Print ChromaDB configuration help
pub fn print_config_help() {
    println!("ChromaDB Configuration Environment Variables:");
    println!("===========================================");
    for (var, desc) in CHROMA_ENV_VARS {
        println!("  {}: {}", var, desc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = ChromaConfig::default();
        assert_eq!(config.url, "http://localhost:8000");
        assert_eq!(config.collection_name, "rag_embeddings");
        assert_eq!(config.embedding_dimension, 384);
        assert_eq!(config.distance_metric, "cosine");
    }
    
    #[test]
    fn test_config_validation() {
        let mut config = ChromaConfig::default();
        assert!(config.validate().is_ok());
        
        config.embedding_dimension = 0;
        assert!(config.validate().is_err());
        
        config.embedding_dimension = 384;
        config.distance_metric = "invalid".to_string();
        assert!(config.validate().is_err());
    }
}