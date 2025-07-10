use alloy::primitives::{Address, B256, U256};
use eyre::Result;
use monmouth_rag_memory_exex::{
    agents::{
        checkpointing::{CheckpointManager, CheckpointTrigger, AgentCheckpointer},
        recovery::{RecoveryManager, RecoveryRequest, RecoveryStrategy, RecoveryPriority, AgentRecovery},
    },
    alh::coordination::{ALHCoordinator, ALHQueryRequest, ALHQueryType},
    context::preprocessing::{ContextPreprocessor, ProcessingConfig},
    enhanced_processor::{EnhancedExExProcessor, EnhancedProcessor, LoggingStateListener},
    memory_exex::{MemoryStore, MemoryType},
    rag_exex::{
        agent_context::UnifiedContextManager,
        context_retrieval::ContextRetriever,
    },
    reorg::{
        coordination::{ReorgCoordinator, ReorgEvent, ReorgSeverity},
        shared_state::SharedState,
    },
    shared::{
        agent_state_manager::{AgentStateManager, LifecycleState},
        communication::CrossExExCoordinator,
        types::AgentAction,
    },
};
use reth_primitives::{TransactionSigned, TxEip1559, TxKind};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting State Synchronization Demo");

    let components = initialize_components().await?;
    
    run_demo(&components).await?;

    Ok(())
}

struct DemoComponents {
    alh_coordinator: Arc<ALHCoordinator>,
    reorg_coordinator: Arc<ReorgCoordinator>,
    checkpoint_manager: Arc<CheckpointManager>,
    recovery_manager: Arc<RecoveryManager>,
    enhanced_processor: Arc<EnhancedExExProcessor>,
    shared_state: Arc<SharedState>,
    memory_store: Arc<MemoryStore>,
    agent_state_manager: Arc<AgentStateManager>,
}

async fn initialize_components() -> Result<DemoComponents> {
    info!("Initializing demo components...");

    let memory_store = Arc::new(MemoryStore::new(Default::default()));
    let context_retriever = Arc::new(ContextRetriever::new(Default::default()));
    let context_preprocessor = Arc::new(ContextPreprocessor::new(ProcessingConfig::default()));
    let shared_state = Arc::new(SharedState::new());
    let cross_exex_coordinator = Arc::new(CrossExExCoordinator::new());
    
    let (agent_state_manager, _) = AgentStateManager::new(Default::default());
    let agent_state_manager = Arc::new(agent_state_manager);

    let (alh_coordinator, _) = ALHCoordinator::new(
        memory_store.clone(),
        context_retriever.clone(),
        Default::default(),
    );
    let alh_coordinator = Arc::new(alh_coordinator);

    let (reorg_coordinator, _, _) = ReorgCoordinator::new(
        shared_state.clone(),
        alh_coordinator.clone(),
        agent_state_manager.clone(),
        cross_exex_coordinator.clone(),
        memory_store.clone(),
        Default::default(),
    );
    let reorg_coordinator = Arc::new(reorg_coordinator);

    let context_manager = Arc::new(UnifiedContextManager::new(
        Default::default(),
        Default::default(),
    ));

    let (checkpoint_manager, _) = CheckpointManager::new(
        alh_coordinator.clone(),
        memory_store.clone(),
        agent_state_manager.clone(),
        context_manager.clone(),
        Default::default(),
    );
    let checkpoint_manager = Arc::new(checkpoint_manager);

    let (recovery_manager, _) = RecoveryManager::new(
        checkpoint_manager.clone(),
        alh_coordinator.clone(),
        memory_store.clone(),
        agent_state_manager.clone(),
        context_manager.clone(),
        Default::default(),
    );
    let recovery_manager = Arc::new(recovery_manager);

    let unified_coordinator = Arc::new(
        monmouth_rag_memory_exex::shared::unified_coordinator::UnifiedCoordinator::new(
            Default::default(),
            Default::default(),
            agent_state_manager.clone(),
            Arc::new(monmouth_rag_memory_exex::shared::agent_standard::AgentRegistry::new()),
            Arc::new(monmouth_rag_memory_exex::shared::coordination::BLSCoordinator::new().await?),
            cross_exex_coordinator,
            context_preprocessor,
            context_manager.clone(),
            Arc::new(monmouth_rag_memory_exex::memory_exex::agent_integration::AgentMemoryIntegration::new(
                Default::default(),
                memory_store.clone(),
                context_retriever,
                Arc::new(ContextPreprocessor::new(ProcessingConfig::default())),
            ).0),
        ).await.0
    );

    let enhanced_processor = Arc::new(EnhancedExExProcessor::new(
        alh_coordinator.clone(),
        unified_coordinator,
        memory_store.clone(),
        Arc::new(ContextRetriever::new(Default::default())),
    ));

    enhanced_processor.register_state_listener(Box::new(LoggingStateListener)).await;

    info!("All components initialized successfully");

    Ok(DemoComponents {
        alh_coordinator,
        reorg_coordinator,
        checkpoint_manager,
        recovery_manager,
        enhanced_processor,
        shared_state,
        memory_store,
        agent_state_manager,
    })
}

