use eyre::Result;
use reth_primitives::{Address, H256, U256, StorageKey, StorageValue};
use reth_provider::{StateProvider, StateProviderFactory};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use dashmap::DashMap;

/// On-chain memory root tracking
/// 
/// Tracks memory roots in storage slot 0x10 as per the architecture notes.
/// This creates a verifiable link between on-chain state and off-chain memory.

/// Storage slot 0x10 for memory root pointer
pub const MEMORY_ROOT_SLOT: StorageKey = StorageKey::new([
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
]);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRootInfo {
    pub agent: Address,
    pub memory_root: H256,
    pub block_number: u64,
    pub timestamp: u64,
    pub verification_proof: Option<MemoryProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProof {
    pub merkle_root: H256,
    pub intent_hashes: Vec<H256>,
    pub total_size: u64,
    pub signature: Vec<u8>,
}

#[derive(Debug)]
pub struct MemoryRootTracker<Provider> {
    provider: Arc<Provider>,
    cached_roots: DashMap<Address, MemoryRootInfo>,
    verification_enabled: bool,
}

impl<Provider> MemoryRootTracker<Provider>
where
    Provider: StateProviderFactory + Send + Sync + 'static,
{
    pub fn new(provider: Arc<Provider>, verification_enabled: bool) -> Self {
        Self {
            provider,
            cached_roots: DashMap::new(),
            verification_enabled,
        }
    }
    
    /// Read memory root from on-chain storage slot 0x10
    pub async fn get_onchain_memory_root(
        &self,
        agent: Address,
        block_number: Option<u64>,
    ) -> Result<Option<H256>> {
        let state_provider = if let Some(block) = block_number {
            self.provider.state_by_block_number_or_tag(block.into())?
        } else {
            self.provider.latest()?
        };
        
        let storage_value = state_provider.storage(agent, MEMORY_ROOT_SLOT)?;
        
        if let Some(value) = storage_value {
            if value != StorageValue::ZERO {
                let memory_root = H256::from(value.to_be_bytes::<32>());
                
                // Update cache
                self.cached_roots.insert(agent, MemoryRootInfo {
                    agent,
                    memory_root,
                    block_number: block_number.unwrap_or(0),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                    verification_proof: None,
                });
                
                Ok(Some(memory_root))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    
    /// Verify that off-chain memory root matches on-chain storage
    pub async fn verify_memory_root(
        &self,
        agent: Address,
        offchain_root: H256,
        block_number: Option<u64>,
    ) -> Result<bool> {
        if !self.verification_enabled {
            return Ok(true);
        }
        
        let onchain_root = self.get_onchain_memory_root(agent, block_number).await?;
        
        match onchain_root {
            Some(root) => Ok(root == offchain_root),
            None => Ok(offchain_root == H256::zero()),
        }
    }
    
    /// Generate proof for current memory state
    pub fn generate_memory_proof(
        &self,
        agent: Address,
        intent_hashes: Vec<H256>,
        total_size: u64,
    ) -> Result<MemoryProof> {
        // Calculate merkle root from intent hashes
        let merkle_root = self.calculate_merkle_root(&intent_hashes);
        
        // Generate signature (placeholder - would use BLS in production)
        let signature = self.sign_memory_proof(&agent, &merkle_root)?;
        
        Ok(MemoryProof {
            merkle_root,
            intent_hashes,
            total_size,
            signature,
        })
    }
    
    /// Verify memory proof
    pub fn verify_memory_proof(
        &self,
        agent: Address,
        proof: &MemoryProof,
    ) -> Result<bool> {
        // Verify merkle root calculation
        let calculated_root = self.calculate_merkle_root(&proof.intent_hashes);
        if calculated_root != proof.merkle_root {
            return Ok(false);
        }
        
        // Verify signature (placeholder)
        self.verify_signature(&agent, &proof.merkle_root, &proof.signature)
    }
    
    /// Get cached memory root info
    pub fn get_cached_root(&self, agent: Address) -> Option<MemoryRootInfo> {
        self.cached_roots.get(&agent).map(|r| r.clone())
    }
    
    /// Clear cache for an agent
    pub fn clear_cache(&self, agent: Address) {
        self.cached_roots.remove(&agent);
    }
    
    /// Get all agents with memory roots
    pub async fn get_all_agents_with_memory(&self) -> Result<Vec<Address>> {
        // In production, this would query events or use a more efficient method
        // For now, return cached agents
        Ok(self.cached_roots.iter().map(|e| *e.key()).collect())
    }
    
    fn calculate_merkle_root(&self, hashes: &[H256]) -> H256 {
        if hashes.is_empty() {
            return H256::zero();
        }
        
        let mut current_level = hashes.to_vec();
        
        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            
            for chunk in current_level.chunks(2) {
                let left = chunk[0];
                let right = if chunk.len() > 1 { chunk[1] } else { chunk[0] };
                
                let mut combined = [0u8; 64];
                combined[..32].copy_from_slice(&left.0);
                combined[32..].copy_from_slice(&right.0);
                
                let hash = keccak256(&combined);
                next_level.push(H256::from_slice(&hash));
            }
            
            current_level = next_level;
        }
        
        current_level[0]
    }
    
    fn sign_memory_proof(&self, agent: &Address, merkle_root: &H256) -> Result<Vec<u8>> {
        // Placeholder - would use proper BLS signing in production
        let mut signature = vec![0u8; 65];
        signature[..20].copy_from_slice(&agent.0);
        signature[20..52].copy_from_slice(&merkle_root.0);
        Ok(signature)
    }
    
    fn verify_signature(&self, agent: &Address, merkle_root: &H256, signature: &[u8]) -> Result<bool> {
        // Placeholder verification
        if signature.len() != 65 {
            return Ok(false);
        }
        
        Ok(&signature[..20] == &agent.0 && &signature[20..52] == &merkle_root.0)
    }
}

/// Helper function for Keccak256 hashing
fn keccak256(data: &[u8]) -> [u8; 32] {
    use sha3::{Keccak256, Digest};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}

/// Contract interface for agents to update their memory root
#[derive(Debug, Clone)]
pub struct MemoryRootContract {
    pub address: Address,
}

impl MemoryRootContract {
    /// Generate calldata for setMemoryRoot(bytes32)
    pub fn encode_set_memory_root(&self, memory_root: H256) -> Vec<u8> {
        let mut calldata = vec![
            // setMemoryRoot(bytes32) selector
            0x3e, 0x4f, 0x49, 0xe6,
        ];
        calldata.extend_from_slice(&memory_root.0);
        calldata
    }
    
    /// Generate calldata for getMemoryRoot()
    pub fn encode_get_memory_root(&self) -> Vec<u8> {
        vec![
            // getMemoryRoot() selector
            0x95, 0x7b, 0xb2, 0xef,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_merkle_root_calculation() {
        let tracker = MemoryRootTracker::<()> {
            provider: Arc::new(()),
            cached_roots: DashMap::new(),
            verification_enabled: true,
        };
        
        let hashes = vec![
            H256::from_low_u64_be(1),
            H256::from_low_u64_be(2),
            H256::from_low_u64_be(3),
            H256::from_low_u64_be(4),
        ];
        
        let root1 = tracker.calculate_merkle_root(&hashes);
        let root2 = tracker.calculate_merkle_root(&hashes);
        
        // Should be deterministic
        assert_eq!(root1, root2);
        
        // Different order should give different root
        let mut hashes_reversed = hashes.clone();
        hashes_reversed.reverse();
        let root3 = tracker.calculate_merkle_root(&hashes_reversed);
        assert_ne!(root1, root3);
    }
    
    #[test]
    fn test_memory_proof_generation() {
        let tracker = MemoryRootTracker::<()> {
            provider: Arc::new(()),
            cached_roots: DashMap::new(),
            verification_enabled: true,
        };
        
        let agent = Address::random();
        let intents = vec![H256::random(), H256::random()];
        let total_size = 1024;
        
        let proof = tracker.generate_memory_proof(agent, intents.clone(), total_size).unwrap();
        
        assert_eq!(proof.intent_hashes, intents);
        assert_eq!(proof.total_size, total_size);
        assert!(!proof.signature.is_empty());
        
        // Verify the proof
        assert!(tracker.verify_memory_proof(agent, &proof).unwrap());
    }
    
    #[test]
    fn test_contract_encoding() {
        let contract = MemoryRootContract {
            address: Address::random(),
        };
        
        let memory_root = H256::random();
        let calldata = contract.encode_set_memory_root(memory_root);
        
        assert_eq!(calldata.len(), 36); // 4 bytes selector + 32 bytes root
        assert_eq!(&calldata[0..4], &[0x3e, 0x4f, 0x49, 0xe6]);
        assert_eq!(&calldata[4..36], &memory_root.0);
    }
}