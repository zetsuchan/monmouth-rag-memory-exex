use eyre::Result;
use candle_core::{Device, Tensor};
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{Tokenizer, PaddingParams};
use reth_primitives::TransactionSigned;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct EmbeddingPipeline {
    model: Arc<RwLock<BertModel>>,
    tokenizer: Arc<Tokenizer>,
    device: Device,
}

impl EmbeddingPipeline {
    pub fn new() -> Result<Self> {
        let device = Device::cuda_if_available(0)?;
        
        let tokenizer = Tokenizer::from_pretrained("sentence-transformers/all-MiniLM-L6-v2", None)?;
        
        let config = Config::default();
        let model = BertModel::new(&config, device.clone())?;
        
        Ok(Self {
            model: Arc::new(RwLock::new(model)),
            tokenizer: Arc::new(tokenizer),
            device,
        })
    }
    
    pub async fn generate_embedding(&self, tx: &TransactionSigned) -> Result<Vec<f32>> {
        let text = self.transaction_to_text(tx);
        
        let encoding = self.tokenizer.encode(text, true)?;
        let tokens = encoding.get_ids();
        
        let input_ids = Tensor::new(tokens, &self.device)?;
        let attention_mask = Tensor::ones_like(&input_ids)?;
        
        let model = self.model.read().await;
        let outputs = model.forward(&input_ids, &attention_mask)?;
        
        let pooled = self.mean_pooling(&outputs, &attention_mask)?;
        
        let embedding: Vec<f32> = pooled.to_vec1()?;
        let normalized = self.normalize_embedding(embedding);
        
        Ok(normalized)
    }
    
    pub async fn generate_text_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self.tokenizer.encode(text, true)?;
        let tokens = encoding.get_ids();
        
        let input_ids = Tensor::new(tokens, &self.device)?;
        let attention_mask = Tensor::ones_like(&input_ids)?;
        
        let model = self.model.read().await;
        let outputs = model.forward(&input_ids, &attention_mask)?;
        
        let pooled = self.mean_pooling(&outputs, &attention_mask)?;
        
        let embedding: Vec<f32> = pooled.to_vec1()?;
        let normalized = self.normalize_embedding(embedding);
        
        Ok(normalized)
    }
    
    fn transaction_to_text(&self, tx: &TransactionSigned) -> String {
        let mut parts = vec![
            format!("Transaction hash: {}", tx.hash()),
            format!("Nonce: {}", tx.nonce()),
            format!("Gas limit: {}", tx.gas_limit()),
        ];
        
        if let Some(to) = tx.to() {
            parts.push(format!("To: {}", to));
        }
        
        if tx.value() > reth_primitives::U256::ZERO {
            parts.push(format!("Value: {}", tx.value()));
        }
        
        if !tx.input().is_empty() {
            let input_preview = if tx.input().len() > 64 {
                format!("{}...", hex::encode(&tx.input()[..64]))
            } else {
                hex::encode(tx.input())
            };
            parts.push(format!("Input: {}", input_preview));
        }
        
        parts.join(" ")
    }
    
    fn mean_pooling(&self, outputs: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
        let expanded_mask = attention_mask.unsqueeze(2)?.expand_as(outputs)?;
        let sum_embeddings = (outputs * &expanded_mask)?.sum(1)?;
        let sum_mask = expanded_mask.sum(1)?;
        
        Ok(&sum_embeddings / &sum_mask)
    }
    
    fn normalize_embedding(&self, mut embedding: Vec<f32>) -> Vec<f32> {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut embedding {
                *x /= norm;
            }
        }
        embedding
    }
}