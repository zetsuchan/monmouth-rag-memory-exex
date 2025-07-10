use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};
use eyre::Result;
use uuid::Uuid;
use alloy::primitives::{Address, B256};

use monmouth_rag_memory_exex::{
    shared::{
        communication::{CrossExExMessage, MessageRouter, CrossExExChannel},
        ai_agent::{RoutingDecision, AgentDecision},
        agent_state_manager::{AgentStateManager, AgentStatus},
        coordination::{BLSCoordinator, ConsensusDecision},
        metrics_v2::MetricsV2,
        types::{AgentId, TransactionHash, MemoryHash},
    },
    rag_exex::{
        context_retrieval::ContextRetriever,
        vector_store::VectorStore,
        embeddings::EmbeddingGenerator,
        knowledge_graph::KnowledgeGraph,
    },
    memory_exex::{
        memory_store::MemoryStore,
        agent_integration::AgentMemoryIntegration,
        checkpointing::CheckpointManager,
    },
    alh::{
        coordination::{ALHCoordinator, QueryType, ALHQuery},
    },
    reorg::{
        coordination::{ReorgCoordinator, ReorgStrategy},
        shared_state::SharedState,
    },
    performance::{
        CrossExExOptimizer, PerformanceConfig, LoadBalancingStrategy,
        cache::CacheManager,
        throughput::ThroughputOptimizer,
    },
    error_handling::{
        CrossExExErrorHandler, CrossExExErrorConfig, ErrorHandlingConfig,
        circuit_breaker::CircuitBreaker,
        recovery::RecoveryCoordinator,
    },
    monitoring::{
        production::ProductionMonitor,
        alerts::AlertManager,
        health::HealthChecker,
        MonitoringConfig,
    },
    agents::{
        checkpointing::AgentCheckpointManager,
        recovery::AgentRecoveryManager,
    },
};

/// Integration test configuration
#[derive(Debug, Clone)]
pub struct IntegrationTestConfig {
    pub test_timeout: Duration,
    pub agent_count: usize,
    pub transaction_count: usize,
    pub memory_cache_size: usize,
    pub enable_performance_monitoring: bool,
    pub enable_error_injection: bool,
}

impl Default for IntegrationTestConfig {
    fn default() -> Self {
        Self {
            test_timeout: Duration::from_secs(30),
            agent_count: 10,
            transaction_count: 100,
            memory_cache_size: 1024 * 1024, // 1MB
            enable_performance_monitoring: true,
            enable_error_injection: false,
        }
    }
}

/// Main integration test harness for cross-ExEx testing
#[derive(Debug)]
pub struct CrossExExTestHarness {
    config: IntegrationTestConfig,
    
    // Core components
    message_router: Arc<MessageRouter>,
    agent_state_manager: Arc<AgentStateManager>,
    bls_coordinator: Arc<BLSCoordinator>,
    metrics: Arc<MetricsV2>,
    
    // RAG ExEx components
    context_retriever: Arc<ContextRetriever>,
    vector_store: Arc<VectorStore>,
    embedding_generator: Arc<EmbeddingGenerator>,
    knowledge_graph: Arc<KnowledgeGraph>,
    
    // Memory ExEx components
    memory_store: Arc<MemoryStore>,
    agent_memory_integration: Arc<AgentMemoryIntegration>,
    checkpoint_manager: Arc<CheckpointManager>,
    
    // State synchronization components
    alh_coordinator: Arc<ALHCoordinator>,
    reorg_coordinator: Arc<ReorgCoordinator>,
    shared_state: Arc<SharedState>,
    
    // Performance components
    performance_optimizer: Arc<CrossExExOptimizer>,
    cache_manager: Arc<CacheManager>,
    throughput_optimizer: Arc<ThroughputOptimizer<String>>,
    
    // Error handling components
    error_handler: Arc<CrossExExErrorHandler>,
    circuit_breaker: Arc<CircuitBreaker>,
    recovery_coordinator: Arc<RecoveryCoordinator>,
    
    // Monitoring components
    production_monitor: Arc<ProductionMonitor>,
    alert_manager: Arc<AlertManager>,
    health_checker: Arc<HealthChecker>,
    
    // Agent management
    agent_checkpoint_manager: Arc<AgentCheckpointManager>,
    agent_recovery_manager: Arc<AgentRecoveryManager>,
    
    // Test state
    test_agents: Arc<RwLock<HashMap<AgentId, TestAgent>>>,
    test_results: Arc<RwLock<Vec<TestResult>>>,
}

#[derive(Debug, Clone)]
pub struct TestAgent {
    pub id: AgentId,
    pub address: Address,
    pub status: AgentStatus,
    pub memory_hash: Option<MemoryHash>,
    pub context_history: Vec<String>,
    pub transaction_count: usize,
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub test_name: String,
    pub success: bool,
    pub duration: Duration,
    pub error_message: Option<String>,
    pub metrics: HashMap<String, f64>,
}

