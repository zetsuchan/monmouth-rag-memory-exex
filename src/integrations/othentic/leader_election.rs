//! Leader Election Implementation for Othentic
//! 
//! Implements leader election mechanisms for determining task performers
//! with support for multiple election algorithms.

use crate::integrations::othentic::{
    LeaderElection, ElectionAlgorithm, OperatorInfo,
};
use serde::{Serialize, Deserialize};
use alloy_primitives::Address;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderRecord {
    pub leader: Address,
    pub epoch: u64,
    pub stake: u64,
    pub timestamp: u64,
}
use eyre::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sha3::{Digest, Keccak256};

impl LeaderElection {
    pub fn new(algorithm: ElectionAlgorithm) -> Self {
        Self {
            current_epoch: Arc::new(RwLock::new(0)),
            leader_history: Arc::new(RwLock::new(Vec::new())),
            algorithm,
            operators: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Register an operator
    pub async fn register_operator(&self, info: OperatorInfo) -> Result<()> {
        let mut operators = self.operators.write().await;
        operators.insert(info.address, info.clone());
        
        tracing::info!(
            "Registered operator {:?} with stake {} and performance score {}",
            hex::encode(info.address),
            info.stake,
            info.performance_score
        );
        
        Ok(())
    }
    
    /// Update operator info
    pub async fn update_operator(&self, address: [u8; 20], info: OperatorInfo) -> Result<()> {
        let mut operators = self.operators.write().await;
        operators.insert(address, info);
        Ok(())
    }
    
    /// Trigger new leader election
    pub async fn elect_leader(&self) -> Result<LeaderRecord> {
        let epoch = {
            let mut current = self.current_epoch.write().await;
            *current += 1;
            *current
        };
        
        let operators = self.operators.read().await;
        let active_operators: Vec<_> = operators
            .values()
            .filter(|op| op.is_active)
            .cloned()
            .collect();
        
        if active_operators.is_empty() {
            return Err(eyre::eyre!("No active operators available"));
        }
        
        let (leader, backup_leaders, proof) = match &self.algorithm {
            ElectionAlgorithm::ProofOfStake { min_stake } => {
                self.pos_election(&active_operators, *min_stake, epoch).await?
            }
            ElectionAlgorithm::PerformanceBased { window_size } => {
                self.performance_election(&active_operators, *window_size, epoch).await?
            }
            ElectionAlgorithm::RoundRobin => {
                self.round_robin_election(&active_operators, epoch).await?
            }
            ElectionAlgorithm::VRF { seed } => {
                self.vrf_election(&active_operators, *seed, epoch).await?
            }
        };
        
        let record = LeaderRecord {
            epoch,
            leader,
            backup_leaders,
            election_proof: proof,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        // Store in history
        let mut history = self.leader_history.write().await;
        history.push(record.clone());
        
        tracing::info!(
            "Elected leader {:?} for epoch {} with {} backups",
            hex::encode(leader),
            epoch,
            record.backup_leaders.len()
        );
        
        Ok(record)
    }
    
    /// Check if an operator is the current leader
    pub async fn is_leader(&self, operator: [u8; 20]) -> Result<bool> {
        let history = self.leader_history.read().await;
        
        if let Some(record) = history.last() {
            Ok(record.leader == operator)
        } else {
            Ok(false)
        }
    }
    
    /// Get current leader
    pub async fn get_current_leader(&self) -> Result<Option<[u8; 20]>> {
        let history = self.leader_history.read().await;
        Ok(history.last().map(|r| r.leader))
    }
    
    /// Get backup leaders
    pub async fn get_backup_leaders(&self) -> Result<Vec<[u8; 20]>> {
        let history = self.leader_history.read().await;
        
        if let Some(record) = history.last() {
            Ok(record.backup_leaders.clone())
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Proof of Stake election
    async fn pos_election(
        &self,
        operators: &[OperatorInfo],
        min_stake: u128,
        epoch: u64,
    ) -> Result<([u8; 20], Vec<[u8; 20]>, Vec<u8>)> {
        // Filter by minimum stake
        let eligible: Vec<_> = operators
            .iter()
            .filter(|op| op.stake >= min_stake)
            .collect();
        
        if eligible.is_empty() {
            return Err(eyre::eyre!("No operators meet minimum stake requirement"));
        }
        
        // Calculate total stake
        let total_stake: u128 = eligible.iter().map(|op| op.stake).sum();
        
        // Generate randomness based on epoch
        let mut hasher = Keccak256::new();
        hasher.update(epoch.to_le_bytes());
        hasher.update(b"leader_election");
        let random = hasher.finalize();
        let random_value = u128::from_le_bytes(random[0..16].try_into().unwrap()) % total_stake;
        
        // Select leader based on stake weight
        let mut cumulative = 0u128;
        let mut leader = eligible[0].address;
        
        for op in &eligible {
            cumulative += op.stake;
            if cumulative > random_value {
                leader = op.address;
                break;
            }
        }
        
        // Select backup leaders (top 3 by stake, excluding leader)
        let mut backups: Vec<_> = eligible
            .iter()
            .filter(|op| op.address != leader)
            .map(|op| (op.address, op.stake))
            .collect();
        backups.sort_by_key(|(_, stake)| std::cmp::Reverse(*stake));
        let backup_leaders: Vec<[u8; 20]> = backups
            .into_iter()
            .take(3)
            .map(|(addr, _)| addr)
            .collect();
        
        // Generate proof
        let proof = self.generate_election_proof(epoch, leader, &backup_leaders);
        
        Ok((leader, backup_leaders, proof))
    }
    
    /// Performance-based election
    async fn performance_election(
        &self,
        operators: &[OperatorInfo],
        window_size: u64,
        epoch: u64,
    ) -> Result<([u8; 20], Vec<[u8; 20]>, Vec<u8>)> {
        // Score = performance_score * availability * (stake / 1 ETH)
        let mut scored: Vec<_> = operators
            .iter()
            .map(|op| {
                let stake_factor = (op.stake / 1_000_000_000_000_000_000) as f64;
                let score = op.performance_score as f64 
                    * op.availability_percentage 
                    * stake_factor.sqrt();
                (op.address, score)
            })
            .collect();
        
        scored.sort_by(|(_, a), (_, b)| b.partial_cmp(a).unwrap());
        
        let leader = scored[0].0;
        let backup_leaders: Vec<[u8; 20]> = scored
            .iter()
            .skip(1)
            .take(3)
            .map(|(addr, _)| *addr)
            .collect();
        
        let proof = self.generate_election_proof(epoch, leader, &backup_leaders);
        
        Ok((leader, backup_leaders, proof))
    }
    
    /// Round-robin election
    async fn round_robin_election(
        &self,
        operators: &[OperatorInfo],
        epoch: u64,
    ) -> Result<([u8; 20], Vec<[u8; 20]>, Vec<u8>)> {
        let idx = (epoch as usize - 1) % operators.len();
        let leader = operators[idx].address;
        
        // Next 3 in rotation as backups
        let backup_leaders: Vec<[u8; 20]> = (1..=3)
            .map(|i| operators[(idx + i) % operators.len()].address)
            .collect();
        
        let proof = self.generate_election_proof(epoch, leader, &backup_leaders);
        
        Ok((leader, backup_leaders, proof))
    }
    
    /// VRF-based election
    async fn vrf_election(
        &self,
        operators: &[OperatorInfo],
        seed: [u8; 32],
        epoch: u64,
    ) -> Result<([u8; 20], Vec<[u8; 20]>, Vec<u8>)> {
        // Generate VRF output for each operator
        let mut vrf_outputs: Vec<_> = operators
            .iter()
            .map(|op| {
                let mut hasher = Keccak256::new();
                hasher.update(seed);
                hasher.update(op.address);
                hasher.update(epoch.to_le_bytes());
                let output = hasher.finalize();
                (op.address, output)
            })
            .collect();
        
        // Sort by VRF output
        vrf_outputs.sort_by_key(|(_, output)| *output);
        
        let leader = vrf_outputs[0].0;
        let backup_leaders: Vec<[u8; 20]> = vrf_outputs
            .iter()
            .skip(1)
            .take(3)
            .map(|(addr, _)| *addr)
            .collect();
        
        // Generate proof including VRF outputs
        let mut proof = Vec::new();
        proof.extend_from_slice(&seed);
        proof.extend_from_slice(&epoch.to_le_bytes());
        for (addr, output) in vrf_outputs.iter().take(4) {
            proof.extend_from_slice(addr);
            proof.extend_from_slice(output);
        }
        
        Ok((leader, backup_leaders, proof))
    }
    
    /// Generate election proof
    fn generate_election_proof(
        &self,
        epoch: u64,
        leader: [u8; 20],
        backup_leaders: &[[u8; 20]],
    ) -> Vec<u8> {
        let mut proof = Vec::new();
        
        // Include epoch
        proof.extend_from_slice(&epoch.to_le_bytes());
        
        // Include algorithm identifier
        let algo_id = match &self.algorithm {
            ElectionAlgorithm::ProofOfStake { .. } => 1u8,
            ElectionAlgorithm::PerformanceBased { .. } => 2u8,
            ElectionAlgorithm::RoundRobin => 3u8,
            ElectionAlgorithm::VRF { .. } => 4u8,
        };
        proof.push(algo_id);
        
        // Include leader
        proof.extend_from_slice(&leader);
        
        // Include backup count and addresses
        proof.push(backup_leaders.len() as u8);
        for backup in backup_leaders {
            proof.extend_from_slice(backup);
        }
        
        // Include timestamp
        proof.extend_from_slice(&chrono::Utc::now().timestamp().to_le_bytes());
        
        // Hash the proof
        let mut hasher = Keccak256::new();
        hasher.update(&proof);
        let hash = hasher.finalize();
        
        proof.extend_from_slice(&hash);
        proof
    }
    
    /// Verify election proof
    pub async fn verify_election_proof(
        &self,
        epoch: u64,
        leader: [u8; 20],
        proof: &[u8],
    ) -> Result<bool> {
        if proof.len() < 8 + 1 + 20 + 1 + 8 + 32 {
            return Ok(false);
        }
        
        // Extract epoch from proof
        let proof_epoch = u64::from_le_bytes(proof[0..8].try_into()?);
        if proof_epoch != epoch {
            return Ok(false);
        }
        
        // Extract leader from proof
        let proof_leader: [u8; 20] = proof[9..29].try_into()?;
        if proof_leader != leader {
            return Ok(false);
        }
        
        // Verify hash
        let proof_len = proof.len();
        let hash_start = proof_len - 32;
        let computed_hash = {
            let mut hasher = Keccak256::new();
            hasher.update(&proof[..hash_start]);
            hasher.finalize()
        };
        
        Ok(&proof[hash_start..] == computed_hash.as_slice())
    }
    
    /// Get election statistics
    pub async fn get_election_stats(&self) -> Result<ElectionStats> {
        let history = self.leader_history.read().await;
        let operators = self.operators.read().await;
        
        let mut leader_counts: HashMap<[u8; 20], u32> = HashMap::new();
        for record in history.iter() {
            *leader_counts.entry(record.leader).or_default() += 1;
        }
        
        Ok(ElectionStats {
            total_elections: history.len() as u64,
            unique_leaders: leader_counts.len() as u32,
            current_epoch: *self.current_epoch.read().await,
            active_operators: operators.values().filter(|op| op.is_active).count() as u32,
            leader_distribution: leader_counts,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ElectionStats {
    pub total_elections: u64,
    pub unique_leaders: u32,
    pub current_epoch: u64,
    pub active_operators: u32,
    pub leader_distribution: HashMap<[u8; 20], u32>,
}