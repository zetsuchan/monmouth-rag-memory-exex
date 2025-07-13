use alloy::primitives::{Address, B256};
use async_trait::async_trait;
use eyre::{eyre, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// ChromaDB vector store implementation using HTTP API
#[derive(Debug, Clone)]
pub struct ChromaStore {
    /// HTTP client
    client: Client,
    /// ChromaDB base URL
    base_url: String,
    /// Collection name
    collection_name: String,
    /// Embedding dimension
    dimension: usize,
    /// Cache for frequently accessed embeddings
    cache: Arc<RwLock<HashMap<String, Vec<f32>>>>,
}

/// ChromaDB collection metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMetadata {
    pub name: String,
    pub dimension: usize,
    pub metric: String,
}

/// ChromaDB query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub ids: Vec<Vec<String>>,
    pub embeddings: Option<Vec<Vec<Vec<f32>>>>,
    pub metadatas: Option<Vec<Vec<HashMap<String, serde_json::Value>>>>,
    pub documents: Option<Vec<Vec<String>>>,
    pub distances: Option<Vec<Vec<f32>>>,
}

/// ChromaDB add request
#[derive(Debug, Clone, Serialize)]
pub struct AddRequest {
    pub ids: Vec<String>,
    pub embeddings: Vec<Vec<f32>>,
    pub metadatas: Option<Vec<HashMap<String, serde_json::Value>>>,
    pub documents: Option<Vec<String>>,
}

/// ChromaDB collection response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionResponse {
    pub id: String,
    pub name: String,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Vector data with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorData {
    pub embedding: Vec<f32>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub document: Option<String>,
}

impl ChromaStore {
    /// Create new ChromaDB store
    pub async fn new(base_url: Option<String>, collection_name: String) -> Result<Self> {
        let base_url = base_url.unwrap_or_else(|| "http://localhost:8000".to_string());
        let client = Client::new();
        
        let store = Self {
            client,
            base_url,
            collection_name,
            dimension: 384, // Default embedding dimension
            cache: Arc::new(RwLock::new(HashMap::new())),
        };
        
        // Initialize collection
        store.initialize_collection().await?;
        
        Ok(store)
    }
    
