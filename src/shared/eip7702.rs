use eyre::Result;
use reth_primitives::TransactionSigned;
use alloy::primitives::{Address, U256, Bytes, B256};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use dashmap::DashMap;

/// EIP-7702 Self-Paying Transaction Support
/// 
/// This module enables agents to execute transactions that pay for their own gas
/// from a dedicated vault, allowing for autonomous operation without external funding.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVault {
    pub agent_address: Address,
    pub vault_address: Address,
    pub balance: U256,
    pub nonce: u64,
    pub authorized_operations: Vec<AuthorizedOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthorizedOperation {
    MemoryUpdate,
    ToolExecution,
    CrossChainTransfer,
    SubtaskChaining,
}

#[derive(Debug)]
pub struct SelfPayingTransactionManager {
    agent_vaults: Arc<DashMap<Address, AgentVault>>,
    gas_estimator: Arc<GasEstimator>,
    transaction_builder: Arc<TransactionBuilder>,
}

#[derive(Debug)]
pub struct GasEstimator {
    base_costs: DashMap<AuthorizedOperation, u64>,
    dynamic_multiplier: f64,
}

#[derive(Debug)]
pub struct TransactionBuilder {
    chain_id: u64,
}

#[derive(Debug, Clone)]
pub struct SelfPayingTransaction {
    pub agent: Address,
    pub operations: Vec<ChainedOperation>,
    pub total_gas_estimate: u64,
    pub vault_payment: VaultPayment,
}

#[derive(Debug, Clone)]
pub struct ChainedOperation {
    pub operation_type: AuthorizedOperation,
    pub target: Address,
    pub data: Bytes,
    pub gas_limit: u64,
}

#[derive(Debug, Clone)]
pub struct VaultPayment {
    pub vault_address: Address,
    pub amount: U256,
    pub signature: Vec<u8>,
}

impl SelfPayingTransactionManager {
    pub fn new(chain_id: u64) -> Self {
        let mut base_costs = DashMap::new();
        base_costs.insert(AuthorizedOperation::MemoryUpdate, 50_000);
        base_costs.insert(AuthorizedOperation::ToolExecution, 100_000);
        base_costs.insert(AuthorizedOperation::CrossChainTransfer, 200_000);
        base_costs.insert(AuthorizedOperation::SubtaskChaining, 30_000);
        
        Self {
            agent_vaults: Arc::new(DashMap::new()),
            gas_estimator: Arc::new(GasEstimator {
                base_costs,
                dynamic_multiplier: 1.2,
            }),
            transaction_builder: Arc::new(TransactionBuilder { chain_id }),
        }
    }
    
    pub fn register_agent_vault(&self, vault: AgentVault) -> Result<()> {
        self.agent_vaults.insert(vault.agent_address, vault);
        Ok(())
    }
    
    pub async fn create_self_paying_transaction(
        &self,
        agent: Address,
        operations: Vec<ChainedOperation>,
    ) -> Result<SelfPayingTransaction> {
        let vault = self.agent_vaults.get(&agent)
            .ok_or_else(|| eyre::eyre!("Agent vault not found"))?;
        
        // Estimate total gas for all operations
        let total_gas = self.estimate_chained_gas(&operations)?;
        
        // Check vault balance
        let gas_cost = U256::from(total_gas) * U256::from(50_000_000_000u64); // 50 gwei
        if vault.balance < gas_cost {
            return Err(eyre::eyre!("Insufficient vault balance"));
        }
        
        // Create vault payment
        let vault_payment = VaultPayment {
            vault_address: vault.vault_address,
            amount: gas_cost,
            signature: self.sign_vault_payment(&vault, gas_cost)?,
        };
        
        Ok(SelfPayingTransaction {
            agent,
            operations,
            total_gas_estimate: total_gas,
            vault_payment,
        })
    }
    
    pub async fn execute_self_paying_transaction(
        &self,
        tx: SelfPayingTransaction,
    ) -> Result<Vec<TransactionReceipt>> {
        let mut receipts = Vec::new();
        
        // Execute operations in sequence
        for operation in &tx.operations {
            let receipt = self.execute_operation(&tx.agent, operation).await?;
            receipts.push(receipt);
            
            // Check if operation succeeded before continuing chain
            if !receipt.success {
                break;
            }
        }
        
        // Deduct gas from vault
        self.deduct_vault_balance(&tx.agent, tx.vault_payment.amount)?;
        
        Ok(receipts)
    }
    
    fn estimate_chained_gas(&self, operations: &[ChainedOperation]) -> Result<u64> {
        let base_gas: u64 = operations.iter()
            .map(|op| {
                self.gas_estimator.base_costs
                    .get(&op.operation_type)
                    .map(|cost| *cost)
                    .unwrap_or(100_000)
            })
            .sum();
        
        // Apply dynamic multiplier for safety margin
        Ok((base_gas as f64 * self.gas_estimator.dynamic_multiplier) as u64)
    }
    
