use blst::{PublicKey, SecretKey, Signature, AggregatePublicKey, AggregateSignature};
use eyre::Result;
use std::collections::HashMap;

#[derive(Debug)]
pub struct BLSCoordinator {
    validators: HashMap<String, ValidatorInfo>,
    threshold: usize,
}

#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub id: String,
    pub public_key: Vec<u8>,
    pub weight: u64,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct CoordinationMessage {
    pub msg_type: MessageType,
    pub agent_id: String,
    pub payload: Vec<u8>,
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Clone)]
pub enum MessageType {
    ConsensusRequest,
    ConsensusResponse,
    StateSync,
    AgentCoordination,
}

impl BLSCoordinator {
    pub fn new(threshold: usize) -> Self {
        Self {
            validators: HashMap::new(),
            threshold,
        }
    }
    
    pub fn add_validator(&mut self, validator: ValidatorInfo) {
        self.validators.insert(validator.id.clone(), validator);
    }
    
    pub fn remove_validator(&mut self, validator_id: &str) {
        self.validators.remove(validator_id);
    }
    
    pub fn create_signature(&self, secret_key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
        let sk = SecretKey::from_bytes(secret_key)?;
        let sig = sk.sign(message, b"monmouth-rag-memory", &[]);
        Ok(sig.to_bytes().to_vec())
    }
    
    pub fn verify_signature(
        &self,
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<bool> {
        let pk = PublicKey::from_bytes(public_key)?;
        let sig = Signature::from_bytes(signature)?;
        
        let result = sig.verify(true, message, b"monmouth-rag-memory", &[], &pk, true);
        Ok(result == blst::BLST_ERROR::BLST_SUCCESS)
    }
    
    pub fn aggregate_signatures(&self, signatures: Vec<Vec<u8>>) -> Result<Vec<u8>> {
        if signatures.is_empty() {
            return Err(eyre::eyre!("No signatures to aggregate"));
        }
        
        let mut agg_sig = AggregateSignature::from_signature(&Signature::from_bytes(&signatures[0])?);
        
        for sig_bytes in signatures.iter().skip(1) {
            let sig = Signature::from_bytes(sig_bytes)?;
            agg_sig.add_signature(&sig, true)?;
        }
        
        Ok(agg_sig.to_signature().to_bytes().to_vec())
    }
    
    pub fn verify_aggregate_signature(
        &self,
        public_keys: Vec<Vec<u8>>,
        messages: Vec<Vec<u8>>,
        aggregate_signature: &[u8],
    ) -> Result<bool> {
        if public_keys.len() != messages.len() {
            return Err(eyre::eyre!("Public keys and messages count mismatch"));
        }
        
        let sig = Signature::from_bytes(aggregate_signature)?;
        
        let pks: Result<Vec<PublicKey>, _> = public_keys.iter()
            .map(|pk| PublicKey::from_bytes(pk))
            .collect();
        let pks = pks?;
        
        let pk_refs: Vec<&PublicKey> = pks.iter().collect();
        let msg_refs: Vec<&[u8]> = messages.iter().map(|m| m.as_slice()).collect();
        
        let result = sig.aggregate_verify(
            true,
            &msg_refs,
            b"monmouth-rag-memory",
            &pk_refs,
            true,
        );
        
        Ok(result == blst::BLST_ERROR::BLST_SUCCESS)
    }
    
    pub fn check_threshold(&self, validators: &[String]) -> bool {
        let total_weight: u64 = validators.iter()
            .filter_map(|id| self.validators.get(id))
            .filter(|v| v.active)
            .map(|v| v.weight)
            .sum();
        
        let required_weight = self.total_weight() * 2 / 3;
        total_weight > required_weight
    }
    
    pub fn coordinate_agents(
        &self,
        agent_ids: Vec<String>,
        action: &str,
    ) -> Result<CoordinationResult> {
        let coordination_id = uuid::Uuid::new_v4().to_string();
        
        let message = CoordinationMessage {
            msg_type: MessageType::AgentCoordination,
            agent_id: coordination_id.clone(),
            payload: action.as_bytes().to_vec(),
            timestamp: std::time::Instant::now(),
        };
        
        Ok(CoordinationResult {
            coordination_id,
            participating_agents: agent_ids,
            status: CoordinationStatus::Pending,
            signatures: Vec::new(),
        })
    }
    
    fn total_weight(&self) -> u64 {
        self.validators.values()
            .filter(|v| v.active)
            .map(|v| v.weight)
            .sum()
    }
}

#[derive(Debug, Clone)]
pub struct CoordinationResult {
    pub coordination_id: String,
    pub participating_agents: Vec<String>,
    pub status: CoordinationStatus,
    pub signatures: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum CoordinationStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}