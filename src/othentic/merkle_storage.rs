use alloy::primitives::{B256, keccak256};
use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::RagDocument;

/// Merkle tree storage for RAG documents
pub struct MerkleStorage {
    /// The merkle tree
    tree: MerkleTree,
    /// Document storage indexed by leaf hash
    documents: HashMap<B256, RagDocument>,
    /// Leaf position mapping
    leaf_positions: HashMap<B256, usize>,
}

/// Merkle tree implementation
#[derive(Debug, Clone)]
pub struct MerkleTree {
    /// Tree levels (bottom to top)
    levels: Vec<Vec<B256>>,
    /// Tree depth
    depth: usize,
    /// Current number of leaves
    leaf_count: usize,
}

/// Merkle proof for a leaf
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Leaf value
    pub leaf: B256,
    /// Root hash
    pub root: B256,
    /// Sibling hashes from leaf to root
    pub siblings: Vec<B256>,
    /// Path directions (true = right, false = left)
    pub path: Vec<bool>,
    /// Leaf index
    pub index: usize,
}

impl MerkleStorage {
    /// Create new merkle storage
    pub fn new() -> Self {
        Self {
            tree: MerkleTree::new(20), // 20 levels = 1M leaves
            documents: HashMap::new(),
            leaf_positions: HashMap::new(),
        }
    }
    
    /// Add document to storage
    pub fn add_document(&mut self, document: &RagDocument) -> Result<B256> {
        // Calculate document hash
        let doc_hash = self.hash_document(document);
        
        // Add to merkle tree
        let leaf_index = self.tree.add_leaf(doc_hash)?;
        
        // Store document and position
        self.documents.insert(doc_hash, document.clone());
        self.leaf_positions.insert(document.id, leaf_index);
        
        Ok(doc_hash)
    }
    
    /// Get merkle proof for document
    pub fn get_proof(&self, document_id: B256) -> Result<MerkleProof> {
        let leaf_index = self.leaf_positions.get(&document_id)
            .ok_or_else(|| eyre!("Document not found"))?;
        
        let document = self.documents.values()
            .find(|d| d.id == document_id)
            .ok_or_else(|| eyre!("Document data not found"))?;
        
        let leaf_hash = self.hash_document(document);
        
        self.tree.get_proof(*leaf_index, leaf_hash)
    }
    
    /// Verify merkle proof
    pub fn verify_proof(&self, proof: &MerkleProof) -> bool {
        Self::verify_merkle_proof(proof)
    }
    
    /// Get current root
    pub fn get_root(&self) -> B256 {
        self.tree.get_root()
    }
    
    /// Get document by ID
    pub fn get_document(&self, document_id: B256) -> Option<&RagDocument> {
        self.documents.values().find(|d| d.id == document_id)
    }
    
    /// Batch add documents
    pub fn batch_add_documents(&mut self, documents: Vec<RagDocument>) -> Result<Vec<B256>> {
        let mut hashes = Vec::new();
        
        for document in documents {
            let hash = self.add_document(&document)?;
            hashes.push(hash);
        }
        
        Ok(hashes)
    }
    
    /// Get tree statistics
    pub fn get_stats(&self) -> TreeStats {
        TreeStats {
            leaf_count: self.tree.leaf_count,
            tree_depth: self.tree.depth,
            document_count: self.documents.len(),
            root_hash: self.tree.get_root(),
        }
    }
    
    /// Hash document for merkle tree
    fn hash_document(&self, document: &RagDocument) -> B256 {
        let mut data = Vec::new();
        data.extend_from_slice(&document.id.0);
        data.extend_from_slice(&document.content);
        data.extend_from_slice(&(document.timestamp.to_le_bytes()));
        
        keccak256(&data)
    }
    
    /// Static method to verify merkle proof
    pub fn verify_merkle_proof(proof: &MerkleProof) -> bool {
        let mut current = proof.leaf;
        
        for (i, sibling) in proof.siblings.iter().enumerate() {
            current = if proof.path[i] {
                // Current node is on the right
                Self::hash_pair(*sibling, current)
            } else {
                // Current node is on the left
                Self::hash_pair(current, *sibling)
            };
        }
        
        current == proof.root
    }
    
    /// Hash two nodes
    fn hash_pair(left: B256, right: B256) -> B256 {
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(&left.0);
        data.extend_from_slice(&right.0);
        keccak256(&data)
    }
}

impl MerkleTree {
    /// Create new merkle tree
    pub fn new(depth: usize) -> Self {
        let mut levels = Vec::with_capacity(depth + 1);
        
        // Initialize empty tree
        for i in 0..=depth {
            let level_size = 1 << (depth - i);
            levels.push(vec![B256::ZERO; level_size]);
        }
        
        Self {
            levels,
            depth,
            leaf_count: 0,
        }
    }
    
