pub mod zk_circuits;
pub mod merkle_storage;
pub mod verification_api;
pub mod proof_cache;

use alloy::primitives::{B256, U256};
use async_trait::async_trait;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Othentic integration for RAG verification
pub struct OthenticIntegration {
    /// ZK circuit manager
    circuit_manager: Arc<zk_circuits::CircuitManager>,
    /// Merkle tree storage
    merkle_storage: Arc<RwLock<merkle_storage::MerkleStorage>>,
    /// Verification API
    verification_api: Arc<verification_api::VerificationAPI>,
    /// Proof cache
    proof_cache: Arc<RwLock<proof_cache::ProofCache>>,
}

/// RAG document for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagDocument {
    /// Document ID
    pub id: B256,
    /// Document content
    pub content: Vec<u8>,
    /// Document embedding
    pub embedding: Vec<f32>,
    /// Metadata
    pub metadata: DocumentMetadata,
    /// Timestamp
    pub timestamp: u64,
}

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Source of the document
    pub source: String,
    /// Document type
    pub doc_type: DocumentType,
    /// Quality score
    pub quality_score: f32,
    /// Tags
    pub tags: Vec<String>,
}

/// Types of documents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DocumentType {
    TransactionContext,
    ProtocolDocumentation,
    MarketData,
    UserPreference,
    HistoricalPattern,
}

/// Verification proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationProof {
    /// Proof ID
    pub id: B256,
    /// Document ID being verified
    pub document_id: B256,
    /// ZK proof data
    pub proof_data: ProofData,
    /// Verification result
    pub result: VerificationResult,
    /// Timestamp
    pub timestamp: u64,
}

/// ZK proof data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    /// Public inputs
    pub public_inputs: Vec<U256>,
    /// Proof points
    pub proof_points: Vec<ProofPoint>,
    /// Circuit ID
    pub circuit_id: String,
}

/// Proof point in ZK proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofPoint {
    pub x: U256,
    pub y: U256,
}

/// Verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the verification passed
    pub verified: bool,
    /// Similarity score (0.0 to 1.0)
    pub similarity_score: f32,
    /// Confidence level
    pub confidence: f32,
    /// Additional metadata
    pub metadata: serde_json::Value,
}

impl OthenticIntegration {
    /// Create new Othentic integration
    pub async fn new() -> Result<Self> {
        let circuit_manager = Arc::new(zk_circuits::CircuitManager::new().await?);
        let merkle_storage = Arc::new(RwLock::new(merkle_storage::MerkleStorage::new()));
        let verification_api = Arc::new(verification_api::VerificationAPI::new(
            circuit_manager.clone(),
            merkle_storage.clone(),
        ));
        let proof_cache = Arc::new(RwLock::new(proof_cache::ProofCache::new()));
        
        Ok(Self {
            circuit_manager,
            merkle_storage,
            verification_api,
            proof_cache,
        })
    }
    
    /// Add document to verified storage
    pub async fn add_document(&self, document: RagDocument) -> Result<B256> {
        // Store in merkle tree
        let leaf_hash = self.merkle_storage.write().await.add_document(&document)?;
        
        // Generate initial verification proof
        let proof = self.circuit_manager.generate_document_proof(&document).await?;
        
        // Cache the proof
        self.proof_cache.write().await.store_proof(document.id, proof)?;
        
        Ok(leaf_hash)
    }
    
    /// Verify document similarity
    pub async fn verify_similarity(
        &self,
        doc1_id: B256,
        doc2_id: B256,
        threshold: f32,
    ) -> Result<VerificationProof> {
        // Check cache first
        if let Some(cached_proof) = self.proof_cache.read().await.get_similarity_proof(doc1_id, doc2_id) {
            return Ok(cached_proof);
        }
        
        // Generate new proof
        let proof = self.verification_api.verify_similarity(doc1_id, doc2_id, threshold).await?;
        
        // Cache the result
        self.proof_cache.write().await.store_proof(proof.id, proof.clone())?;
        
        Ok(proof)
    }
    
    /// Get merkle proof for document
    pub async fn get_merkle_proof(&self, document_id: B256) -> Result<merkle_storage::MerkleProof> {
        self.merkle_storage.read().await.get_proof(document_id)
    }
    
    /// Batch verify multiple documents
    pub async fn batch_verify(&self, document_ids: Vec<B256>) -> Result<Vec<VerificationResult>> {
        let mut results = Vec::new();
        
        for doc_id in document_ids {
            let result = self.verification_api.verify_document(doc_id).await?;
            results.push(result);
        }
        
        Ok(results)
    }
}