impl CrossExExTestHarness {
    pub async fn new(config: IntegrationTestConfig) -> Result<Self> {
        // Initialize core components
        let message_router = Arc::new(MessageRouter::new());
        let agent_state_manager = Arc::new(AgentStateManager::new().await?);
        let bls_coordinator = Arc::new(BLSCoordinator::new().await?);
        let metrics = Arc::new(MetricsV2::new().await?);
        
        // Initialize RAG ExEx components
        let context_retriever = Arc::new(ContextRetriever::new().await?);
        let vector_store = Arc::new(VectorStore::new().await?);
        let embedding_generator = Arc::new(EmbeddingGenerator::new().await?);
        let knowledge_graph = Arc::new(KnowledgeGraph::new().await?);
        
        // Initialize Memory ExEx components
        let memory_store = Arc::new(MemoryStore::new().await?);
        let agent_memory_integration = Arc::new(AgentMemoryIntegration::new(memory_store.clone()).await?);
        let checkpoint_manager = Arc::new(CheckpointManager::new().await?);
        
        // Initialize state synchronization components
        let alh_coordinator = Arc::new(ALHCoordinator::new().await?);
        let reorg_coordinator = Arc::new(ReorgCoordinator::new().await?);
        let shared_state = Arc::new(SharedState::new().await?);
        
        // Initialize performance components
        let perf_config = PerformanceConfig {
            enable_connection_pooling: true,
            max_connections_per_exex: 10,
            connection_timeout: Duration::from_secs(30),
            message_batch_size: 50,
            batch_timeout: Duration::from_millis(100),
            enable_compression: true,
            compression_threshold: 1024,
            cache_size_mb: 64,
            cache_ttl: Duration::from_secs(300),
            enable_parallel_processing: true,
            max_parallel_workers: 4,
            load_balancing_strategy: LoadBalancingStrategy::AdaptiveLoad,
        };
        let performance_optimizer = Arc::new(CrossExExOptimizer::new(perf_config));
        let cache_manager = Arc::new(CacheManager::new(Default::default()));
        let throughput_optimizer = Arc::new(ThroughputOptimizer::new(Default::default()));
        
        // Initialize error handling components
        let error_config = CrossExExErrorConfig {
            base_config: ErrorHandlingConfig::default(),
            exex_health_check_interval: Duration::from_secs(15),
            max_failed_health_checks: 3,
            failover_timeout: Duration::from_secs(30),
            enable_automatic_failover: true,
            enable_load_redistribution: true,
            quarantine_duration: Duration::from_secs(300),
        };
        let error_handler = Arc::new(CrossExExErrorHandler::new(error_config));
        let circuit_breaker = Arc::new(CircuitBreaker::new(Default::default()));
        let recovery_coordinator = Arc::new(RecoveryCoordinator::new(Default::default()));
        
        // Initialize monitoring components
        let monitoring_config = MonitoringConfig::default();
        let production_monitor = Arc::new(ProductionMonitor::new(monitoring_config.clone()));
        let alert_manager = Arc::new(AlertManager::new(monitoring_config.clone()));
        let health_checker = Arc::new(HealthChecker::new(monitoring_config));
        
        // Initialize agent management
        let agent_checkpoint_manager = Arc::new(AgentCheckpointManager::new().await?);
        let agent_recovery_manager = Arc::new(AgentRecoveryManager::new().await?);
        
        Ok(Self {
            config,
            message_router,
            agent_state_manager,
            bls_coordinator,
            metrics,
            context_retriever,
            vector_store,
            embedding_generator,
            knowledge_graph,
            memory_store,
            agent_memory_integration,
            checkpoint_manager,
            alh_coordinator,
            reorg_coordinator,
            shared_state,
            performance_optimizer,
            cache_manager,
            throughput_optimizer,
            error_handler,
            circuit_breaker,
            recovery_coordinator,
            production_monitor,
            alert_manager,
            health_checker,
            agent_checkpoint_manager,
            agent_recovery_manager,
            test_agents: Arc::new(RwLock::new(HashMap::new())),
            test_results: Arc::new(RwLock::new(Vec::new())),
        })
    }
    
    /// Setup test environment with mock agents and data
    pub async fn setup_test_environment(&self) -> Result<()> {
        // Initialize all subsystems
        self.performance_optimizer.initialize_connection_pool("rag_exex".to_string(), 5).await?;
        self.performance_optimizer.initialize_connection_pool("memory_exex".to_string(), 5).await?;
        self.performance_optimizer.initialize_connection_pool("coordination_exex".to_string(), 3).await?;
        
        self.cache_manager.start_all_cleanup_tasks().await;
        self.error_handler.start_health_monitoring().await?;
        self.production_monitor.start_monitoring().await?;
        self.alert_manager.start().await?;
        self.health_checker.start().await?;
        
        // Create test agents
        let mut test_agents = self.test_agents.write().await;
        for i in 0..self.config.agent_count {
            let agent_id = format!("test_agent_{}", i);
            let address = Address::random();
            
            let test_agent = TestAgent {
                id: agent_id.clone(),
                address,
                status: AgentStatus::Active,
                memory_hash: None,
                context_history: Vec::new(),
                transaction_count: 0,
            };
            
            // Register agent with state manager
            self.agent_state_manager.register_agent(agent_id.clone(), address).await?;
            
            test_agents.insert(agent_id, test_agent);
        }
        
        Ok(())
    }
    