async fn run_demo(components: &DemoComponents) -> Result<()> {
    info!("=== Starting State Synchronization Demo ===");

    demo_1_basic_alh_queries(components).await?;
    demo_2_checkpoint_and_recovery(components).await?;
    demo_3_reorg_handling(components).await?;
    demo_4_cross_exex_coordination(components).await?;
    demo_5_state_synchronization(components).await?;

    info!("=== Demo completed successfully ===");
    Ok(())
}

async fn demo_1_basic_alh_queries(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 1: Basic ALH Queries ===");

    let test_agents = vec![
        Address::from([1u8; 20]),
        Address::from([2u8; 20]),
        Address::from([3u8; 20]),
    ];

    for agent in &test_agents {
        components.agent_state_manager.register_agent(
            *agent,
            vec![monmouth_rag_memory_exex::shared::agent_standard::AgentCapability::MemoryManagement],
        ).await?;
        
        components.agent_state_manager.transition_state(
            *agent,
            LifecycleState::Active,
            "Demo initialization".to_string(),
        ).await?;
    }

    for agent in &test_agents {
        let request = ALHQueryRequest {
            query_id: B256::random(),
            requester: Address::random(),
            query_type: ALHQueryType::AgentMemoryState {
                agent_address: *agent,
                memory_types: vec![MemoryType::Working, MemoryType::Episodic],
                include_proofs: false,
            },
            timestamp: chrono::Utc::now().timestamp() as u64,
            priority: 1,
            metadata: None,
        };

        let response = components.alh_coordinator.process_query(request).await?;
        info!("ALH Query Response for agent {:?}: execution_time={}ms state_hash={:?}", 
            agent, response.execution_time_ms, response.state_hash);
    }

    info!("Demo 1 completed successfully");
    Ok(())
}

async fn demo_2_checkpoint_and_recovery(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 2: Checkpoint and Recovery ===");

    let test_agent = Address::from([10u8; 20]);
    
    components.agent_state_manager.register_agent(
        test_agent,
        vec![monmouth_rag_memory_exex::shared::agent_standard::AgentCapability::MemoryManagement],
    ).await?;

    let memory_data = monmouth_rag_memory_exex::memory_exex::AgentMemory {
        agent_address: test_agent,
        memory_type: MemoryType::Working,
        content: b"Test memory content".to_vec(),
        content_hash: B256::random(),
        timestamp: chrono::Utc::now().timestamp() as u64,
        metadata: serde_json::json!({"demo": "checkpoint_test"}),
    };

    components.memory_store.store_memory(memory_data).await?;

    let checkpoint_id = components.checkpoint_manager
        .create_checkpoint(test_agent, CheckpointTrigger::Manual)
        .await?;

    info!("Created checkpoint: {:?}", checkpoint_id);

    let is_valid = components.checkpoint_manager
        .validate_checkpoint(checkpoint_id)
        .await?;

    info!("Checkpoint validation: {}", is_valid);

    let recovery_request = RecoveryRequest {
        recovery_id: B256::random(),
        agent_address: test_agent,
        strategy: RecoveryStrategy::FullRestore {
            checkpoint_id,
            validate_integrity: true,
        },
        priority: RecoveryPriority::High,
        requester: Address::random(),
        metadata: None,
    };

    let recovery_result = components.recovery_manager
        .recover_agent(recovery_request)
        .await?;

    info!("Recovery result: success={} time={}ms", 
        recovery_result.success, recovery_result.recovery_time_ms);

    info!("Demo 2 completed successfully");
    Ok(())
}