    /// Add leaf to tree
    pub fn add_leaf(&mut self, leaf: B256) -> Result<usize> {
        if self.leaf_count >= (1 << self.depth) {
            return Err(eyre!("Tree is full"));
        }
        
        let leaf_index = self.leaf_count;
        self.levels[0][leaf_index] = leaf;
        self.leaf_count += 1;
        
        // Update tree from leaf to root
        self.update_path(leaf_index);
        
        Ok(leaf_index)
    }
    
    /// Get merkle proof for leaf
    pub fn get_proof(&self, leaf_index: usize, leaf: B256) -> Result<MerkleProof> {
        if leaf_index >= self.leaf_count {
            return Err(eyre!("Leaf index out of bounds"));
        }
        
        if self.levels[0][leaf_index] != leaf {
            return Err(eyre!("Leaf value mismatch"));
        }
        
        let mut siblings = Vec::new();
        let mut path = Vec::new();
        let mut current_index = leaf_index;
        
        // Collect siblings from leaf to root
        for level in 0..self.depth {
            let is_right = current_index % 2 == 1;
            path.push(is_right);
            
            let sibling_index = if is_right {
                current_index - 1
            } else {
                current_index + 1
            };
            
            // Get sibling value (use zero if out of bounds)
            let sibling = if sibling_index < self.levels[level].len() {
                self.levels[level][sibling_index]
            } else {
                B256::ZERO
            };
            
            siblings.push(sibling);
            current_index /= 2;
        }
        
        Ok(MerkleProof {
            leaf,
            root: self.get_root(),
            siblings,
            path,
            index: leaf_index,
        })
    }
    
    /// Get current root
    pub fn get_root(&self) -> B256 {
        self.levels[self.depth][0]
    }
    
    /// Update tree path from leaf to root
    fn update_path(&mut self, mut index: usize) {
        for level in 0..self.depth {
            let parent_index = index / 2;
            let left_index = parent_index * 2;
            let right_index = left_index + 1;
            
            let left = self.levels[level][left_index];
            let right = if right_index < self.levels[level].len() {
                self.levels[level][right_index]
            } else {
                B256::ZERO
            };
            
            self.levels[level + 1][parent_index] = MerkleStorage::hash_pair(left, right);
            index = parent_index;
        }
    }
    
    /// Get tree utilization
    pub fn utilization(&self) -> f32 {
        self.leaf_count as f32 / (1 << self.depth) as f32
    }
}

/// Tree statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeStats {
    pub leaf_count: usize,
    pub tree_depth: usize,
    pub document_count: usize,
    pub root_hash: B256,
}

/// Merkle tree snapshot for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleSnapshot {
    /// Tree levels
    pub levels: Vec<Vec<B256>>,
    /// Leaf count
    pub leaf_count: usize,
    /// Snapshot timestamp
    pub timestamp: u64,
}

impl MerkleTree {
    /// Create snapshot
    pub fn snapshot(&self) -> MerkleSnapshot {
        MerkleSnapshot {
            levels: self.levels.clone(),
            leaf_count: self.leaf_count,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    /// Restore from snapshot
    pub fn from_snapshot(snapshot: MerkleSnapshot, depth: usize) -> Result<Self> {
        if snapshot.levels.len() != depth + 1 {
            return Err(eyre!("Snapshot depth mismatch"));
        }
        
        Ok(Self {
            levels: snapshot.levels,
            depth,
            leaf_count: snapshot.leaf_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_merkle_tree_creation() {
        let tree = MerkleTree::new(10);
        assert_eq!(tree.depth, 10);
        assert_eq!(tree.leaf_count, 0);
        assert_eq!(tree.get_root(), B256::ZERO);
    }
    
    #[test]
    fn test_merkle_proof_verification() {
        let mut tree = MerkleTree::new(3);
        
        let leaf1 = B256::random();
        let leaf2 = B256::random();
        
        tree.add_leaf(leaf1).unwrap();
        tree.add_leaf(leaf2).unwrap();
        
        let proof = tree.get_proof(0, leaf1).unwrap();
        assert!(MerkleStorage::verify_merkle_proof(&proof));
        
        // Test invalid proof
        let mut invalid_proof = proof.clone();
        invalid_proof.root = B256::random();
        assert!(!MerkleStorage::verify_merkle_proof(&invalid_proof));
    }
    
    #[test]
    fn test_document_storage() {
        let mut storage = MerkleStorage::new();
        
        let doc = RagDocument {
            id: B256::random(),
            content: b"test content".to_vec(),
            embedding: vec![0.1, 0.2, 0.3],
            metadata: super::super::DocumentMetadata {
                source: "test".to_string(),
                doc_type: super::super::DocumentType::TransactionContext,
                quality_score: 0.9,
                tags: vec!["test".to_string()],
            },
            timestamp: 12345,
        };
        
        let hash = storage.add_document(&doc).unwrap();
        assert!(storage.get_document(doc.id).is_some());
        
        let proof = storage.get_proof(doc.id).unwrap();
        assert!(storage.verify_proof(&proof));
    }
}