use eyre::Result;
use std::collections::HashMap;
use tokio::sync::RwLock;
use dashmap::DashMap;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use ordered_float::OrderedFloat;

/// In-memory vector store implementation
/// TODO: Replace with ChromaDB when dependency conflicts are resolved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorData {
    pub embedding: Vec<f32>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug)]
pub struct VectorStore {
    /// In-memory storage for vectors
    vectors: Arc<DashMap<String, VectorData>>,
    /// Cache for frequently accessed embeddings
    cache: DashMap<String, Vec<f32>>,
}

impl VectorStore {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            vectors: Arc::new(DashMap::new()),
            cache: DashMap::new(),
        })
    }
    
    pub async fn initialize(&self) -> Result<()> {
        // No-op for in-memory store
        Ok(())
    }
    
    pub async fn store_embedding(&self, id: &str, embedding: Vec<f32>) -> Result<()> {
        self.cache.insert(id.to_string(), embedding.clone());
        
        let mut metadata = HashMap::new();
        metadata.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());
        
        let vector_data = VectorData {
            embedding,
            metadata,
        };
        
        self.vectors.insert(id.to_string(), vector_data);
        
        Ok(())
    }
    
    pub async fn search(&self, query_embedding: Vec<f32>, k: usize) -> Result<Vec<(String, f32)>> {
        let mut results = Vec::new();
        
        // Simple cosine similarity search
        for entry in self.vectors.iter() {
            let id = entry.key().clone();
            let similarity = self.cosine_similarity(&query_embedding, &entry.value().embedding);
            results.push((id, similarity));
        }
        
        // Sort by similarity (descending)
        results.sort_by_key(|(_, sim)| OrderedFloat(-*sim));
        results.truncate(k);
        
        Ok(results)
    }
    
    pub async fn get_embedding(&self, id: &str) -> Result<Option<Vec<f32>>> {
        // Check cache first
        if let Some(embedding) = self.cache.get(id) {
            return Ok(Some(embedding.clone()));
        }
        
        // Check main storage
        if let Some(vector_data) = self.vectors.get(id) {
            let embedding = vector_data.embedding.clone();
            // Update cache
            self.cache.insert(id.to_string(), embedding.clone());
            return Ok(Some(embedding));
        }
        
        Ok(None)
    }
    
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.cache.remove(id);
        self.vectors.remove(id);
        Ok(())
    }
    
    pub async fn update_metadata(&self, id: &str, metadata: HashMap<String, String>) -> Result<()> {
        if let Some(mut vector_data) = self.vectors.get_mut(id) {
            vector_data.metadata = metadata;
        }
        Ok(())
    }
    
    pub async fn list_ids(&self) -> Result<Vec<String>> {
        Ok(self.vectors.iter().map(|entry| entry.key().clone()).collect())
    }
    
    pub async fn count(&self) -> Result<usize> {
        Ok(self.vectors.len())
    }
    
    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if magnitude_a == 0.0 || magnitude_b == 0.0 {
            0.0
        } else {
            dot_product / (magnitude_a * magnitude_b)
        }
    }
}