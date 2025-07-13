use eyre::Result;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use serde_json::json;

use super::chroma_store::ChromaStore;

/// Vector store implementation using ChromaDB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorData {
    pub embedding: Vec<f32>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct VectorStore {
    /// ChromaDB backend
    chroma: ChromaStore,
}

impl VectorStore {
    pub async fn new() -> Result<Self> {
        // Use RAG collection in ChromaDB
        let chroma = ChromaStore::new(None, "rag_embeddings".to_string()).await?;
        Ok(Self { chroma })
    }
    
    pub async fn initialize(&self) -> Result<()> {
        // ChromaDB initialization is handled in new()
        Ok(())
    }
    
    pub async fn store_embedding(&self, id: &str, embedding: Vec<f32>) -> Result<()> {
        let mut metadata = HashMap::new();
        metadata.insert("timestamp".to_string(), json!(chrono::Utc::now().to_rfc3339()));
        metadata.insert("embedding_size".to_string(), json!(embedding.len()));
        
        self.chroma.store_embedding(id, embedding, metadata).await
    }
    
    pub async fn search(&self, query_embedding: Vec<f32>, k: usize) -> Result<Vec<(String, f32)>> {
        let results = self.chroma.search(query_embedding, k, None).await?;
        Ok(results.into_iter().map(|(id, score, _)| (id, score)).collect())
    }
    
    pub async fn get_embedding(&self, id: &str) -> Result<Option<Vec<f32>>> {
        self.chroma.get_embedding(id).await
    }
    
    pub async fn delete(&self, id: &str) -> Result<()> {
        self.chroma.delete(id).await
    }
    
    pub async fn remove_embedding(&self, id: &str) -> Result<()> {
        self.chroma.remove_embedding(id).await
    }
    
    pub async fn update_metadata(&self, id: &str, metadata: HashMap<String, String>) -> Result<()> {
        let json_metadata: HashMap<String, serde_json::Value> = metadata
            .into_iter()
            .map(|(k, v)| (k, json!(v)))
            .collect();
        
        self.chroma.update_metadata(id, json_metadata).await
    }
    
    pub async fn list_ids(&self) -> Result<Vec<String>> {
        // ChromaDB doesn't have a direct list API, so we'll search with a dummy embedding
        // and high k value to get all IDs
        let dummy_embedding = vec![0.0; 384]; // Assuming 384 dimensions
        let results = self.chroma.search(dummy_embedding, 10000, None).await?;
        Ok(results.into_iter().map(|(id, _, _)| id).collect())
    }
    
    pub async fn count(&self) -> Result<usize> {
        self.chroma.count().await
    }
}