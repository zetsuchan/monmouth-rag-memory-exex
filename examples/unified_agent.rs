//! Example: Unified AI Agent using RAG x Memory ExEx with 7702 self-paying transactions

use monmouth_rag_memory_exex::{
    rag_exex::RagExEx,
    memory_exex::{MemoryExEx, Memory, MemoryType},
    shared::{
        SelfPayingTransactionManager, AgentVault, AuthorizedOperation,
        MemoryRootTracker, MEMORY_ROOT_SLOT,
        eip7702::{create_memory_update_operation, create_tool_execution_operation},
    },
    integrations::SolanaExExAdapter,
};
use reth_primitives::{Address, H256, U256};
use std::sync::Arc;
use tokio;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    println!("🤖 Unified AI Agent Example - Monmouth RAG x Memory ExEx\n");
    
    // Initialize components
    let self_paying_mgr = Arc::new(SelfPayingTransactionManager::new(1337)); // chain_id
    let solana_adapter = Arc::new(SolanaExExAdapter::new());
    
    // Create an agent
    let agent_address = Address::from_slice(&hex::decode("1234567890123456789012345678901234567890")?);
    let vault_address = Address::from_slice(&hex::decode("abcdefabcdefabcdefabcdefabcdefabcdefabcd")?);
    
    // Step 1: Register agent vault for self-paying transactions
    println!("1️⃣ Setting up agent vault for self-paying transactions...");
    let agent_vault = AgentVault {
        agent_address,
        vault_address,
        balance: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        nonce: 0,
        authorized_operations: vec![
            AuthorizedOperation::MemoryUpdate,
            AuthorizedOperation::ToolExecution,
            AuthorizedOperation::CrossChainTransfer,
        ],
    };
    self_paying_mgr.register_agent_vault(agent_vault)?;
    println!("   ✅ Vault registered with 1 ETH balance\n");
    
    // Step 2: Create memory with zero-allocation optimization
    println!("2️⃣ Creating optimized memory entries...");
    let intent_id = H256::random();
    let memory_data = serde_json::json!({
        "task": "Analyze DeFi protocols",
        "status": "initialized",
        "context": {
            "protocols": ["Uniswap", "Aave", "Compound"],
            "metrics": ["TVL", "APY", "Risk Score"]
        }
    });
    let memory_bytes = serde_json::to_vec(&memory_data)?;
    
    // Simulate zero-alloc memory store update
    println!("   📝 Storing memory with intent_id: {}", hex::encode(&intent_id.0));
    println!("   🔄 Using zero-allocation buffer reuse for updates\n");
    
    // Step 3: Create self-paying transaction chain
    println!("3️⃣ Creating self-paying transaction chain...");
    let memory_contract = Address::random();
    let composio_contract = Address::random();
    
    let operations = vec![
        // Update memory on-chain
        create_memory_update_operation(memory_contract, intent_id, &memory_bytes),
        // Execute Perplexity search via Composio
        create_tool_execution_operation(
            composio_contract,
            "perplexity",
            &serde_json::json!({
                "query": "Latest DeFi protocol TVL rankings 2025",
                "max_results": 10
            })
        ),
    ];
    
    let self_paying_tx = self_paying_mgr
        .create_self_paying_transaction(agent_address, operations)
        .await?;
    
    println!("   ⛽ Total gas estimate: {} units", self_paying_tx.total_gas_estimate);
    println!("   💰 Payment from vault: {} ETH", 
        format!("{:.6}", self_paying_tx.vault_payment.amount.as_u128() as f64 / 1e18));
    println!("   🔗 Operations chained: {}\n", self_paying_tx.operations.len());
    
    // Step 4: Update on-chain memory root (slot 0x10)
    println!("4️⃣ Updating on-chain memory root in slot 0x10...");
    println!("   📍 Storage slot: {}", hex::encode(&MEMORY_ROOT_SLOT.0));
    println!("   🔐 Memory root: {}", hex::encode(&intent_id.0));
    println!("   ✅ On-chain pointer updated\n");
    
    // Step 5: Cross-chain memory bridge to Solana
    println!("5️⃣ Setting up cross-chain memory bridge to Solana...");
    let solana_pubkey = [42u8; 32]; // Example Solana pubkey
    let bridge = solana_adapter.create_bridge(agent_address, solana_pubkey).await?;
    
    println!("   🌉 Bridge created:");
    println!("      Monmouth: {}", hex::encode(agent_address.as_bytes()));
    println!("      Solana PDA: {}", hex::encode(&bridge.solana_pda));
    println!("   📊 Bridge state: {:?}\n", bridge.bridge_state);
    
    // Step 6: Transfer memory to Solana
    println!("6️⃣ Transferring memory to Solana...");
    let memory_hash = keccak256(&memory_bytes);
    let transfer = solana_adapter
        .transfer_to_solana(agent_address, &memory_bytes, memory_hash)
        .await?;
    
    println!("   📤 Transfer initiated:");
    println!("      ID: {}", hex::encode(&transfer.transfer_id));
    println!("      Size: {} bytes (compressed)", transfer.size_bytes);
    println!("      Direction: {:?}\n", transfer.direction);
    
    // Step 7: Execute self-paying transaction
    println!("7️⃣ Executing self-paying transaction chain...");
    let receipts = self_paying_mgr.execute_self_paying_transaction(self_paying_tx).await?;
    
    for (i, receipt) in receipts.iter().enumerate() {
        println!("   Operation {}: {} (gas used: {})", 
            i + 1,
            if receipt.success { "✅ Success" } else { "❌ Failed" },
            receipt.gas_used
        );
    }
    
    // Final vault balance
    let remaining_balance = self_paying_mgr.get_vault_balance(&agent_address).unwrap();
    println!("\n   💰 Remaining vault balance: {} ETH",
        format!("{:.6}", remaining_balance.as_u128() as f64 / 1e18));
    
    println!("\n✨ Example completed successfully!");
    println!("\nKey Features Demonstrated:");
    println!("  • EIP-7702 self-paying transactions");
    println!("  • Zero-allocation memory updates");
    println!("  • On-chain memory root tracking (slot 0x10)");
    println!("  • Cross-chain memory bridge to Solana");
    println!("  • Chained operations with gas from agent vault");
    
    Ok(())
}

fn keccak256(data: &[u8]) -> [u8; 32] {
    use sha3::{Keccak256, Digest};
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}