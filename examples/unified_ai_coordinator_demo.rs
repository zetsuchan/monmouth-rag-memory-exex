use alloy::primitives::{Address, B256, U256};
use alloy::providers::ProviderBuilder;
use eyre::Result;
use monmouth_rag_memory_exex::{
    context::{
        preprocessing::{ContextPreprocessor, ProcessingConfig},
    },
    memory_exex::{
        agent_integration::AgentMemoryIntegration,
        memory_store::MemoryStore,
    },
    rag_exex::{
        agent_context::UnifiedContextManager,
        context_retrieval::ContextRetriever,
    },
    shared::{
        agent_standard::{AgentCapability, AgentRegistry},
        agent_state_manager::{AgentStateManager, LifecycleState, StateEvent, StateManagerConfig},
        ai_agent::{AIAgent, RoutingDecision},
        ai_agent_v2::{EnhancedAIDecisionEngine, EnsembleConfig},
        communication::{CrossExExCoordinator, CrossExExMessage},
        coordination::{BLSCoordinator, CoordinationMessage},
        metrics_v2::{EnhancedMetrics, PerformanceReport},
        types::{AgentAction, AgentContext},
        unified_coordinator::{CoordinationConfig, CoordinationEvent, UnifiedCoordinator},
    },
};
use reth_primitives::{TransactionSigned, TxEip1559, TxKind};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Unified AI Coordinator Demo");

    // Initialize components
    let components = initialize_components().await?;
    
    // Start monitoring tasks
    let monitoring_handle = tokio::spawn(monitor_system(
        components.state_event_rx,
        components.coord_event_rx,
        components.metrics.clone(),
    ));

    // Register test agents
    info!("Registering test agents...");
    register_test_agents(&components).await?;

    // Simulate transaction processing
    info!("Starting transaction simulation...");
    simulate_transactions(&components).await?;

    // Wait for monitoring to complete
    monitoring_handle.await?;

    Ok(())
}

struct SystemComponents {
    coordinator: Arc<UnifiedCoordinator>,
    state_manager: Arc<AgentStateManager>,
    ai_engine: Arc<EnhancedAIDecisionEngine>,
    metrics: Arc<EnhancedMetrics>,
    agent_registry: Arc<AgentRegistry>,
    state_event_rx: mpsc::Receiver<StateEvent>,
    coord_event_rx: mpsc::Receiver<CoordinationEvent>,
}

async fn initialize_components() -> Result<SystemComponents> {
    info!("Initializing system components...");

    // Initialize metrics
    let metrics = Arc::new(EnhancedMetrics::new()?);

    // Initialize AI decision engine with ensemble configuration
    let ensemble_config = EnsembleConfig::default();
    let ai_engine = Arc::new(EnhancedAIDecisionEngine::new(ensemble_config)?);

    // Initialize agent state manager
    let state_config = StateManagerConfig::default();
    let (state_manager, state_event_rx) = AgentStateManager::new(state_config);
    let state_manager = Arc::new(state_manager);

    // Initialize agent registry
    let agent_registry = Arc::new(AgentRegistry::new());

    // Initialize BLS coordinator
    let bls_coordinator = Arc::new(BLSCoordinator::new().await?);

    // Initialize cross-ExEx coordinator
    let cross_exex_coordinator = Arc::new(CrossExExCoordinator::new());

    // Initialize context components
    let context_preprocessor = Arc::new(ContextPreprocessor::new(ProcessingConfig::default()));
    let memory_store = Arc::new(MemoryStore::new(Default::default()));
    let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
    
    let (memory_integration, _) = AgentMemoryIntegration::new(
        Default::default(),
        memory_store,
        context_retriever,
        context_preprocessor.clone(),
    );
    let memory_integration = Arc::new(memory_integration);
    
    let context_manager = Arc::new(UnifiedContextManager::new(
        memory_integration.clone(),
        Default::default(),
    ));

    // Initialize unified coordinator
    let coord_config = CoordinationConfig::default();
    let (coordinator, coord_event_rx) = UnifiedCoordinator::new(
        coord_config,
        ai_engine.clone(),
        state_manager.clone(),
        agent_registry.clone(),
        bls_coordinator,
        cross_exex_coordinator,
        context_preprocessor,
        context_manager,
        memory_integration,
    ).await;
    let coordinator = Arc::new(coordinator);

    info!("All components initialized successfully");

    Ok(SystemComponents {
        coordinator,
        state_manager,
        ai_engine,
        metrics,
        agent_registry,
        state_event_rx,
        coord_event_rx,
    })
}