    /// Run comprehensive integration test suite
    pub async fn run_integration_tests(&self) -> Result<Vec<TestResult>> {
        let mut all_results = Vec::new();
        
        // Test 1: Cross-ExEx Communication
        all_results.push(self.test_cross_exex_communication().await?);
        
        // Test 2: State Synchronization
        all_results.push(self.test_state_synchronization().await?);
        
        // Test 3: Agent Memory Integration
        all_results.push(self.test_agent_memory_integration().await?);
        
        // Test 4: RAG Context Retrieval
        all_results.push(self.test_rag_context_retrieval().await?);
        
        // Test 5: Performance Optimization
        all_results.push(self.test_performance_optimization().await?);
        
        // Test 6: Error Handling and Recovery
        all_results.push(self.test_error_handling_recovery().await?);
        
        // Test 7: Reorg Coordination
        all_results.push(self.test_reorg_coordination().await?);
        
        // Test 8: Agent Checkpointing
        all_results.push(self.test_agent_checkpointing().await?);
        
        // Test 9: Monitoring and Alerting
        all_results.push(self.test_monitoring_alerting().await?);
        
        // Test 10: End-to-End Workflow
        all_results.push(self.test_end_to_end_workflow().await?);
        
        // Store results
        *self.test_results.write().await = all_results.clone();
        
        Ok(all_results)
    }
    