    /// Initialize or get collection
    async fn initialize_collection(&self) -> Result<()> {
        // Try to get existing collection
        let collection_url = format!("{}/api/v1/collections/{}", self.base_url, self.collection_name);
        let response = self.client.get(&collection_url).send().await;
        
        match response {
            Ok(resp) if resp.status().is_success() => {
                // Collection exists
                Ok(())
            }
            _ => {
                // Create new collection
                let create_url = format!("{}/api/v1/collections", self.base_url);
                let body = json!({
                    "name": self.collection_name,
                    "metadata": {
                        "dimension": self.dimension,
                        "metric": "cosine"
                    }
                });
                
                self.client
                    .post(&create_url)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| eyre!("Failed to create collection: {}", e))?;
                
                Ok(())
            }
        }
    }
    
    /// Store embedding with metadata
    pub async fn store_embedding(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        // Update cache
        self.cache.write().await.insert(id.to_string(), embedding.clone());
        
        // Prepare request
        let add_request = AddRequest {
            ids: vec![id.to_string()],
            embeddings: vec![embedding],
            metadatas: Some(vec![metadata]),
            documents: None,
        };
        
        // Add to ChromaDB
        let url = format!("{}/api/v1/collections/{}/add", self.base_url, self.collection_name);
        let response = self.client
            .post(&url)
            .json(&add_request)
            .send()
            .await
            .map_err(|e| eyre!("Failed to store embedding: {}", e))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(eyre!("Failed to store embedding: {}", error_text));
        }
        
        Ok(())
    }
    
    /// Store transaction embedding
    pub async fn store_transaction_embedding(
        &self,
        tx_hash: &B256,
        embedding: Vec<f32>,
        sender: &Address,
        block_number: u64,
    ) -> Result<()> {
        let mut metadata = HashMap::new();
        metadata.insert("tx_hash".to_string(), json!(tx_hash.to_string()));
        metadata.insert("sender".to_string(), json!(sender.to_string()));
        metadata.insert("block_number".to_string(), json!(block_number));
        metadata.insert("timestamp".to_string(), json!(chrono::Utc::now().to_rfc3339()));
        metadata.insert("type".to_string(), json!("transaction"));
        
        self.store_embedding(&tx_hash.to_string(), embedding, metadata).await
    }
    
    /// Store document embedding
    pub async fn store_document_embedding(
        &self,
        doc_id: &B256,
        embedding: Vec<f32>,
        doc_type: &str,
        content: Option<&str>,
    ) -> Result<()> {
        let mut metadata = HashMap::new();
        metadata.insert("doc_id".to_string(), json!(doc_id.to_string()));
        metadata.insert("doc_type".to_string(), json!(doc_type));
        metadata.insert("timestamp".to_string(), json!(chrono::Utc::now().to_rfc3339()));
        metadata.insert("type".to_string(), json!("document"));
        
        let add_request = AddRequest {
            ids: vec![doc_id.to_string()],
            embeddings: vec![embedding],
            metadatas: Some(vec![metadata]),
            documents: content.map(|c| vec![c.to_string()]),
        };
        
        let url = format!("{}/api/v1/collections/{}/add", self.base_url, self.collection_name);
        self.client
            .post(&url)
            .json(&add_request)
            .send()
            .await
            .map_err(|e| eyre!("Failed to store document: {}", e))?;
        
        Ok(())
    }
    
    /// Search for similar embeddings
    pub async fn search(
        &self,
        query_embedding: Vec<f32>,
        k: usize,
        filter: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<Vec<(String, f32, HashMap<String, serde_json::Value>)>> {
        let query_body = json!({
            "query_embeddings": vec![query_embedding],
            "n_results": k,
            "where": filter,
            "include": ["metadatas", "distances"]
        });
        
        let url = format!("{}/api/v1/collections/{}/query", self.base_url, self.collection_name);
        let response = self.client
            .post(&url)
            .json(&query_body)
            .send()
            .await
            .map_err(|e| eyre!("Failed to query embeddings: {}", e))?;
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(eyre!("Query failed: {}", error_text));
        }
        
        let result: QueryResult = response.json().await
            .map_err(|e| eyre!("Failed to parse query result: {}", e))?;
        
        // Extract results
        let mut results = Vec::new();
        if let (Some(ids), Some(distances), Some(metadatas)) = 
            (result.ids.first(), result.distances.as_ref().and_then(|d| d.first()), 
             result.metadatas.as_ref().and_then(|m| m.first())) {
            
            for i in 0..ids.len() {
                let id = ids[i].clone();
                let distance = distances.get(i).copied().unwrap_or(f32::MAX);
                let similarity = 1.0 - distance; // Convert distance to similarity
                let metadata = metadatas.get(i).cloned().unwrap_or_default();
                
                results.push((id, similarity, metadata));
            }
        }
        
        Ok(results)
    }
    
    /// Search for similar transactions
    pub async fn search_similar_transactions(
        &self,
        query_embedding: Vec<f32>,
        k: usize,
        sender_filter: Option<Address>,
    ) -> Result<Vec<(B256, f32)>> {
        let mut filter = HashMap::new();
        filter.insert("type".to_string(), json!("transaction"));
        
        if let Some(sender) = sender_filter {
            filter.insert("sender".to_string(), json!(sender.to_string()));
        }
        
        let results = self.search(query_embedding, k, Some(filter)).await?;
        
        Ok(results.into_iter()
            .filter_map(|(id, score, _)| {
                B256::from_str_radix(&id, 16).ok().map(|hash| (hash, score))
            })
            .collect())
    }
    
    /// Get embedding by ID
    pub async fn get_embedding(&self, id: &str) -> Result<Option<Vec<f32>>> {
        // Check cache first
        if let Some(embedding) = self.cache.read().await.get(id) {
            return Ok(Some(embedding.clone()));
        }
        
        // Query ChromaDB
        let query_body = json!({
            "ids": vec![id],
            "include": ["embeddings"]
        });
        
        let url = format!("{}/api/v1/collections/{}/get", self.base_url, self.collection_name);
        let response = self.client
            .post(&url)
            .json(&query_body)
            .send()
            .await
            .map_err(|e| eyre!("Failed to get embedding: {}", e))?;
        
        if !response.status().is_success() {
            return Ok(None);
        }
        
        let result: QueryResult = response.json().await
            .map_err(|e| eyre!("Failed to parse result: {}", e))?;
        
        if let Some(embeddings) = result.embeddings {
            if let Some(first_batch) = embeddings.first() {
                if let Some(embedding) = first_batch.first() {
                    // Update cache
                    self.cache.write().await.insert(id.to_string(), embedding.clone());
                    return Ok(Some(embedding.clone()));
                }
            }
        }
        
        Ok(None)
    }
    
    /// Delete embedding
    pub async fn delete(&self, id: &str) -> Result<()> {
        // Remove from cache
        self.cache.write().await.remove(id);
        
        // Delete from ChromaDB
        let delete_body = json!({
            "ids": vec![id]
        });
        
        let url = format!("{}/api/v1/collections/{}/delete", self.base_url, self.collection_name);
        self.client
            .post(&url)
            .json(&delete_body)
            .send()
            .await
            .map_err(|e| eyre!("Failed to delete embedding: {}", e))?;
        
        Ok(())
    }
    
    /// Remove embedding (alias for compatibility)
    pub async fn remove_embedding(&self, id: &str) -> Result<()> {
        self.delete(id).await
    }
    
    /// Update metadata
    pub async fn update_metadata(
        &self,
        id: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        // ChromaDB doesn't have a direct update API, so we need to get and re-add
        if let Some(embedding) = self.get_embedding(id).await? {
            // Delete old entry
            self.delete(id).await?;
            
            // Add with new metadata
            self.store_embedding(id, embedding, metadata).await?;
        }
        
        Ok(())
    }
    
    /// Count embeddings in collection
    pub async fn count(&self) -> Result<usize> {
        let url = format!("{}/api/v1/collections/{}/count", self.base_url, self.collection_name);
        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| eyre!("Failed to get count: {}", e))?;
        
        if !response.status().is_success() {
            return Ok(0);
        }
        
        let count: usize = response.json().await
            .map_err(|e| eyre!("Failed to parse count: {}", e))?;
        
        Ok(count)
    }
    
    /// Reset collection (delete and recreate)
    pub async fn reset(&self) -> Result<()> {
        // Delete collection
        let delete_url = format!("{}/api/v1/collections/{}", self.base_url, self.collection_name);
        self.client
            .delete(&delete_url)
            .send()
            .await
            .map_err(|e| eyre!("Failed to delete collection: {}", e))?;
        
        // Recreate collection
        self.initialize_collection().await?;
        
        // Clear cache
        self.cache.write().await.clear();
        
        Ok(())
    }
}