async fn register_test_agents(components: &SystemComponents) -> Result<()> {
    // Register specialized agents
    let agents = vec![
        (
            Address::from([1u8; 20]),
            vec![AgentCapability::RAGQuery, AgentCapability::ComplexExecution],
            "rag_specialist",
        ),
        (
            Address::from([2u8; 20]),
            vec![AgentCapability::MemoryManagement, AgentCapability::StateTracking],
            "memory_specialist",
        ),
        (
            Address::from([3u8; 20]),
            vec![AgentCapability::TransactionRouting, AgentCapability::Consensus],
            "routing_specialist",
        ),
        (
            Address::from([4u8; 20]),
            vec![AgentCapability::SecurityAnalysis, AgentCapability::Validation],
            "security_specialist",
        ),
    ];

    for (address, capabilities, name) in agents {
        info!("Registering agent: {} ({:?})", name, address);
        
        // Register with state manager
        components.state_manager.register_agent(address, capabilities.clone()).await?;
        
        // Register with agent registry
        components.agent_registry.register_agent(address, capabilities, 50).await?;
        
        // Transition to active state
        components.state_manager.transition_state(
            address,
            LifecycleState::Active,
            "Initialization complete".to_string(),
        ).await?;
        
        // Add to AI engine context
        let agent_context = AgentContext {
            agent_address: address,
            last_action: AgentAction::Transfer {
                to: Address::default(),
                amount: U256::ZERO,
            },
            reputation_score: 50,
            total_interactions: 0,
            success_rate: 0.0,
            specialization: vec![name.to_string()],
        };
        
        // Simulate some initial context
        components.ai_engine.add_agent_context(
            address,
            create_test_context(address),
        ).await;
    }

    info!("All agents registered and activated");
    Ok(())
}

async fn simulate_transactions(components: &SystemComponents) -> Result<()> {
    let mut interval = interval(Duration::from_millis(500));
    let mut tx_count = 0;

    loop {
        interval.tick().await;
        
        if tx_count >= 50 {
            info!("Completed {} transactions", tx_count);
            break;
        }

        // Create different types of transactions
        let tx = match tx_count % 5 {
            0 => create_complex_contract_tx(),
            1 => create_token_transfer_tx(),
            2 => create_swap_tx(),
            3 => create_high_value_tx(),
            _ => create_simple_tx(),
        };

        let tx_hash = tx.hash();
        info!("Processing transaction {} (type: {})", 
            tx_count, 
            match tx_count % 5 {
                0 => "complex_contract",
                1 => "token_transfer",
                2 => "swap",
                3 => "high_value",
                _ => "simple",
            }
        );

        // Track transaction
        components.metrics.start_transaction_tracking(tx_hash).await;

        // Process through coordinator
        let routing_start = std::time::Instant::now();
        if let Err(e) = components.coordinator.process_transaction(tx).await {
            error!("Failed to process transaction: {}", e);
        } else {
            components.metrics.record_routing_latency(
                tx_hash,
                routing_start.elapsed(),
            ).await;
        }

        // Simulate agent heartbeats
        if tx_count % 10 == 0 {
            update_agent_heartbeats(components).await?;
        }

        // Simulate consensus rounds
        if tx_count % 15 == 0 {
            simulate_consensus(components).await?;
        }

        tx_count += 1;
    }

    // Generate final report
    generate_performance_report(components).await?;

    Ok(())
}

