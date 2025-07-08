use eyre::Result;
use chromadb::{ChromaClient, Collection};
use std::collections::HashMap;
use tokio::sync::RwLock;
use dashmap::DashMap;

#[derive(Debug)]
pub struct VectorStore {
    client: ChromaClient,
    collection: RwLock<Option<Collection>>,
    cache: DashMap<String, Vec<f32>>,
}

impl VectorStore {
    pub async fn new() -> Result<Self> {
        let client = ChromaClient::new(ChromaClient::default_host(), ChromaClient::default_port());
        
        Ok(Self {
            client,
            collection: RwLock::new(None),
            cache: DashMap::new(),
        })
    }
    
    pub async fn initialize(&self) -> Result<()> {
        let collection = self.client
            .get_or_create_collection("monmouth_transactions", None)
            .await?;
        
        *self.collection.write().await = Some(collection);
        Ok(())
    }
    
    pub async fn store_embedding(&self, id: &str, embedding: Vec<f32>) -> Result<()> {
        self.cache.insert(id.to_string(), embedding.clone());
        
        if let Some(collection) = self.collection.read().await.as_ref() {
            let mut metadata = HashMap::new();
            metadata.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());
            
            collection.add(
                vec![embedding],
                Some(vec![metadata]),
                None,
                vec![id.to_string()],
            ).await?;
        }
        
        Ok(())
    }
    
    pub async fn query_similar(
        &self,
        query_embedding: Vec<f32>,
        n_results: usize,
    ) -> Result<Vec<(String, f32)>> {
        if let Some(collection) = self.collection.read().await.as_ref() {
            let results = collection.query(
                vec![query_embedding],
                n_results,
                None,
                None,
                None,
            ).await?;
            
            if let (Some(ids), Some(distances)) = (results.ids.first(), results.distances.first()) {
                let similar: Vec<(String, f32)> = ids.iter()
                    .zip(distances.iter())
                    .map(|(id, dist)| (id.clone(), *dist))
                    .collect();
                
                return Ok(similar);
            }
        }
        
        Ok(vec![])
    }
    
    pub async fn remove_embedding(&self, id: &str) -> Result<()> {
        self.cache.remove(id);
        
        if let Some(collection) = self.collection.read().await.as_ref() {
            collection.delete(vec![id.to_string()], None).await?;
        }
        
        Ok(())
    }
    
    pub async fn get_embedding(&self, id: &str) -> Option<Vec<f32>> {
        self.cache.get(id).map(|e| e.clone())
    }
}