async fn demo_3_reorg_handling(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 3: Reorg Handling ===");

    components.shared_state.update_state(
        100,
        B256::random(),
        B256::random(),
        vec![Address::from([20u8; 20])],
        vec![B256::random()],
    ).await?;

    let new_block_hash = B256::random();
    let reorg_event = components.reorg_coordinator
        .detect_reorg(98, new_block_hash)
        .await?;

    if let Some(event) = reorg_event {
        info!("Reorg detected: from_block={} to_block={} severity={:?}", 
            event.from_block, event.to_block, event.severity);

        match components.reorg_coordinator.handle_reorg(event).await {
            Ok(_) => info!("Reorg handled successfully"),
            Err(e) => warn!("Reorg handling failed: {}", e),
        }
    } else {
        info!("No reorg detected (creating synthetic reorg event)");
        
        let synthetic_event = ReorgEvent {
            event_id: B256::random(),
            from_block: 100,
            to_block: 98,
            common_ancestor: 97,
            affected_agents: vec![Address::from([20u8; 20])],
            affected_transactions: vec![B256::random()],
            timestamp: chrono::Utc::now().timestamp() as u64,
            severity: ReorgSeverity::Minor { depth: 3 },
        };

        components.reorg_coordinator.handle_reorg(synthetic_event).await?;
    }

    let snapshot_id = components.reorg_coordinator.create_snapshot(100).await?;
    info!("Created snapshot: {:?}", snapshot_id);

    info!("Demo 3 completed successfully");
    Ok(())
}

async fn demo_4_cross_exex_coordination(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 4: Cross-ExEx Coordination ===");

    let test_agents = vec![
        Address::from([30u8; 20]),
        Address::from([31u8; 20]),
        Address::from([32u8; 20]),
    ];

    for agent in &test_agents {
        components.agent_state_manager.register_agent(
            *agent,
            vec![monmouth_rag_memory_exex::shared::agent_standard::AgentCapability::MemoryManagement],
        ).await?;
    }

    let sync_request = ALHQueryRequest {
        query_id: B256::random(),
        requester: Address::random(),
        query_type: ALHQueryType::CrossAgentSync {
            agents: test_agents.clone(),
            sync_type: monmouth_rag_memory_exex::alh::coordination::SyncType::Full,
        },
        timestamp: chrono::Utc::now().timestamp() as u64,
        priority: 1,
        metadata: None,
    };

    let sync_response = components.alh_coordinator.process_query(sync_request).await?;
    
    match sync_response.result {
        monmouth_rag_memory_exex::alh::coordination::ALHQueryResult::SyncResult { 
            synced_agents, 
            sync_hashes, 
            conflicts 
        } => {
            info!("Cross-agent sync completed: {} agents, {} hashes, {} conflicts",
                synced_agents.len(), sync_hashes.len(), conflicts.len());
        }
        _ => warn!("Unexpected sync response type"),
    }

    info!("Demo 4 completed successfully");
    Ok(())
}

