//! Enhanced Lagrange Integration Example
//! 
//! Demonstrates the full capabilities of the enhanced Lagrange integration including:
//! - Historical memory queries with time-aware proofs
//! - RAG document usage verification
//! - Cross-chain intent execution and verification
//! - Agent reputation metrics aggregation
//! - Economic fee management

use monmouth_rag_memory_exex::integrations::{
    LagrangeIntegration,
    lagrange::{
        ChainId, CrossChainIntent, CrossChainStep, CrossChainAction,
        StepStatus, IntentStatus, TokenInfo,
        reputation::{TimeRange, MetricType},
        economics::{QueryComplexity, ProofComplexity, QueryPriority},
    },
};
use std::sync::Arc;
use tokio;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    println!("🚀 Enhanced Lagrange Integration Demo\n");
    
    // Initialize Lagrange with all components
    let mut lagrange = LagrangeIntegration::new(
        "https://prover.lagrange.dev".to_string(),
        "https://aggregator.lagrange.dev".to_string(),
        "0x1234567890123456789012345678901234567890".to_string(),
    );
    
    // Start background services
    lagrange.start_services().await?;
    
    // Demo 1: Historical Memory Query (Time-Aware Agent Plans)
    println!("📚 Demo 1: Querying Historical Memory Slice\n");
    
    let agent_address = [0x12; 20];
    let intent_id = "intent_20250707_001".to_string();
    let memory_slot = 0x10; // Standard memory root slot
    
    println!("Querying memory for intent {} at slot {:#x}", intent_id, memory_slot);
    
    match lagrange.query_historical_memory(
        intent_id.clone(),
        agent_address,
        memory_slot,
        Some(12345678), // Specific block
    ).await {
        Ok(result) => {
            println!("✅ Memory retrieved successfully!");
            println!("   Memory data: {} bytes", result.memory_data.len());
            println!("   Block: {}", result.metadata.block_number);
            println!("   Memory type: {:?}", result.metadata.memory_type);
            
            match &result.proof {
                monmouth_rag_memory_exex::integrations::lagrange::historical_queries::MemoryProof::MerkleInclusion { root, path, .. } => {
                    println!("   Proof type: Merkle inclusion");
                    println!("   Root: {}", hex::encode(root));
                    println!("   Path length: {}", path.len());
                }
                monmouth_rag_memory_exex::integrations::lagrange::historical_queries::MemoryProof::DeterministicReplay { computation_hash, .. } => {
                    println!("   Proof type: Deterministic replay");
                    println!("   Computation hash: {}", hex::encode(computation_hash));
                }
                _ => {}
            }
        }
        Err(e) => {
            println!("❌ Failed to query memory: {}", e);
        }
    }
    
    // Demo 2: Cross-Chain Intent Verification
    println!("\n\n🌉 Demo 2: Cross-Chain Intent Execution\n");
    
    // Create a cross-chain DeFi intent
    let cross_chain_intent = CrossChainIntent {
        intent_id: "cross_chain_defi_001".to_string(),
        agent_address,
        steps: vec![
            CrossChainStep {
                step_id: "bridge_usdc".to_string(),
                chain_id: ChainId::new("ethereum"),
                action: CrossChainAction::Bridge {
                    bridge_protocol: "stargate".to_string(),
                    amount: 1000_000_000, // 1000 USDC
                    token: TokenInfo {
                        symbol: "USDC".to_string(),
                        decimals: 6,
                        source_address: vec![0xA0; 20],
                        target_address: Some(vec![0xB0; 20]),
                    },
                    target_chain: ChainId::new("optimism"),
                },
                dependencies: vec![],
                status: StepStatus::Pending,
                proof: None,
            },
            CrossChainStep {
                step_id: "swap_to_eth".to_string(),
                chain_id: ChainId::new("optimism"),
                action: CrossChainAction::Swap {
                    protocol: "uniswap_v3".to_string(),
                    token_in: TokenInfo {
                        symbol: "USDC".to_string(),
                        decimals: 6,
                        source_address: vec![0xB0; 20],
                        target_address: None,
                    },
                    token_out: TokenInfo {
                        symbol: "ETH".to_string(),
                        decimals: 18,
                        source_address: vec![0xC0; 20],
                        target_address: None,
                    },
                    amount_in: 1000_000_000,
                    min_amount_out: 500_000_000_000_000, // 0.5 ETH minimum
                },
                dependencies: vec!["bridge_usdc".to_string()],
                status: StepStatus::Pending,
                proof: None,
            },
            CrossChainStep {
                step_id: "deposit_aave".to_string(),
                chain_id: ChainId::new("optimism"),
                action: CrossChainAction::Deposit {
                    protocol: "aave_v3".to_string(),
                    amount: 500_000_000_000_000, // 0.5 ETH
                    token: TokenInfo {
                        symbol: "ETH".to_string(),
                        decimals: 18,
                        source_address: vec![0xC0; 20],
                        target_address: None,
                    },
                },
                dependencies: vec!["swap_to_eth".to_string()],
                status: StepStatus::Pending,
                proof: None,
            },
        ],
        current_step: 0,
        status: IntentStatus::Active,
        created_at: chrono::Utc::now().timestamp() as u64,
    };
    
    println!("Submitting cross-chain intent with {} steps:", cross_chain_intent.steps.len());
    for (i, step) in cross_chain_intent.steps.iter().enumerate() {
        println!("   Step {}: {} on {}", i + 1, step.step_id, step.chain_id.0);
    }
    
    // Submit and track the intent
    let mut event_rx = lagrange.submit_cross_chain_intent(cross_chain_intent).await?;
    
    // Monitor bridge transfer
    println!("\nMonitoring bridge transfer...");
    let bridge_action = CrossChainAction::Bridge {
        bridge_protocol: "stargate".to_string(),
        amount: 1000_000_000,
        token: TokenInfo {
            symbol: "USDC".to_string(),
            decimals: 6,
            source_address: vec![0xA0; 20],
            target_address: Some(vec![0xB0; 20]),
        },
        target_chain: ChainId::new("optimism"),
    };
    
    match lagrange.verify_cross_chain_action(
        "cross_chain_defi_001".to_string(),
        ChainId::new("ethereum"),
        bridge_action,
    ).await {
        Ok(result) => {
            println!("✅ Bridge verification: {}", if result.verified { "SUCCESS" } else { "FAILED" });
            if let Some(proof) = result.proof {
                println!("   Proof type: {:?}", proof.proof_type);
                println!("   Block: {}", proof.block_number);
            }
        }
        Err(e) => {
            println!("❌ Bridge verification error: {}", e);
        }
    }
    
    // Demo 3: Agent Reputation Metrics
    println!("\n\n📊 Demo 3: Agent Reputation Metrics\n");
    
    let agent_id = "agent_defi_optimizer_001".to_string();
    let time_range = TimeRange {
        start_block: 12000000,
        end_block: 12345678,
        start_timestamp: None,
        end_timestamp: None,
    };
    
    println!("Computing reputation metrics for agent: {}", agent_id);
    println!("Time range: blocks {} to {}", time_range.start_block, time_range.end_block);
    
    match lagrange.compute_agent_reputation(
        agent_id.clone(),
        time_range,
        true, // Include cross-chain metrics
    ).await {
        Ok(metrics) => {
            println!("\n✅ Agent Metrics Retrieved:");
            
            // Performance metrics
            println!("\n📈 Performance:");
            println!("   Total value managed: {} ETH", 
                metrics.performance_metrics.total_value_managed as f64 / 1e18);
            println!("   Average APY: {:.2}%", metrics.performance_metrics.average_apy);
            println!("   Best APY: {:.2}%", metrics.performance_metrics.best_apy);
            println!("   Worst APY: {:.2}%", metrics.performance_metrics.worst_apy);
            
            // Efficiency metrics
            println!("\n⚡ Efficiency:");
            println!("   Total gas used: {} ETH", 
                metrics.efficiency_metrics.total_gas_used as f64 / 1e18);
            println!("   Average gas per intent: {} gwei", 
                metrics.efficiency_metrics.average_gas_per_intent / 1e9);
            println!("   Average slippage: {:.2}%", 
                metrics.efficiency_metrics.average_slippage_bps as f64 / 100.0);
            
            // Reliability metrics
            println!("\n🛡️ Reliability:");
            println!("   Total intents: {}", metrics.reliability_metrics.total_intents);
            println!("   Success rate: {:.2}%", 
                metrics.reliability_metrics.success_rate * 100.0);
            println!("   Uptime: {:.2}%", 
                metrics.reliability_metrics.uptime_percentage);
            
            // Reputation score
            println!("\n⭐ Reputation Score:");
            println!("   Overall: {}/1000", metrics.reputation_score.overall_score);
            println!("   Tier: {:?}", metrics.reputation_score.tier);
            println!("   Performance: {}/250", metrics.reputation_score.performance_score);
            println!("   Efficiency: {}/250", metrics.reputation_score.efficiency_score);
            println!("   Reliability: {}/250", metrics.reputation_score.reliability_score);
            println!("   Cross-chain: {}/250", metrics.reputation_score.cross_chain_score);
            
            // Cross-chain metrics
            if let Some(cross_chain) = &metrics.cross_chain_metrics {
                println!("\n🌐 Cross-Chain Activity:");
                println!("   Active on {} chains", cross_chain.chains.len());
                println!("   Total cross-chain value: {} ETH", 
                    cross_chain.total_cross_chain_value as f64 / 1e18);
                println!("   Bridge fees paid: {} ETH", 
                    cross_chain.bridge_fees_paid as f64 / 1e18);
            }
        }
        Err(e) => {
            println!("❌ Failed to compute metrics: {}", e);
        }
    }
    
    // Demo 4: Economic Model - Query Fee Calculation
    println!("\n\n💰 Demo 4: Query Fee Economics\n");
    
    let query_types = vec![
        ("Historical Memory", monmouth_rag_memory_exex::integrations::lagrange::QueryType::HistoricalMemory),
        ("RAG Document", monmouth_rag_memory_exex::integrations::lagrange::QueryType::RAGDocument),
        ("Cross-Chain State", monmouth_rag_memory_exex::integrations::lagrange::QueryType::CrossChainState),
    ];
    
    for (name, query_type) in query_types {
        let complexity = QueryComplexity {
            data_size_kb: 100,
            time_range_blocks: 1000,
            cross_chain_hops: if name == "Cross-Chain State" { 2 } else { 0 },
            proof_complexity: ProofComplexity::Complex,
        };
        
        match lagrange.economics_manager.calculate_fee(
            query_type,
            complexity,
            QueryPriority::High,
        ).await {
            Ok(fee) => {
                println!("\n{} Query Fee:", name);
                println!("   Base fee: {} ETH", fee.base_fee as f64 / 1e18);
                println!("   Complexity fee: {} ETH", fee.complexity_fee as f64 / 1e18);
                println!("   Priority fee: {} ETH", fee.priority_fee as f64 / 1e18);
                println!("   Total fee: {} ETH", fee.total_fee as f64 / 1e18);
                println!("   Fee breakdown:");
                println!("     - Operators: {} ETH", fee.fee_breakdown.operator_reward as f64 / 1e18);
                println!("     - Protocol: {} ETH", fee.fee_breakdown.protocol_fee as f64 / 1e18);
                println!("     - Treasury: {} ETH", fee.fee_breakdown.treasury_allocation as f64 / 1e18);
                println!("     - Gas reimburse: {} ETH", fee.fee_breakdown.gas_reimbursement as f64 / 1e18);
            }
            Err(e) => {
                println!("❌ Fee calculation error: {}", e);
            }
        }
    }
    
    // Demo 5: RAG Document Usage Proof
    println!("\n\n📄 Demo 5: RAG Document Usage Verification\n");
    
    let document_hash = [0x42; 32];
    println!("Proving usage of document: {}", hex::encode(&document_hash));
    
    match lagrange.historical_processor.prove_document_usage(
        intent_id.clone(),
        document_hash,
        12345678, // Block number
    ).await {
        Ok(proof) => {
            println!("✅ Document usage proof generated!");
            println!("   Document was in RAG database: ✓");
            println!("   Merkle proof depth: {}", proof.inclusion_proof.siblings.len());
            
            if let Some(usage) = &proof.usage_proof {
                println!("   Document was in top-{} results", usage.top_k_results.len());
                println!("   Similarity score: {:.3}", usage.similarity_scores[0]);
            }
        }
        Err(e) => {
            println!("❌ Document proof error: {}", e);
        }
    }
    
    println!("\n\n✨ Enhanced Lagrange Demo Complete!");
    println!("\nKey Features Demonstrated:");
    println!("  • Time-aware historical memory queries");
    println!("  • Cross-chain intent verification with state committees");
    println!("  • Agent reputation metrics aggregation");
    println!("  • Dynamic fee calculation with economic model");
    println!("  • RAG document usage proofs");
    println!("\nThis integration enables fully auditable, cross-chain verifiable AI agent operations!");
    
    Ok(())
}