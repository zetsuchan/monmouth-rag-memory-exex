use std::sync::Arc;
use blst::{
    min_pk::{PublicKey, SecretKey, Signature},
    BLST_ERROR,
};
use ark_bn254::{Fq, Fq2, G1Affine, G2Affine};
use ark_ec::AffineRepr;
use ark_ff::PrimeField;
use ark_serialize::{CanonicalSerialize, CanonicalDeserialize};
use eyre::{Result, eyre};
use sha3::{Sha3_256, Digest};
use serde::{Serialize, Deserialize};

const DST: &[u8] = b"EIGENLAYER_BLS_SIG_V1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlsKeyPair {
    pub secret_key: Vec<u8>,
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct BlsPublicKey {
    pub g1_pubkey: G1PublicKey,
    pub g2_pubkey: G2PublicKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct G1PublicKey {
    pub x: String,
    pub y: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct G2PublicKey {
    pub x: [String; 2],
    pub y: [String; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlsSignature {
    pub g1_signature: Option<G1Signature>,
    pub g2_signature: Option<G2Signature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct G1Signature {
    pub x: String,
    pub y: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct G2Signature {
    pub x: [String; 2],
    pub y: [String; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSignature {
    pub signature: BlsSignature,
    pub signers: Vec<BlsPublicKey>,
    pub total_stake: u64,
}

impl BlsKeyPair {
    pub fn generate() -> Result<Self> {
        let secret_key = SecretKey::key_gen(&rand::random::<[u8; 32]>(), &[])
            .map_err(|e| eyre!("Failed to generate secret key: {:?}", e))?;
        
        let public_key = PublicKey::from_secret_key(&secret_key);
        
        let mut sk_bytes = vec![0u8; 32];
        secret_key.serialize(&mut sk_bytes)?;
        
        let g1_pubkey = Self::public_key_to_g1(&public_key)?;
        let g2_pubkey = Self::public_key_to_g2(&public_key)?;
        
        Ok(BlsKeyPair {
            secret_key: sk_bytes,
            public_key: BlsPublicKey {
                g1_pubkey,
                g2_pubkey,
            },
        })
    }
    
    pub fn from_secret_key(secret_key_bytes: &[u8]) -> Result<Self> {
        if secret_key_bytes.len() != 32 {
            return Err(eyre!("Secret key must be 32 bytes"));
        }
        
        let secret_key = SecretKey::deserialize(secret_key_bytes)
            .map_err(|e| eyre!("Failed to deserialize secret key: {:?}", e))?;
        
        let public_key = PublicKey::from_secret_key(&secret_key);
        
        let g1_pubkey = Self::public_key_to_g1(&public_key)?;
        let g2_pubkey = Self::public_key_to_g2(&public_key)?;
        
        Ok(BlsKeyPair {
            secret_key: secret_key_bytes.to_vec(),
            public_key: BlsPublicKey {
                g1_pubkey,
                g2_pubkey,
            },
        })
    }
    
    pub fn sign_message(&self, message: &[u8]) -> Result<BlsSignature> {
        let secret_key = SecretKey::deserialize(&self.secret_key)
            .map_err(|e| eyre!("Failed to deserialize secret key: {:?}", e))?;
        
        let signature = secret_key.sign(message, DST, &[]);
        
        let g2_signature = Self::signature_to_g2(&signature)?;
        
        Ok(BlsSignature {
            g1_signature: None,
            g2_signature: Some(g2_signature),
        })
    }
    
    pub fn verify_signature(
        public_key: &BlsPublicKey,
        message: &[u8],
        signature: &BlsSignature,
    ) -> Result<bool> {
        let pk_bytes = Self::g1_pubkey_to_bytes(&public_key.g1_pubkey)?;
        let public_key = PublicKey::deserialize(&pk_bytes)
            .map_err(|e| eyre!("Failed to deserialize public key: {:?}", e))?;
        
        if let Some(g2_sig) = &signature.g2_signature {
            let sig_bytes = Self::g2_signature_to_bytes(g2_sig)?;
            let sig = Signature::deserialize(&sig_bytes)
                .map_err(|e| eyre!("Failed to deserialize signature: {:?}", e))?;
            
            let result = sig.verify(true, message, DST, &[], &public_key, true);
            Ok(result == BLST_ERROR::BLST_SUCCESS)
        } else {
            Err(eyre!("No G2 signature provided"))
        }
    }
    
    fn public_key_to_g1(public_key: &PublicKey) -> Result<G1PublicKey> {
        let mut bytes = vec![0u8; 48];
        public_key.serialize(&mut bytes)?;
        
        let g1_point = G1Affine::deserialize_compressed(&bytes[..])
            .map_err(|e| eyre!("Failed to deserialize G1 point: {:?}", e))?;
        
        Ok(G1PublicKey {
            x: g1_point.x.to_string(),
            y: g1_point.y.to_string(),
        })
    }
    
    fn public_key_to_g2(public_key: &PublicKey) -> Result<G2PublicKey> {
        let mut hasher = Sha3_256::new();
        let mut pk_bytes = vec![0u8; 48];
        public_key.serialize(&mut pk_bytes)?;
        hasher.update(&pk_bytes);
        let hash = hasher.finalize();
        
        let c0 = Fq::from_le_bytes_mod_order(&hash[..32]);
        let c1 = Fq::from_le_bytes_mod_order(&hash[16..]);
        
        Ok(G2PublicKey {
            x: [c0.to_string(), c1.to_string()],
            y: [c0.to_string(), c1.to_string()],
        })
    }
    
    fn signature_to_g2(signature: &Signature) -> Result<G2Signature> {
        let mut bytes = vec![0u8; 96];
        signature.serialize(&mut bytes)?;
        
        let g2_point = G2Affine::deserialize_compressed(&bytes[..])
            .map_err(|e| eyre!("Failed to deserialize G2 point: {:?}", e))?;
        
        let x0 = g2_point.x.c0.to_string();
        let x1 = g2_point.x.c1.to_string();
        let y0 = g2_point.y.c0.to_string();
        let y1 = g2_point.y.c1.to_string();
        
        Ok(G2Signature {
            x: [x0, x1],
            y: [y0, y1],
        })
    }
    
    fn g1_pubkey_to_bytes(g1_pubkey: &G1PublicKey) -> Result<Vec<u8>> {
        let x = Fq::from_str(&g1_pubkey.x)
            .map_err(|e| eyre!("Failed to parse x coordinate: {:?}", e))?;
        let y = Fq::from_str(&g1_pubkey.y)
            .map_err(|e| eyre!("Failed to parse y coordinate: {:?}", e))?;
        
        let point = G1Affine::new(x, y);
        let mut bytes = vec![];
        point.serialize_compressed(&mut bytes)?;
        
        Ok(bytes)
    }
    
    fn g2_signature_to_bytes(g2_sig: &G2Signature) -> Result<Vec<u8>> {
        let x0 = Fq::from_str(&g2_sig.x[0])
            .map_err(|e| eyre!("Failed to parse x0: {:?}", e))?;
        let x1 = Fq::from_str(&g2_sig.x[1])
            .map_err(|e| eyre!("Failed to parse x1: {:?}", e))?;
        let y0 = Fq::from_str(&g2_sig.y[0])
            .map_err(|e| eyre!("Failed to parse y0: {:?}", e))?;
        let y1 = Fq::from_str(&g2_sig.y[1])
            .map_err(|e| eyre!("Failed to parse y1: {:?}", e))?;
        
        let x = Fq2::new(x0, x1);
        let y = Fq2::new(y0, y1);
        let point = G2Affine::new(x, y);
        
        let mut bytes = vec![];
        point.serialize_compressed(&mut bytes)?;
        
        Ok(bytes)
    }
}

pub struct BlsAggregator;

impl BlsAggregator {
    pub fn aggregate_signatures(
        signatures: Vec<(BlsSignature, BlsPublicKey, u64)>,
    ) -> Result<AggregatedSignature> {
        if signatures.is_empty() {
            return Err(eyre!("No signatures to aggregate"));
        }
        
        let mut sig_bytes_vec = Vec::new();
        let mut signers = Vec::new();
        let mut total_stake = 0u64;
        
        for (sig, pubkey, stake) in signatures {
            if let Some(g2_sig) = &sig.g2_signature {
                let bytes = BlsKeyPair::g2_signature_to_bytes(g2_sig)?;
                sig_bytes_vec.push(bytes);
                signers.push(pubkey);
                total_stake += stake;
            }
        }
        
        if sig_bytes_vec.is_empty() {
            return Err(eyre!("No valid signatures to aggregate"));
        }
        
        let signatures: Result<Vec<Signature>, _> = sig_bytes_vec
            .iter()
            .map(|bytes| {
                Signature::deserialize(bytes)
                    .map_err(|e| eyre!("Failed to deserialize signature: {:?}", e))
            })
            .collect();
        
        let signatures = signatures?;
        let aggregated = Signature::aggregate(&signatures, true)
            .map_err(|e| eyre!("Failed to aggregate signatures: {:?}", e))?;
        
        let mut agg_bytes = vec![0u8; 96];
        aggregated.serialize(&mut agg_bytes)?;
        
        let g2_sig = BlsKeyPair::signature_to_g2(&aggregated)?;
        
        Ok(AggregatedSignature {
            signature: BlsSignature {
                g1_signature: None,
                g2_signature: Some(g2_sig),
            },
            signers,
            total_stake,
        })
    }
    
    pub fn verify_aggregated_signature(
        aggregated: &AggregatedSignature,
        message: &[u8],
    ) -> Result<bool> {
        if aggregated.signers.is_empty() {
            return Ok(false);
        }
        
        let mut pubkeys = Vec::new();
        for signer in &aggregated.signers {
            let pk_bytes = BlsKeyPair::g1_pubkey_to_bytes(&signer.g1_pubkey)?;
            let pk = PublicKey::deserialize(&pk_bytes)
                .map_err(|e| eyre!("Failed to deserialize public key: {:?}", e))?;
            pubkeys.push(pk);
        }
        
        let aggregated_pubkey = PublicKey::aggregate(&pubkeys, true)
            .map_err(|e| eyre!("Failed to aggregate public keys: {:?}", e))?;
        
        let g2_sig = aggregated.signature.g2_signature.as_ref()
            .ok_or_else(|| eyre!("No G2 signature in aggregated signature"))?;
        
        let sig_bytes = BlsKeyPair::g2_signature_to_bytes(g2_sig)?;
        let sig = Signature::deserialize(&sig_bytes)
            .map_err(|e| eyre!("Failed to deserialize signature: {:?}", e))?;
        
        let result = sig.verify(true, message, DST, &[], &aggregated_pubkey, true);
        Ok(result == BLST_ERROR::BLST_SUCCESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_key_generation() {
        let keypair = BlsKeyPair::generate().unwrap();
        assert_eq!(keypair.secret_key.len(), 32);
        assert!(!keypair.public_key.g1_pubkey.x.is_empty());
        assert!(!keypair.public_key.g1_pubkey.y.is_empty());
    }
    
    #[test]
    fn test_sign_verify() {
        let keypair = BlsKeyPair::generate().unwrap();
        let message = b"test message";
        
        let signature = keypair.sign_message(message).unwrap();
        let is_valid = BlsKeyPair::verify_signature(
            &keypair.public_key,
            message,
            &signature,
        ).unwrap();
        
        assert!(is_valid);
    }
    
    #[test]
    fn test_signature_aggregation() {
        let message = b"test message";
        let mut signatures = Vec::new();
        
        for i in 0..3 {
            let keypair = BlsKeyPair::generate().unwrap();
            let signature = keypair.sign_message(message).unwrap();
            signatures.push((signature, keypair.public_key, 100 * (i + 1)));
        }
        
        let aggregated = BlsAggregator::aggregate_signatures(signatures).unwrap();
        assert_eq!(aggregated.total_stake, 600);
        assert_eq!(aggregated.signers.len(), 3);
        
        let is_valid = BlsAggregator::verify_aggregated_signature(&aggregated, message).unwrap();
        assert!(is_valid);
    }
}