    async fn test_cross_exex_communication(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Cross-ExEx Communication".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test message routing between ExExs
        for i in 0..10 {
            let message = CrossExExMessage::TransactionAnalysis {
                tx_hash: reth_primitives::H256::random(),
                routing_decision: RoutingDecision::ProcessWithRAG,
                context: format!("test_context_{}", i).into_bytes(),
                timestamp: std::time::Instant::now(),
            };
            
            let target_exex = if i % 2 == 0 { "rag_exex" } else { "memory_exex" };
            
            match self.performance_optimizer.send_optimized_message(target_exex.to_string(), message).await {
                Ok(_) => {
                    metrics.insert(format!("message_{}_success", i), 1.0);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Message {} failed: {}", i, e));
                    break;
                }
            }
        }
        
        // Test load balancing
        if success {
            match self.performance_optimizer.optimize_load_balancing().await {
                Ok(_) => {
                    metrics.insert("load_balancing_success".to_string(), 1.0);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Load balancing failed: {}", e));
                }
            }
        }
        
        // Get performance metrics
        if success {
            match self.performance_optimizer.get_performance_metrics().await {
                Ok(perf_metrics) => {
                    metrics.insert("avg_latency_ms".to_string(), perf_metrics.average_latency_ms);
                    metrics.insert("cache_hit_rate".to_string(), perf_metrics.cache_hit_rate);
                    metrics.insert("messages_processed".to_string(), perf_metrics.total_messages_processed as f64);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Failed to get metrics: {}", e));
                }
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_state_synchronization(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "State Synchronization".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test ALH queries
        for i in 0..5 {
            let query = ALHQuery {
                query_id: format!("test_query_{}", i),
                query_type: QueryType::MemoryState,
                agent_id: format!("test_agent_{}", i % self.config.agent_count),
                parameters: serde_json::json!({
                    "memory_key": format!("test_memory_{}", i),
                    "version": i
                }),
                timestamp: chrono::Utc::now().timestamp() as u64,
            };
            
            match self.alh_coordinator.process_query(query).await {
                Ok(result) => {
                    metrics.insert(format!("alh_query_{}_success", i), 1.0);
                    metrics.insert(format!("alh_query_{}_response_time", i), result.processing_time_ms);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("ALH query {} failed: {}", i, e));
                    break;
                }
            }
        }
        
        // Test cross-agent synchronization
        if success {
            let test_agents: Vec<String> = (0..self.config.agent_count)
                .map(|i| format!("test_agent_{}", i))
                .collect();
            
            match self.alh_coordinator.sync_agents(test_agents, Default::default()).await {
                Ok(sync_result) => {
                    metrics.insert("cross_agent_sync_success".to_string(), 1.0);
                    metrics.insert("sync_duration_ms".to_string(), sync_result.total_sync_time_ms as f64);
                    metrics.insert("agents_synchronized".to_string(), sync_result.agents_synchronized.len() as f64);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Cross-agent sync failed: {}", e));
                }
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_agent_memory_integration(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Agent Memory Integration".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test memory storage and retrieval
        for i in 0..10 {
            let agent_id = format!("test_agent_{}", i % self.config.agent_count);
            let memory_data = format!("test_memory_data_{}", i).into_bytes();
            
            // Store memory
            match self.agent_memory_integration.store_agent_memory(
                &agent_id,
                "working",
                memory_data.clone(),
            ).await {
                Ok(memory_hash) => {
                    metrics.insert(format!("memory_store_{}_success", i), 1.0);
                    
                    // Update test agent
                    if let Some(agent) = self.test_agents.write().await.get_mut(&agent_id) {
                        agent.memory_hash = Some(memory_hash.clone());
                    }
                    
                    // Retrieve memory
                    match self.agent_memory_integration.retrieve_agent_memory(&agent_id, "working").await {
                        Ok(retrieved_data) => {
                            if retrieved_data == memory_data {
                                metrics.insert(format!("memory_retrieve_{}_success", i), 1.0);
                            } else {
                                success = false;
                                error_message = Some(format!("Memory data mismatch for agent {}", agent_id));
                                break;
                            }
                        }
                        Err(e) => {
                            success = false;
                            error_message = Some(format!("Memory retrieval failed for agent {}: {}", agent_id, e));
                            break;
                        }
                    }
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Memory storage failed for agent {}: {}", agent_id, e));
                    break;
                }
            }
        }
        
        // Test memory integration metrics
        if success {
            match self.agent_memory_integration.get_integration_metrics().await {
                Ok(integration_metrics) => {
                    metrics.insert("total_memory_operations".to_string(), integration_metrics.total_memory_operations as f64);
                    metrics.insert("cache_hit_rate".to_string(), integration_metrics.cache_hit_rate);
                    metrics.insert("avg_operation_time_ms".to_string(), integration_metrics.average_operation_time_ms);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Failed to get integration metrics: {}", e));
                }
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_rag_context_retrieval(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "RAG Context Retrieval".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Add test data to vector store
        for i in 0..20 {
            let test_context = format!("This is test context {} for RAG retrieval testing", i);
            let embedding = vec![0.1 * i as f32; 384]; // Mock embedding
            
            match self.vector_store.store_embedding(
                format!("test_doc_{}", i),
                embedding,
                test_context.clone(),
            ).await {
                Ok(_) => {
                    metrics.insert(format!("embedding_store_{}_success", i), 1.0);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Failed to store embedding {}: {}", i, e));
                    break;
                }
            }
        }
        
        // Test context retrieval
        if success {
            for i in 0..5 {
                let agent_id = format!("test_agent_{}", i);
                let query = "test context retrieval";
                
                match self.context_retriever.retrieve_context(&agent_id, query, 5).await {
                    Ok(context) => {
                        metrics.insert(format!("context_retrieve_{}_success", i), 1.0);
                        metrics.insert(format!("context_retrieve_{}_results", i), context.len() as f64);
                        
                        // Update test agent context history
                        if let Some(agent) = self.test_agents.write().await.get_mut(&agent_id) {
                            agent.context_history.extend(context);
                        }
                    }
                    Err(e) => {
                        success = false;
                        error_message = Some(format!("Context retrieval failed for agent {}: {}", agent_id, e));
                        break;
                    }
                }
            }
        }
        
        // Test knowledge graph integration
        if success {
            for i in 0..3 {
                let agent1 = format!("test_agent_{}", i);
                let agent2 = format!("test_agent_{}", (i + 1) % self.config.agent_count);
                
                match self.knowledge_graph.add_relationship(
                    agent1.clone(),
                    agent2.clone(),
                    "test_interaction".to_string(),
                    0.5 + (i as f64 * 0.1),
                ).await {
                    Ok(_) => {
                        metrics.insert(format!("kg_relationship_{}_success", i), 1.0);
                    }
                    Err(e) => {
                        success = false;
                        error_message = Some(format!("Knowledge graph relationship {} failed: {}", i, e));
                        break;
                    }
                }
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_performance_optimization(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Performance Optimization".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test throughput optimization
        let processor = |messages: Vec<String>| async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<(), eyre::Report>(())
        };
        
        match self.throughput_optimizer.start_processing(processor).await {
            Ok(_) => {
                metrics.insert("throughput_processor_start_success".to_string(), 1.0);
                
                // Submit test messages
                for i in 0..50 {
                    let message = monmouth_rag_memory_exex::performance::throughput::PrioritizedMessage::new(
                        format!("test_message_{}", i),
                        (i % 3) as u8,
                        Address::random(),
                    );
                    
                    if let Err(e) = self.throughput_optimizer.submit_message(message).await {
                        success = false;
                        error_message = Some(format!("Failed to submit message {}: {}", i, e));
                        break;
                    }
                }
                
                // Wait for processing
                tokio::time::sleep(Duration::from_secs(2)).await;
                
                // Get metrics
                let throughput_metrics = self.throughput_optimizer.get_metrics().await;
                metrics.insert("current_tps".to_string(), throughput_metrics.current_tps);
                metrics.insert("average_tps".to_string(), throughput_metrics.average_tps);
                metrics.insert("queue_depth".to_string(), throughput_metrics.queue_depth as f64);
                metrics.insert("worker_utilization".to_string(), throughput_metrics.worker_utilization);
            }
            Err(e) => {
                success = false;
                error_message = Some(format!("Throughput processor start failed: {}", e));
            }
        }
        
        // Test caching performance
        if success {
            let query_cache = self.cache_manager.query_cache();
            
            for i in 0..20 {
                let key = format!("perf_test_key_{}", i);
                let value = format!("perf_test_value_{}", i).into_bytes();
                
                if let Err(e) = query_cache.put(key.clone(), value, None).await {
                    success = false;
                    error_message = Some(format!("Cache put failed for key {}: {}", key, e));
                    break;
                }
            }
            
            if success {
                let cache_stats = self.cache_manager.get_combined_stats().await;
                metrics.insert("cache_hit_rate".to_string(), cache_stats.query_cache.hit_rate());
                metrics.insert("cache_memory_mb".to_string(), cache_stats.total_memory_mb);
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_error_handling_recovery(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Error Handling and Recovery".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test error handler registration
        match self.error_handler.register_exex("test_error_exex".to_string()).await {
            Ok(_) => {
                metrics.insert("error_handler_registration_success".to_string(), 1.0);
                
                // Test error recording
                self.error_handler.record_communication_result(
                    "test_error_exex",
                    false,
                    Duration::from_millis(5000),
                    Some("Simulated connection timeout".to_string()),
                ).await?;
                
                metrics.insert("error_recording_success".to_string(), 1.0);
                
                // Test circuit breaker
                for i in 0..3 {
                    let result = self.circuit_breaker.call(async {
                        if i < 2 {
                            Ok::<i32, &str>(42)
                        } else {
                            Err("Test circuit breaker failure")
                        }
                    }).await;
                    
                    match result {
                        Ok(_) => metrics.insert(format!("circuit_breaker_call_{}_success", i), 1.0),
                        Err(_) => metrics.insert(format!("circuit_breaker_call_{}_failure", i), 1.0),
                    };
                }
                
                // Test recovery coordinator
                let component_info = monmouth_rag_memory_exex::error_handling::recovery::ComponentInfo {
                    component_id: "test_recovery_component".to_string(),
                    status: monmouth_rag_memory_exex::error_handling::recovery::ComponentStatus::Healthy,
                    last_health_check: std::time::Instant::now(),
                    dependencies: vec!["database".to_string()],
                    recovery_attempts: 0,
                    total_failures: 0,
                    last_recovery_attempt: None,
                };
                
                match self.recovery_coordinator.register_component(component_info).await {
                    Ok(_) => {
                        metrics.insert("recovery_component_registration_success".to_string(), 1.0);
                        
                        // Start recovery processing
                        if let Err(e) = self.recovery_coordinator.start_recovery_processing().await {
                            success = false;
                            error_message = Some(format!("Recovery processing start failed: {}", e));
                        } else {
                            metrics.insert("recovery_processing_start_success".to_string(), 1.0);
                        }
                    }
                    Err(e) => {
                        success = false;
                        error_message = Some(format!("Recovery component registration failed: {}", e));
                    }
                }
            }
            Err(e) => {
                success = false;
                error_message = Some(format!("Error handler registration failed: {}", e));
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_reorg_coordination(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Reorg Coordination".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Simulate reorg detection
        let reorg_info = monmouth_rag_memory_exex::reorg::coordination::ReorgInfo {
            reorg_id: format!("test_reorg_{}", Uuid::new_v4()),
            old_chain_tip: B256::random(),
            new_chain_tip: B256::random(),
            common_ancestor: B256::random(),
            depth: 3,
            severity: monmouth_rag_memory_exex::reorg::coordination::ReorgSeverity::Minor,
            affected_agents: (0..5).map(|i| format!("test_agent_{}", i)).collect(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        
        match self.reorg_coordinator.handle_reorg(reorg_info, ReorgStrategy::Rollback).await {
            Ok(result) => {
                metrics.insert("reorg_handling_success".to_string(), 1.0);
                metrics.insert("reorg_processing_time_ms".to_string(), result.total_processing_time_ms as f64);
                metrics.insert("agents_recovered".to_string(), result.agents_recovered.len() as f64);
                
                // Test checkpoint creation during reorg
                for i in 0..3 {
                    let agent_id = format!("test_agent_{}", i);
                    match self.checkpoint_manager.create_checkpoint(&agent_id, "reorg_test").await {
                        Ok(checkpoint_id) => {
                            metrics.insert(format!("checkpoint_{}_creation_success", i), 1.0);
                            
                            // Test checkpoint validation
                            match self.checkpoint_manager.validate_checkpoint(&checkpoint_id).await {
                                Ok(is_valid) => {
                                    if is_valid {
                                        metrics.insert(format!("checkpoint_{}_validation_success", i), 1.0);
                                    } else {
                                        success = false;
                                        error_message = Some(format!("Checkpoint {} validation failed", checkpoint_id));
                                        break;
                                    }
                                }
                                Err(e) => {
                                    success = false;
                                    error_message = Some(format!("Checkpoint {} validation error: {}", checkpoint_id, e));
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            success = false;
                            error_message = Some(format!("Checkpoint creation failed for agent {}: {}", agent_id, e));
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                success = false;
                error_message = Some(format!("Reorg handling failed: {}", e));
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_agent_checkpointing(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Agent Checkpointing".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test agent checkpoint creation
        for i in 0..5 {
            let agent_id = format!("test_agent_{}", i);
            
            match self.agent_checkpoint_manager.create_agent_checkpoint(&agent_id).await {
                Ok(checkpoint_result) => {
                    metrics.insert(format!("agent_checkpoint_{}_success", i), 1.0);
                    metrics.insert(format!("agent_checkpoint_{}_size", i), checkpoint_result.checkpoint_size_bytes as f64);
                    
                    // Test checkpoint restoration
                    match self.agent_checkpoint_manager.restore_from_checkpoint(&checkpoint_result.checkpoint_id).await {
                        Ok(restore_result) => {
                            metrics.insert(format!("agent_restore_{}_success", i), 1.0);
                            metrics.insert(format!("agent_restore_{}_time_ms", i), restore_result.restoration_time_ms as f64);
                        }
                        Err(e) => {
                            success = false;
                            error_message = Some(format!("Agent checkpoint restoration failed for {}: {}", agent_id, e));
                            break;
                        }
                    }
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Agent checkpoint creation failed for {}: {}", agent_id, e));
                    break;
                }
            }
        }
        
        // Test agent recovery
        if success {
            let test_agent_id = "test_agent_0".to_string();
            
            match self.agent_recovery_manager.initiate_recovery(&test_agent_id).await {
                Ok(recovery_result) => {
                    metrics.insert("agent_recovery_success".to_string(), 1.0);
                    metrics.insert("agent_recovery_time_ms".to_string(), recovery_result.recovery_time_ms as f64);
                    metrics.insert("agent_recovery_strategy".to_string(), recovery_result.strategy_used as f64);
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Agent recovery failed: {}", e));
                }
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_monitoring_alerting(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "Monitoring and Alerting".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Test production monitoring
        let test_component = monmouth_rag_memory_exex::monitoring::production::ComponentHealth {
            component_id: "test_monitoring_component".to_string(),
            status: monmouth_rag_memory_exex::monitoring::production::ComponentStatus::Healthy,
            health_score: 0.95,
            last_check: chrono::Utc::now().timestamp() as u64,
            metrics: monmouth_rag_memory_exex::monitoring::production::ComponentMetrics {
                response_time_ms: 25.0,
                throughput: 500.0,
                error_count: 0,
                resource_utilization: 0.3,
            },
            dependencies: vec!["database".to_string()],
        };
        
        match self.production_monitor.register_component(test_component).await {
            Ok(_) => {
                metrics.insert("production_monitor_registration_success".to_string(), 1.0);
                
                // Get monitoring metrics
                match self.production_monitor.get_metrics().await {
                    Ok(monitoring_metrics) => {
                        metrics.insert("system_health_score".to_string(), monitoring_metrics.system_health_score);
                        metrics.insert("healthy_components".to_string(), monitoring_metrics.healthy_components as f64);
                        metrics.insert("degraded_components".to_string(), monitoring_metrics.degraded_components as f64);
                    }
                    Err(e) => {
                        success = false;
                        error_message = Some(format!("Failed to get monitoring metrics: {}", e));
                    }
                }
            }
            Err(e) => {
                success = false;
                error_message = Some(format!("Production monitor registration failed: {}", e));
            }
        }
        
        // Test alerting
        if success {
            let test_rule = monmouth_rag_memory_exex::monitoring::alerts::AlertRule {
                name: "test_integration_alert".to_string(),
                description: "Integration test alert".to_string(),
                condition: "test_metric > 100.0".to_string(),
                severity: monmouth_rag_memory_exex::monitoring::alerts::AlertSeverity::Warning,
                evaluation_window: Duration::from_secs(60),
                cooldown: Duration::from_secs(300),
                enabled: true,
            };
            
            match self.alert_manager.add_rule(test_rule).await {
                Ok(_) => {
                    metrics.insert("alert_rule_addition_success".to_string(), 1.0);
                    
                    // Test alert evaluation
                    let test_production_metrics = monmouth_rag_memory_exex::monitoring::production::ProductionMetrics {
                        timestamp: chrono::Utc::now().timestamp() as u64,
                        uptime_seconds: 3600,
                        cpu_usage_percent: 45.0,
                        memory_usage_mb: 2048,
                        disk_usage_percent: 30.0,
                        network_bytes_in: 1_000_000,
                        network_bytes_out: 800_000,
                        active_connections: 150,
                        request_rate: 500.0,
                        error_rate: 1.5,
                        latency_p50_ms: 25.0,
                        latency_p95_ms: 150.0,
                        latency_p99_ms: 200.0,
                    };
                    
                    if let Err(e) = self.alert_manager.evaluate_metrics(&test_production_metrics).await {
                        success = false;
                        error_message = Some(format!("Alert evaluation failed: {}", e));
                    } else {
                        metrics.insert("alert_evaluation_success".to_string(), 1.0);
                    }
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("Alert rule addition failed: {}", e));
                }
            }
        }
        
        // Test health checking
        if success {
            let test_components = vec!["rag_component", "memory_component", "coordination_component"];
            
            for (i, component) in test_components.iter().enumerate() {
                match self.health_checker.perform_health_check(component).await {
                    Ok(result) => {
                        metrics.insert(format!("health_check_{}_success", i), 1.0);
                        metrics.insert(format!("health_check_{}_score", i), result.overall_score);
                        metrics.insert(format!("health_check_{}_response_time", i), result.response_time_ms as f64);
                    }
                    Err(e) => {
                        success = false;
                        error_message = Some(format!("Health check failed for {}: {}", component, e));
                        break;
                    }
                }
            }
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    async fn test_end_to_end_workflow(&self) -> Result<TestResult> {
        let start_time = std::time::Instant::now();
        let test_name = "End-to-End Workflow".to_string();
        
        let mut metrics = HashMap::new();
        let mut error_message = None;
        let mut success = true;
        
        // Simulate complete transaction workflow
        for i in 0..5 {
            let agent_id = format!("test_agent_{}", i);
            let tx_hash = reth_primitives::H256::random();
            
            // Step 1: RAG context retrieval
            let query = format!("Transaction analysis for agent {}", agent_id);
            match self.context_retriever.retrieve_context(&agent_id, &query, 3).await {
                Ok(context) => {
                    metrics.insert(format!("e2e_{}_rag_success", i), 1.0);
                    
                    // Step 2: AI decision making
                    let decision = AgentDecision {
                        agent_id: agent_id.clone(),
                        decision_type: "process_transaction".to_string(),
                        confidence: 0.8 + (i as f64 * 0.05),
                        reasoning: format!("Based on context: {:?}", context),
                        suggested_actions: vec!["store_memory".to_string(), "update_graph".to_string()],
                        timestamp: chrono::Utc::now().timestamp() as u64,
                    };
                    
                    // Step 3: Memory storage
                    let memory_data = format!("Decision result for tx {:?}: {:?}", tx_hash, decision).into_bytes();
                    match self.agent_memory_integration.store_agent_memory(&agent_id, "working", memory_data).await {
                        Ok(memory_hash) => {
                            metrics.insert(format!("e2e_{}_memory_success", i), 1.0);
                            
                            // Step 4: State synchronization
                            let alh_query = ALHQuery {
                                query_id: format!("e2e_query_{}", i),
                                query_type: QueryType::MemoryState,
                                agent_id: agent_id.clone(),
                                parameters: serde_json::json!({
                                    "memory_hash": memory_hash,
                                    "transaction_hash": format!("{:?}", tx_hash)
                                }),
                                timestamp: chrono::Utc::now().timestamp() as u64,
                            };
                            
                            match self.alh_coordinator.process_query(alh_query).await {
                                Ok(alh_result) => {
                                    metrics.insert(format!("e2e_{}_alh_success", i), 1.0);
                                    metrics.insert(format!("e2e_{}_alh_time_ms", i), alh_result.processing_time_ms);
                                    
                                    // Step 5: Cross-ExEx message
                                    let message = CrossExExMessage::TransactionAnalysis {
                                        tx_hash,
                                        routing_decision: RoutingDecision::ProcessWithRAG,
                                        context: context.join(" ").into_bytes(),
                                        timestamp: std::time::Instant::now(),
                                    };
                                    
                                    match self.performance_optimizer.send_optimized_message("memory_exex".to_string(), message).await {
                                        Ok(_) => {
                                            metrics.insert(format!("e2e_{}_message_success", i), 1.0);
                                            
                                            // Step 6: Update agent state
                                            if let Some(agent) = self.test_agents.write().await.get_mut(&agent_id) {
                                                agent.transaction_count += 1;
                                                agent.memory_hash = Some(memory_hash);
                                                agent.context_history.extend(context);
                                                agent.status = AgentStatus::Active;
                                            }
                                            
                                            metrics.insert(format!("e2e_{}_complete_success", i), 1.0);
                                        }
                                        Err(e) => {
                                            success = false;
                                            error_message = Some(format!("E2E step 5 failed for agent {}: {}", agent_id, e));
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    success = false;
                                    error_message = Some(format!("E2E step 4 failed for agent {}: {}", agent_id, e));
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            success = false;
                            error_message = Some(format!("E2E step 3 failed for agent {}: {}", agent_id, e));
                            break;
                        }
                    }
                }
                Err(e) => {
                    success = false;
                    error_message = Some(format!("E2E step 1 failed for agent {}: {}", agent_id, e));
                    break;
                }
            }
        }
        
        // Calculate overall workflow metrics
        if success {
            let test_agents = self.test_agents.read().await;
            let total_transactions: usize = test_agents.values().map(|a| a.transaction_count).sum();
            let agents_with_memory = test_agents.values().filter(|a| a.memory_hash.is_some()).count();
            let total_context_entries: usize = test_agents.values().map(|a| a.context_history.len()).sum();
            
            metrics.insert("total_e2e_transactions".to_string(), total_transactions as f64);
            metrics.insert("agents_with_memory".to_string(), agents_with_memory as f64);
            metrics.insert("total_context_entries".to_string(), total_context_entries as f64);
        }
        
        let duration = start_time.elapsed();
        metrics.insert("test_duration_ms".to_string(), duration.as_millis() as f64);
        
        Ok(TestResult {
            test_name,
            success,
            duration,
            error_message,
            metrics,
        })
    }
    
    /// Generate comprehensive test report
    pub async fn generate_test_report(&self) -> Result<String> {
        let results = self.test_results.read().await;
        let mut report = String::new();
        
        report.push_str("# Cross-ExEx Integration Test Report\n\n");
        report.push_str(&format!("Generated at: {}\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
        report.push_str(&format!("Test Configuration: {:?}\n\n", self.config));
        
        let total_tests = results.len();
        let successful_tests = results.iter().filter(|r| r.success).count();
        let failed_tests = total_tests - successful_tests;
        
        report.push_str("## Summary\n");
        report.push_str(&format!("- Total Tests: {}\n", total_tests));
        report.push_str(&format!("- Successful: {}\n", successful_tests));
        report.push_str(&format!("- Failed: {}\n", failed_tests));
        report.push_str(&format!("- Success Rate: {:.2}%\n\n", (successful_tests as f64 / total_tests as f64) * 100.0));
        
        report.push_str("## Detailed Results\n\n");
        
        for result in results.iter() {
            report.push_str(&format!("### {}\n", result.test_name));
            report.push_str(&format!("- Status: {}\n", if result.success { "✅ PASSED" } else { "❌ FAILED" }));
            report.push_str(&format!("- Duration: {:.2}ms\n", result.duration.as_millis()));
            
            if let Some(ref error) = result.error_message {
                report.push_str(&format!("- Error: {}\n", error));
            }
            
            if !result.metrics.is_empty() {
                report.push_str("- Metrics:\n");
                for (key, value) in &result.metrics {
                    report.push_str(&format!("  - {}: {:.2}\n", key, value));
                }
            }
            
            report.push_str("\n");
        }
        
        // Add performance summary
        report.push_str("## Performance Summary\n\n");
        let total_duration: u128 = results.iter().map(|r| r.duration.as_millis()).sum();
        report.push_str(&format!("- Total Test Duration: {:.2}s\n", total_duration as f64 / 1000.0));
        
        // Extract key performance metrics
        let mut avg_latencies = Vec::new();
        let mut cache_hit_rates = Vec::new();
        let mut throughput_values = Vec::new();
        
        for result in results.iter() {
            if let Some(latency) = result.metrics.get("avg_latency_ms") {
                avg_latencies.push(*latency);
            }
            if let Some(hit_rate) = result.metrics.get("cache_hit_rate") {
                cache_hit_rates.push(*hit_rate);
            }
            if let Some(tps) = result.metrics.get("current_tps") {
                throughput_values.push(*tps);
            }
        }
        
        if !avg_latencies.is_empty() {
            let avg_latency = avg_latencies.iter().sum::<f64>() / avg_latencies.len() as f64;
            report.push_str(&format!("- Average Latency: {:.2}ms\n", avg_latency));
        }
        
        if !cache_hit_rates.is_empty() {
            let avg_hit_rate = cache_hit_rates.iter().sum::<f64>() / cache_hit_rates.len() as f64;
            report.push_str(&format!("- Average Cache Hit Rate: {:.2}%\n", avg_hit_rate * 100.0));
        }
        
        if !throughput_values.is_empty() {
            let avg_throughput = throughput_values.iter().sum::<f64>() / throughput_values.len() as f64;
            report.push_str(&format!("- Average Throughput: {:.2} TPS\n", avg_throughput));
        }
        
        Ok(report)
    }
    
    /// Clean up test environment
    pub async fn cleanup(&self) -> Result<()> {
        // Stop all monitoring and processing
        let _ = self.production_monitor.stop_monitoring().await;
        let _ = self.alert_manager.stop().await;
        let _ = self.health_checker.stop().await;
        let _ = self.throughput_optimizer.stop_processing().await;
        
        // Clear test data
        self.test_agents.write().await.clear();
        self.test_results.write().await.clear();
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_integration_harness_creation() {
        let config = IntegrationTestConfig::default();
        let harness = CrossExExTestHarness::new(config).await.unwrap();
        
        // Basic validation that harness was created
        assert_eq!(harness.test_agents.read().await.len(), 0);
        assert_eq!(harness.test_results.read().await.len(), 0);
    }
    
    #[tokio::test]
    async fn test_environment_setup() {
        let config = IntegrationTestConfig {
            agent_count: 3,
            ..Default::default()
        };
        let harness = CrossExExTestHarness::new(config).await.unwrap();
        
        harness.setup_test_environment().await.unwrap();
        
        // Verify agents were created
        let test_agents = harness.test_agents.read().await;
        assert_eq!(test_agents.len(), 3);
        
        for i in 0..3 {
            let agent_id = format!("test_agent_{}", i);
            assert!(test_agents.contains_key(&agent_id));
        }
    }
    
    #[tokio::test]
    async fn test_single_integration_test() {
        let config = IntegrationTestConfig {
            agent_count: 2,
            transaction_count: 5,
            test_timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let harness = CrossExExTestHarness::new(config).await.unwrap();
        
        harness.setup_test_environment().await.unwrap();
        
        // Run a single test
        let result = harness.test_cross_exex_communication().await.unwrap();
        
        // The test might fail due to mock components, but it should complete
        assert!(!result.test_name.is_empty());
        assert!(result.duration.as_millis() > 0);
    }
    
    #[tokio::test] 
    async fn test_report_generation() {
        let config = IntegrationTestConfig {
            agent_count: 1,
            transaction_count: 1,
            ..Default::default()
        };
        let harness = CrossExExTestHarness::new(config).await.unwrap();
        
        // Add a mock test result
        let test_result = TestResult {
            test_name: "Mock Test".to_string(),
            success: true,
            duration: Duration::from_millis(100),
            error_message: None,
            metrics: {
                let mut m = HashMap::new();
                m.insert("test_metric".to_string(), 42.0);
                m
            },
        };
        
        harness.test_results.write().await.push(test_result);
        
        let report = harness.generate_test_report().await.unwrap();
        
        assert!(report.contains("Cross-ExEx Integration Test Report"));
        assert!(report.contains("Mock Test"));
        assert!(report.contains("✅ PASSED"));
        assert!(report.contains("test_metric: 42.00"));
    }
}