use eyre::Result;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use dashmap::DashMap;
use borsh::{BorshSerialize, BorshDeserialize};
use alloy::primitives::Address;

/// Solana ExEx Compatibility Layer
/// 
/// Enables cross-chain memory operations between Monmouth and Solana,
/// supporting different account models and consensus mechanisms.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaAccount {
    pub pubkey: [u8; 32],
    pub owner: [u8; 32],
    pub lamports: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaMemoryBridge {
    pub monmouth_agent: Address,
    pub solana_pda: [u8; 32],
    pub bridge_state: BridgeState,
    pub pending_transfers: Vec<MemoryTransfer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeState {
    Active,
    Syncing,
    Paused,
    Emergency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTransfer {
    pub transfer_id: [u8; 32],
    pub direction: TransferDirection,
    pub memory_hash: [u8; 32],
    pub size_bytes: u64,
    pub timestamp: u64,
    pub status: TransferStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferDirection {
    MonmouthToSolana,
    SolanaToMonmouth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

#[derive(Debug)]
pub struct SolanaExExAdapter {
    bridges: Arc<DashMap<Address, SolanaMemoryBridge>>,
    solana_client: Arc<MockSolanaClient>,
    memory_compressor: Arc<MemoryCompressor>,
}

#[derive(Debug)]
pub struct MockSolanaClient {
    accounts: DashMap<[u8; 32], SolanaAccount>,
}

#[derive(Debug)]
pub struct MemoryCompressor {
    compression_level: u32,
}

/// Solana Program-Derived Address (PDA) for agent memory
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct SolanaAgentMemoryPDA {
    pub bump: u8,
    pub agent_pubkey: [u8; 32],
    pub memory_slots: Vec<MemorySlot>,
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct MemorySlot {
    pub slot_id: u64,
    pub memory_hash: [u8; 32],
    pub size: u64,
    pub last_update_slot: u64,
}

impl SolanaExExAdapter {
    pub fn new() -> Self {
        Self {
            bridges: Arc::new(DashMap::new()),
            solana_client: Arc::new(MockSolanaClient {
                accounts: DashMap::new(),
            }),
            memory_compressor: Arc::new(MemoryCompressor {
                compression_level: 6,
            }),
        }
    }
    
    /// Create a bridge between Monmouth agent and Solana PDA
    pub async fn create_bridge(
        &self,
        monmouth_agent: Address,
        solana_pubkey: [u8; 32],
    ) -> Result<SolanaMemoryBridge> {
        // Derive PDA for the agent
        let (pda, _bump) = self.derive_agent_pda(&solana_pubkey)?;
        
        let bridge = SolanaMemoryBridge {
            monmouth_agent,
            solana_pda: pda,
            bridge_state: BridgeState::Active,
            pending_transfers: Vec::new(),
        };
        
        self.bridges.insert(monmouth_agent, bridge.clone());
        
        // Initialize Solana account
        self.initialize_solana_account(pda).await?;
        
        Ok(bridge)
    }
    
    /// Transfer memory from Monmouth to Solana
    pub async fn transfer_to_solana(
        &self,
        monmouth_agent: Address,
        memory_data: &[u8],
        memory_hash: [u8; 32],
    ) -> Result<MemoryTransfer> {
        let bridge = self.bridges.get(&monmouth_agent)
            .ok_or_else(|| eyre::eyre!("Bridge not found"))?;
        
        // Compress memory for cross-chain transfer
        let compressed = self.memory_compressor.compress(memory_data)?;
        
        let transfer = MemoryTransfer {
            transfer_id: self.generate_transfer_id(),
            direction: TransferDirection::MonmouthToSolana,
            memory_hash,
            size_bytes: compressed.len() as u64,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            status: TransferStatus::Pending,
        };
        
        // Simulate Solana transaction
        self.execute_solana_transfer(&bridge.solana_pda, &compressed, &memory_hash).await?;
        
        Ok(transfer)
    }
    
    /// Transfer memory from Solana to Monmouth
    pub async fn transfer_from_solana(
        &self,
        solana_pda: [u8; 32],
        target_agent: Address,
    ) -> Result<Vec<u8>> {
        let account = self.solana_client.accounts.get(&solana_pda)
            .ok_or_else(|| eyre::eyre!("Solana account not found"))?;
        
        // Deserialize Solana memory data
        let memory_pda: SolanaAgentMemoryPDA = borsh::from_slice(&account.data)?;
        
        // Get latest memory slot
        let latest_slot = memory_pda.memory_slots
            .iter()
            .max_by_key(|s| s.last_update_slot)
            .ok_or_else(|| eyre::eyre!("No memory slots found"))?;
        
        // Fetch actual memory data (simulated)
        let memory_data = self.fetch_solana_memory(&solana_pda, latest_slot.slot_id).await?;
        
        // Decompress if needed
        let decompressed = self.memory_compressor.decompress(&memory_data)?;
        
        Ok(decompressed)
    }
    
    /// Handle consensus differences between chains
    pub async fn sync_consensus_state(
        &self,
        monmouth_block: u64,
        solana_slot: u64,
    ) -> Result<()> {
        // Map Monmouth blocks to Solana slots (approximately 2 blocks per slot)
        let expected_slot = monmouth_block / 2;
        
        if solana_slot < expected_slot.saturating_sub(100) {
            // Solana is behind, wait for catch up
            return Err(eyre::eyre!("Solana consensus behind Monmouth"));
        }
        
        if solana_slot > expected_slot + 100 {
            // Solana is ahead, might need to wait for Monmouth
            return Err(eyre::eyre!("Monmouth consensus behind Solana"));
        }
        
        Ok(())
    }
    
    /// Convert Monmouth memory types to Solana format
    pub fn convert_memory_format(
        &self,
        monmouth_memory: &crate::memory_exex::Memory,
    ) -> Result<Vec<u8>> {
        #[derive(BorshSerialize)]
        struct SolanaMemory {
            agent_id: [u8; 32],
            memory_type: u8,
            content: Vec<u8>,
            importance: u64,
            access_count: u64,
        }
        
        let mut agent_bytes = [0u8; 32];
        agent_bytes[12..].copy_from_slice(&monmouth_memory.agent_id.as_bytes()[..20]);
        
        let solana_memory = SolanaMemory {
            agent_id: agent_bytes,
            memory_type: match monmouth_memory.memory_type {
                crate::memory_exex::MemoryType::ShortTerm => 0,
                crate::memory_exex::MemoryType::LongTerm => 1,
                crate::memory_exex::MemoryType::Working => 2,
                crate::memory_exex::MemoryType::Episodic => 3,
                crate::memory_exex::MemoryType::Semantic => 4,
            },
            content: monmouth_memory.content.clone(),
            importance: (monmouth_memory.importance * 1000.0) as u64,
            access_count: monmouth_memory.access_count,
        };
        
        Ok(borsh::to_vec(&solana_memory)?)
    }
    
    fn derive_agent_pda(&self, solana_pubkey: &[u8; 32]) -> Result<([u8; 32], u8)> {
        // Simulate PDA derivation
        let seeds = [b"agent_memory", solana_pubkey];
        let mut pda = [0u8; 32];
        pda[..12].copy_from_slice(b"agent_memory");
        pda[12..20].copy_from_slice(&solana_pubkey[..8]);
        Ok((pda, 255))
    }
    
    async fn initialize_solana_account(&self, pda: [u8; 32]) -> Result<()> {
        let account = SolanaAccount {
            pubkey: pda,
            owner: [0; 32], // Memory program ID
            lamports: 1_000_000, // Rent exempt
            data: vec![0; 1024], // Initial capacity
        };
        
        self.solana_client.accounts.insert(pda, account);
        Ok(())
    }
    
    async fn execute_solana_transfer(
        &self,
        pda: &[u8; 32],
        data: &[u8],
        memory_hash: &[u8; 32],
    ) -> Result<()> {
        // Simulate Solana transaction execution
        if let Some(mut account) = self.solana_client.accounts.get_mut(pda) {
            account.data = data.to_vec();
        }
        Ok(())
    }
    
    async fn fetch_solana_memory(&self, pda: &[u8; 32], slot_id: u64) -> Result<Vec<u8>> {
        // Simulate fetching memory from Solana
        if let Some(account) = self.solana_client.accounts.get(pda) {
            Ok(account.data.clone())
        } else {
            Err(eyre::eyre!("Memory not found"))
        }
    }
    
    fn generate_transfer_id(&self) -> [u8; 32] {
        let mut id = [0u8; 32];
        getrandom::getrandom(&mut id).unwrap();
        id
    }
}

impl MemoryCompressor {
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        
        let mut encoder = GzEncoder::new(Vec::new(), Compression::new(self.compression_level));
        encoder.write_all(data)?;
        Ok(encoder.finish()?)
    }
    
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        use flate2::read::GzDecoder;
        use std::io::Read;
        
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_bridge_creation() {
        let adapter = SolanaExExAdapter::new();
        let monmouth_agent = Address::random();
        let solana_pubkey = [1u8; 32];
        
        let bridge = adapter.create_bridge(monmouth_agent, solana_pubkey).await.unwrap();
        
        assert_eq!(bridge.monmouth_agent, monmouth_agent);
        assert!(matches!(bridge.bridge_state, BridgeState::Active));
    }
    
    #[test]
    fn test_memory_compression() {
        let compressor = MemoryCompressor { compression_level: 6 };
        let original = vec![42u8; 1000];
        
        let compressed = compressor.compress(&original).unwrap();
        assert!(compressed.len() < original.len());
        
        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }
}