/// Trait for vector store operations (for compatibility)
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn store_embedding(&self, id: &str, embedding: Vec<f32>) -> Result<()>;
    async fn search(&self, query_embedding: Vec<f32>, k: usize) -> Result<Vec<(String, f32)>>;
    async fn get_embedding(&self, id: &str) -> Result<Option<Vec<f32>>>;
    async fn delete(&self, id: &str) -> Result<()>;
    async fn count(&self) -> Result<usize>;
}

#[async_trait]
impl VectorStore for ChromaStore {
    async fn store_embedding(&self, id: &str, embedding: Vec<f32>) -> Result<()> {
        let metadata = HashMap::new();
        self.store_embedding(id, embedding, metadata).await
    }
    
    async fn search(&self, query_embedding: Vec<f32>, k: usize) -> Result<Vec<(String, f32)>> {
        let results = self.search(query_embedding, k, None).await?;
        Ok(results.into_iter().map(|(id, score, _)| (id, score)).collect())
    }
    
    async fn get_embedding(&self, id: &str) -> Result<Option<Vec<f32>>> {
        self.get_embedding(id).await
    }
    
    async fn delete(&self, id: &str) -> Result<()> {
        self.delete(id).await
    }
    
    async fn count(&self) -> Result<usize> {
        self.count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_chroma_store_creation() {
        // This test requires ChromaDB to be running
        let store = ChromaStore::new(Some("http://localhost:8000".to_string()), "test_collection".to_string()).await;
        assert!(store.is_ok());
    }
    
    #[tokio::test]
    async fn test_embedding_operations() {
        // Skip if ChromaDB is not running
        let store = match ChromaStore::new(None, "test_embedding_ops".to_string()).await {
            Ok(s) => s,
            Err(_) => return,
        };
        
        // Test embedding
        let embedding = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let id = "test_id";
        
        // Store
        let mut metadata = HashMap::new();
        metadata.insert("test_key".to_string(), json!("test_value"));
        
        assert!(store.store_embedding(id, embedding.clone(), metadata).await.is_ok());
        
        // Retrieve
        let retrieved = store.get_embedding(id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), embedding);
        
        // Search
        let results = store.search(embedding.clone(), 5, None).await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, id);
        
        // Delete
        assert!(store.delete(id).await.is_ok());
        
        // Verify deletion
        let deleted = store.get_embedding(id).await.unwrap();
        assert!(deleted.is_none());
    }
}