async fn demo_5_state_synchronization(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 5: Full State Synchronization ===");

    let test_agent = Address::from([40u8; 20]);
    
    components.agent_state_manager.register_agent(
        test_agent,
        vec![monmouth_rag_memory_exex::shared::agent_standard::AgentCapability::MemoryManagement],
    ).await?;

    let transactions = vec![
        create_test_transaction(test_agent, 1),
        create_test_transaction(test_agent, 2),
        create_test_transaction(test_agent, 3),
    ];

    for tx in &transactions {
        if let Err(e) = components.enhanced_processor.process_with_alh_hooks(tx).await {
            warn!("Transaction processing failed: {}", e);
        }
    }

    let checkpoint_id = components.checkpoint_manager
        .create_checkpoint(test_agent, CheckpointTrigger::Manual)
        .await?;

    info!("Created checkpoint after transaction processing: {:?}", checkpoint_id);

    let synthetic_reorg = ReorgEvent {
        event_id: B256::random(),
        from_block: 105,
        to_block: 102,
        common_ancestor: 101,
        affected_agents: vec![test_agent],
        affected_transactions: transactions.iter().map(|tx| tx.hash()).collect(),
        timestamp: chrono::Utc::now().timestamp() as u64,
        severity: ReorgSeverity::Major { depth: 4, state_conflicts: 2 },
    };

    components.reorg_coordinator.handle_reorg(synthetic_reorg).await?;

    let recovery_request = RecoveryRequest {
        recovery_id: B256::random(),
        agent_address: test_agent,
        strategy: RecoveryStrategy::FullRestore {
            checkpoint_id,
            validate_integrity: true,
        },
        priority: RecoveryPriority::Critical,
        requester: Address::random(),
        metadata: Some(serde_json::json!({
            "reason": "post_reorg_recovery",
            "demo": "state_sync"
        })),
    };

    let recovery_result = components.recovery_manager
        .recover_agent(recovery_request)
        .await?;

    info!("Post-reorg recovery: success={} components={:?}", 
        recovery_result.success, recovery_result.restored_components);

    let final_checkpoint = components.enhanced_processor
        .checkpoint_agent_state(test_agent)
        .await?;

    info!("Final checkpoint created: {:?}", final_checkpoint);

    info!("Demo 5 completed successfully");
    Ok(())
}

fn create_test_transaction(agent: Address, nonce: u64) -> TransactionSigned {
    let mut calldata = vec![0xa9, 0x05, 0x9c, 0xbb];
    calldata.extend_from_slice(&[0u8; 32]);
    calldata.extend_from_slice(&agent.as_slice());
    calldata.extend_from_slice(&U256::from(1000 * nonce).to_be_bytes::<32>());

    TransactionSigned::from_transaction_and_signature(
        reth_primitives::Transaction::Eip1559(TxEip1559 {
            chain_id: 1,
            nonce,
            gas_limit: 65_000,
            max_fee_per_gas: 30_000_000_000,
            max_priority_fee_per_gas: 1_000_000_000,
            to: TxKind::Call(Address::random()),
            value: U256::ZERO,
            input: calldata.into(),
            access_list: Default::default(),
        }),
        reth_primitives::Signature::default(),
    )
}

async fn run_monitoring_task(components: &DemoComponents) -> Result<()> {
    let mut interval = interval(Duration::from_secs(10));
    
    loop {
        interval.tick().await;
        
        let active_agents = components.agent_state_manager.get_active_agents().await;
        let current_state = components.shared_state.get_current_state().await?;
        
        info!("System status: {} active agents, block {}", 
            active_agents.len(), current_state.block_number);
        
        for agent in active_agents.iter().take(3) {
            let checkpoints = components.checkpoint_manager.list_checkpoints(*agent).await?;
            info!("Agent {:?}: {} checkpoints", agent, checkpoints.len());
        }
        
        if chrono::Utc::now().timestamp() % 30 == 0 {
            break;
        }
    }
    
    Ok(())
}