async fn monitor_system(
    mut state_rx: mpsc::Receiver<StateEvent>,
    mut coord_rx: mpsc::Receiver<CoordinationEvent>,
    metrics: Arc<EnhancedMetrics>,
) -> Result<()> {
    let mut monitoring_interval = interval(Duration::from_secs(5));
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(state_event) = state_rx.recv() => {
                match state_event {
                    StateEvent::AgentRegistered { address } => {
                        info!("Agent registered: {:?}", address);
                    }
                    StateEvent::StateChanged { address, old_state, new_state } => {
                        info!("Agent {:?} state changed: {:?} -> {:?}", address, old_state, new_state);
                        metrics.record_agent_state_transition(address, &old_state, &new_state).await;
                    }
                    StateEvent::HealthChanged { address, healthy } => {
                        if !healthy {
                            warn!("Agent {:?} became unhealthy", address);
                        }
                    }
                    _ => {}
                }
            }
            Some(coord_event) = coord_rx.recv() => {
                match coord_event {
                    CoordinationEvent::TransactionRouted { hash, decision, agent } => {
                        info!("Transaction {:?} routed: {:?} -> {:?}", hash, decision, agent);
                        metrics.record_ai_decision("ensemble", &decision, 0.85).await;
                    }
                    CoordinationEvent::ExecutionCompleted { hash, success, duration_ms } => {
                        info!("Execution completed: {:?} success={} duration={}ms", hash, success, duration_ms);
                        metrics.record_execution_latency(
                            hash,
                            "unified",
                            Duration::from_millis(duration_ms),
                        ).await;
                    }
                    CoordinationEvent::BatchProcessed { count, duration_ms } => {
                        info!("Batch processed: {} transactions in {}ms", count, duration_ms);
                    }
                    _ => {}
                }
            }
            _ = monitoring_interval.tick() => {
                let overview = metrics.get_system_overview().await;
                info!("System Overview: {:?}", overview);
                
                if start_time.elapsed() > Duration::from_secs(60) {
                    info!("Monitoring complete");
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn update_agent_heartbeats(components: &SystemComponents) -> Result<()> {
    let agents = components.state_manager.get_active_agents().await;
    
    for agent in agents {
        components.state_manager.update_heartbeat(agent).await?;
        
        // Update resource metrics
        let cpu_usage = rand::random::<f64>() * 50.0 + 20.0;
        let memory_mb = rand::random::<u64>() % 500 + 100;
        let queue_size = rand::random::<usize>() % 20;
        
        components.metrics.update_agent_resources(
            agent,
            cpu_usage,
            memory_mb,
            queue_size,
        ).await;
    }
    
    Ok(())
}

async fn simulate_consensus(components: &SystemComponents) -> Result<()> {
    let round_id = format!("round_{}", chrono::Utc::now().timestamp());
    let participants = components.state_manager.get_active_agents().await;
    
    if participants.len() < 2 {
        return Ok(());
    }
    
    info!("Starting consensus round: {}", round_id);
    
    let message = CoordinationMessage::StateUpdate {
        updater: participants[0],
        new_state: B256::random(),
        proof: vec![],
    };
    
    let start = std::time::Instant::now();
    let success = components.coordinator.coordinate_consensus(message).await?;
    let duration = start.elapsed();
    
    components.metrics.record_consensus_round(
        round_id,
        participants,
        duration,
        success,
    ).await;
    
    info!("Consensus completed: success={} duration={:?}", success, duration);
    
    Ok(())
}

async fn generate_performance_report(components: &SystemComponents) -> Result<()> {
    let report = components.metrics.generate_performance_report().await;
    
    info!("=== Performance Report ===");
    info!("Timestamp: {}", report.timestamp);
    info!("Overview: {:?}", report.overview);
    
    info!("\nModel Performance:");
    for (model, perf) in &report.model_performance {
        info!("  {}: accuracy={:.2}% confidence={:.2} error={:.2}",
            model,
            perf.accuracy_rate * 100.0,
            perf.average_confidence,
            perf.average_error
        );
    }
    
    info!("\nAgent Utilization:");
    for (agent, util) in &report.agent_utilization {
        info!("  {}: cpu={:.1}% mem={}MB queue={} processed={}",
            agent,
            util.cpu_usage,
            util.memory_usage_mb,
            util.queue_depth,
            util.transactions_processed
        );
    }
    
    Ok(())
}

// Helper functions to create test transactions
fn create_complex_contract_tx() -> TransactionSigned {
    let mut data = vec![0x60, 0x80, 0x60, 0x40]; // Contract creation bytecode
    data.extend_from_slice(&[0u8; 200]); // Complex data
    
    TransactionSigned::from_transaction_and_signature(
        reth_primitives::Transaction::Eip1559(TxEip1559 {
            chain_id: 1,
            nonce: rand::random(),
            gas_limit: 3_000_000,
            max_fee_per_gas: 50_000_000_000,
            max_priority_fee_per_gas: 2_000_000_000,
            to: TxKind::Create,
            value: U256::ZERO,
            input: data.into(),
            access_list: Default::default(),
        }),
        reth_primitives::Signature::default(),
    )
}

fn create_token_transfer_tx() -> TransactionSigned {
    let mut data = vec![0xa9, 0x05, 0x9c, 0xbb]; // transfer(address,uint256)
    data.extend_from_slice(&[0u8; 64]); // Parameters
    
    TransactionSigned::from_transaction_and_signature(
        reth_primitives::Transaction::Eip1559(TxEip1559 {
            chain_id: 1,
            nonce: rand::random(),
            gas_limit: 65_000,
            max_fee_per_gas: 30_000_000_000,
            max_priority_fee_per_gas: 1_000_000_000,
            to: TxKind::Call(Address::random()),
            value: U256::ZERO,
            input: data.into(),
            access_list: Default::default(),
        }),
        reth_primitives::Signature::default(),
    )
}

fn create_swap_tx() -> TransactionSigned {
    let mut data = vec![0x38, 0xed, 0x17, 0x39]; // swapExactTokensForTokens
    data.extend_from_slice(&[0u8; 160]); // Swap parameters
    
    TransactionSigned::from_transaction_and_signature(
        reth_primitives::Transaction::Eip1559(TxEip1559 {
            chain_id: 1,
            nonce: rand::random(),
            gas_limit: 250_000,
            max_fee_per_gas: 40_000_000_000,
            max_priority_fee_per_gas: 1_500_000_000,
            to: TxKind::Call(Address::random()),
            value: U256::ZERO,
            input: data.into(),
            access_list: Default::default(),
        }),
        reth_primitives::Signature::default(),
    )
}

fn create_high_value_tx() -> TransactionSigned {
    TransactionSigned::from_transaction_and_signature(
        reth_primitives::Transaction::Eip1559(TxEip1559 {
            chain_id: 1,
            nonce: rand::random(),
            gas_limit: 21_000,
            max_fee_per_gas: 100_000_000_000,
            max_priority_fee_per_gas: 5_000_000_000,
            to: TxKind::Call(Address::random()),
            value: U256::from(50_000_000_000_000_000_000u128), // 50 ETH
            input: Default::default(),
            access_list: Default::default(),
        }),
        reth_primitives::Signature::default(),
    )
}

fn create_simple_tx() -> TransactionSigned {
    TransactionSigned::from_transaction_and_signature(
        reth_primitives::Transaction::Eip1559(TxEip1559 {
            chain_id: 1,
            nonce: rand::random(),
            gas_limit: 21_000,
            max_fee_per_gas: 20_000_000_000,
            max_priority_fee_per_gas: 1_000_000_000,
            to: TxKind::Call(Address::random()),
            value: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
            input: Default::default(),
            access_list: Default::default(),
        }),
        reth_primitives::Signature::default(),
    )
}

fn create_test_context(agent: Address) -> crate::context::preprocessing::PreprocessedContext {
    use crate::context::preprocessing::PreprocessedContext;
    
    PreprocessedContext {
        transaction_hash: B256::random(),
        agent_address: agent,
        action_type: AgentAction::Transfer {
            to: Address::random(),
            amount: U256::from(1000),
        },
        extracted_features: Default::default(),
        semantic_tags: vec!["test".to_string()],
        priority_score: 0.5,
        timestamp: chrono::Utc::now().timestamp() as u64,
        compressed_data: None,
    }
}