    fn sign_vault_payment(&self, vault: &AgentVault, amount: U256) -> Result<Vec<u8>> {
        // In production, this would use proper cryptographic signing
        // For now, return a placeholder signature
        Ok(vec![0; 65])
    }
    
    async fn execute_operation(
        &self,
        agent: &Address,
        operation: &ChainedOperation,
    ) -> Result<TransactionReceipt> {
        // Simulate operation execution
        Ok(TransactionReceipt {
            transaction_hash: B256::random(),
            success: true,
            gas_used: operation.gas_limit,
            logs: vec![],
        })
    }
    
    fn deduct_vault_balance(&self, agent: &Address, amount: U256) -> Result<()> {
        if let Some(mut vault) = self.agent_vaults.get_mut(agent) {
            if vault.balance >= amount {
                vault.balance -= amount;
                vault.nonce += 1;
                Ok(())
            } else {
                Err(eyre::eyre!("Insufficient vault balance during deduction"))
            }
        } else {
            Err(eyre::eyre!("Agent vault not found"))
        }
    }
    
    pub fn get_vault_balance(&self, agent: &Address) -> Option<U256> {
        self.agent_vaults.get(agent).map(|v| v.balance)
    }
}

#[derive(Debug, Clone)]
pub struct TransactionReceipt {
    pub transaction_hash: B256,
    pub success: bool,
    pub gas_used: u64,
    pub logs: Vec<Log>,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Bytes,
}

/// Helper to create memory update operations
pub fn create_memory_update_operation(
    memory_contract: Address,
    intent_id: B256,
    data: &[u8],
) -> ChainedOperation {
    // Encode function call: updateMemory(bytes32 intentId, bytes data)
    let mut encoded = vec![0x12, 0x34, 0x56, 0x78]; // Function selector
    encoded.extend_from_slice(&intent_id.0);
    encoded.extend_from_slice(&encode_bytes(data));
    
    ChainedOperation {
        operation_type: AuthorizedOperation::MemoryUpdate,
        target: memory_contract,
        data: Bytes::from(encoded),
        gas_limit: 100_000,
    }
}

/// Helper to create tool execution operations
pub fn create_tool_execution_operation(
    tool_contract: Address,
    tool_name: &str,
    params: &serde_json::Value,
) -> ChainedOperation {
    let params_bytes = serde_json::to_vec(params).unwrap_or_default();
    
    let mut encoded = vec![0xaa, 0xbb, 0xcc, 0xdd]; // Function selector
    encoded.extend_from_slice(&encode_string(tool_name));
    encoded.extend_from_slice(&encode_bytes(&params_bytes));
    
    ChainedOperation {
        operation_type: AuthorizedOperation::ToolExecution,
        target: tool_contract,
        data: Bytes::from(encoded),
        gas_limit: 200_000,
    }
}

fn encode_bytes(data: &[u8]) -> Vec<u8> {
    let mut encoded = vec![0; 32]; // offset
    encoded.extend_from_slice(&(data.len() as u64).to_be_bytes()[56..64]); // length
    encoded.extend_from_slice(data); // data
    // Pad to 32 bytes
    let padding = (32 - (data.len() % 32)) % 32;
    encoded.extend_from_slice(&vec![0; padding]);
    encoded
}

fn encode_string(s: &str) -> Vec<u8> {
    encode_bytes(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_self_paying_transaction() {
        let manager = SelfPayingTransactionManager::new(1);
        
        let agent = Address::random();
        let vault = AgentVault {
            agent_address: agent,
            vault_address: Address::random(),
            balance: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
            nonce: 0,
            authorized_operations: vec![
                AuthorizedOperation::MemoryUpdate,
                AuthorizedOperation::ToolExecution,
            ],
        };
        
        manager.register_agent_vault(vault).unwrap();
        
        let operations = vec![
            create_memory_update_operation(
                Address::random(),
                B256::random(),
                b"test data",
            ),
            create_tool_execution_operation(
                Address::random(),
                "perplexity",
                &serde_json::json!({"query": "test"}),
            ),
        ];
        
        let tx = manager.create_self_paying_transaction(agent, operations).await.unwrap();
        assert_eq!(tx.operations.len(), 2);
        assert!(tx.total_gas_estimate > 0);
        
        let receipts = manager.execute_self_paying_transaction(tx).await.unwrap();
        assert_eq!(receipts.len(), 2);
        
        // Check vault balance was deducted
        let remaining = manager.get_vault_balance(&agent).unwrap();
        assert!(remaining < U256::from(1_000_000_000_000_000_000u64));
    }
}