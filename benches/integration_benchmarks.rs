use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use alloy::primitives::{Address, B256};

use monmouth_rag_memory_exex::{
    shared::{
        communication::{CrossExExMessage, MessageRouter},
        ai_agent::{RoutingDecision, AgentDecision},
        agent_state_manager::AgentStateManager,
        metrics_v2::MetricsV2,
    },
    rag_exex::{
        context_retrieval::ContextRetriever,
        vector_store::VectorStore,
        embeddings::{EmbeddingGenerator, batch::BatchEmbeddingProcessor},
        knowledge_graph::KnowledgeGraph,
    },
    memory_exex::{
        memory_store::MemoryStore,
        agent_integration::AgentMemoryIntegration,
        checkpointing::CheckpointManager,
    },
    alh::coordination::{ALHCoordinator, QueryType, ALHQuery},
    reorg::coordination::{ReorgCoordinator, ReorgStrategy},
    performance::{
        CrossExExOptimizer, PerformanceConfig, LoadBalancingStrategy,
        cache::CacheManager,
        throughput::{ThroughputOptimizer, PrioritizedMessage},
        batching::BatchedMessageProcessor,
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
        dashboard::MetricsDashboard,
        MonitoringConfig,
    },
};

/// Benchmark configuration for different test scenarios
#[derive(Clone)]
struct BenchmarkConfig {
    agent_count: usize,
    message_count: usize,
    batch_size: usize,
    context_window_size: usize,
    memory_size_kb: usize,
}

impl BenchmarkConfig {
    fn small() -> Self {
        Self {
            agent_count: 10,
            message_count: 100,
            batch_size: 10,
            context_window_size: 5,
            memory_size_kb: 1,
        }
    }
    
    fn medium() -> Self {
        Self {
            agent_count: 100,
            message_count: 1000,
            batch_size: 50,
            context_window_size: 10,
            memory_size_kb: 10,
        }
    }
    
    fn large() -> Self {
        Self {
            agent_count: 1000,
            message_count: 10000,
            batch_size: 100,
            context_window_size: 20,
            memory_size_kb: 100,
        }
    }
}

/// Benchmark harness for integrated system performance testing
struct IntegrationBenchmarkHarness {
    // Core components
    message_router: Arc<MessageRouter>,
    agent_state_manager: Arc<AgentStateManager>,
    metrics: Arc<MetricsV2>,
    
    // RAG components
    context_retriever: Arc<ContextRetriever>,
    vector_store: Arc<VectorStore>,
    embedding_generator: Arc<EmbeddingGenerator>,
    batch_processor: Arc<BatchEmbeddingProcessor>,
    knowledge_graph: Arc<KnowledgeGraph>,
    
    // Memory components
    memory_store: Arc<MemoryStore>,
    agent_memory_integration: Arc<AgentMemoryIntegration>,
    checkpoint_manager: Arc<CheckpointManager>,
    
    // State sync components
    alh_coordinator: Arc<ALHCoordinator>,
    reorg_coordinator: Arc<ReorgCoordinator>,
    
    // Performance components
    performance_optimizer: Arc<CrossExExOptimizer>,
    cache_manager: Arc<CacheManager>,
    throughput_optimizer: Arc<ThroughputOptimizer<String>>,
    batch_message_processor: Arc<BatchedMessageProcessor<String>>,
    
    // Error handling components
    error_handler: Arc<CrossExExErrorHandler>,
    circuit_breaker: Arc<CircuitBreaker>,
    recovery_coordinator: Arc<RecoveryCoordinator>,
    
    // Monitoring components
    production_monitor: Arc<ProductionMonitor>,
    alert_manager: Arc<AlertManager>,
    health_checker: Arc<HealthChecker>,
    dashboard: Arc<MetricsDashboard>,
}

impl IntegrationBenchmarkHarness {
    async fn new() -> eyre::Result<Self> {
        // Initialize core components
        let message_router = Arc::new(MessageRouter::new());
        let agent_state_manager = Arc::new(AgentStateManager::new().await?);
        let metrics = Arc::new(MetricsV2::new().await?);
        
        // Initialize RAG components
        let context_retriever = Arc::new(ContextRetriever::new().await?);
        let vector_store = Arc::new(VectorStore::new().await?);
        let embedding_generator = Arc::new(EmbeddingGenerator::new().await?);
        let batch_processor = Arc::new(BatchEmbeddingProcessor::new().await?);
        let knowledge_graph = Arc::new(KnowledgeGraph::new().await?);
        
        // Initialize Memory components
        let memory_store = Arc::new(MemoryStore::new().await?);
        let agent_memory_integration = Arc::new(AgentMemoryIntegration::new(memory_store.clone()).await?);
        let checkpoint_manager = Arc::new(CheckpointManager::new().await?);
        
        // Initialize state sync components
        let alh_coordinator = Arc::new(ALHCoordinator::new().await?);
        let reorg_coordinator = Arc::new(ReorgCoordinator::new().await?);
        
        // Initialize performance components
        let perf_config = PerformanceConfig {
            enable_connection_pooling: true,
            max_connections_per_exex: 20,
            connection_timeout: Duration::from_secs(30),
            message_batch_size: 100,
            batch_timeout: Duration::from_millis(50),
            enable_compression: true,
            compression_threshold: 1024,
            cache_size_mb: 256,
            cache_ttl: Duration::from_secs(300),
            enable_parallel_processing: true,
            max_parallel_workers: 8,
            load_balancing_strategy: LoadBalancingStrategy::AdaptiveLoad,
        };
        
        let performance_optimizer = Arc::new(CrossExExOptimizer::new(perf_config));
        let cache_manager = Arc::new(CacheManager::new(Default::default()));
        let throughput_optimizer = Arc::new(ThroughputOptimizer::new(Default::default()));
        let batch_message_processor = Arc::new(BatchedMessageProcessor::new(Default::default()));
        
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
        let dashboard = Arc::new(MetricsDashboard::new(Default::default()));
        
        Ok(Self {
            message_router,
            agent_state_manager,
            metrics,
            context_retriever,
            vector_store,
            embedding_generator,
            batch_processor,
            knowledge_graph,
            memory_store,
            agent_memory_integration,
            checkpoint_manager,
            alh_coordinator,
            reorg_coordinator,
            performance_optimizer,
            cache_manager,
            throughput_optimizer,
            batch_message_processor,
            error_handler,
            circuit_breaker,
            recovery_coordinator,
            production_monitor,
            alert_manager,
            health_checker,
            dashboard,
        })
    }
    
    async fn setup(&self, config: &BenchmarkConfig) -> eyre::Result<()> {
        // Initialize performance optimization
        self.performance_optimizer.initialize_connection_pool("rag_exex".to_string(), 10).await?;
        self.performance_optimizer.initialize_connection_pool("memory_exex".to_string(), 10).await?;
        self.performance_optimizer.initialize_connection_pool("coordination_exex".to_string(), 5).await?;
        
        // Start cache cleanup tasks
        self.cache_manager.start_all_cleanup_tasks().await;
        
        // Register test agents
        for i in 0..config.agent_count {
            let agent_id = format!("bench_agent_{:04}", i);
            let address = Address::random();
            self.agent_state_manager.register_agent(agent_id, address).await?;
        }
        
        // Populate vector store with test data
        for i in 0..100 {
            let doc_id = format!("bench_doc_{}", i);
            let content = format!("Benchmark document {} for performance testing with context retrieval", i);
            let embedding: Vec<f32> = (0..384).map(|j| (i as f32 + j as f32) * 0.001).collect();
            
            self.vector_store.store_embedding(doc_id, embedding, content).await?;
        }
        
        Ok(())
    }
}

/// Benchmark cross-ExEx communication performance
fn bench_cross_exex_communication(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("cross_exex_communication");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Elements(config.message_count as u64));
        group.bench_with_input(
            BenchmarkId::new("message_routing", config.message_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.message_count {
                        let message = CrossExExMessage::TransactionAnalysis {
                            tx_hash: reth_primitives::H256::random(),
                            routing_decision: RoutingDecision::ProcessWithRAG,
                            context: format!("benchmark_context_{}", i).into_bytes(),
                            timestamp: std::time::Instant::now(),
                        };
                        
                        let target_exex = if i % 2 == 0 { "rag_exex" } else { "memory_exex" };
                        
                        black_box(
                            harness.performance_optimizer
                                .send_optimized_message(target_exex.to_string(), message)
                                .await
                                .unwrap()
                        );
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark RAG context retrieval performance
fn bench_rag_context_retrieval(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("rag_context_retrieval");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Elements(config.agent_count as u64));
        group.bench_with_input(
            BenchmarkId::new("context_queries", config.agent_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.agent_count {
                        let agent_id = format!("bench_agent_{:04}", i);
                        let query = format!("benchmark query {} for performance testing", i);
                        
                        black_box(
                            harness.context_retriever
                                .retrieve_context(&agent_id, &query, config.context_window_size)
                                .await
                                .unwrap()
                        );
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark memory operations performance
fn bench_memory_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("memory_operations");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Bytes(
            (config.agent_count * config.memory_size_kb * 1024) as u64
        ));
        
        group.bench_with_input(
            BenchmarkId::new("store_retrieve", format!("{}agents_{}kb", config.agent_count, config.memory_size_kb)),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.agent_count {
                        let agent_id = format!("bench_agent_{:04}", i);
                        let memory_data = vec![0u8; config.memory_size_kb * 1024];
                        
                        // Store memory
                        let memory_hash = harness.agent_memory_integration
                            .store_agent_memory(&agent_id, "working", memory_data.clone())
                            .await
                            .unwrap();
                        
                        // Retrieve memory
                        let retrieved = harness.agent_memory_integration
                            .retrieve_agent_memory(&agent_id, "working")
                            .await
                            .unwrap();
                        
                        black_box((memory_hash, retrieved));
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark state synchronization performance
fn bench_state_synchronization(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("state_synchronization");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Elements(config.agent_count as u64));
        group.bench_with_input(
            BenchmarkId::new("alh_queries", config.agent_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.agent_count {
                        let agent_id = format!("bench_agent_{:04}", i);
                        
                        let query = ALHQuery {
                            query_id: format!("bench_query_{}", i),
                            query_type: QueryType::MemoryState,
                            agent_id: agent_id.clone(),
                            parameters: serde_json::json!({
                                "benchmark": true,
                                "iteration": i
                            }),
                            timestamp: chrono::Utc::now().timestamp() as u64,
                        };
                        
                        black_box(
                            harness.alh_coordinator
                                .process_query(query)
                                .await
                                .unwrap()
                        );
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark caching performance
fn bench_caching_performance(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("caching_performance");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        let query_cache = harness.cache_manager.query_cache();
        
        // Pre-populate cache
        rt.block_on(async {
            for i in 0..config.agent_count {
                let key = format!("bench_key_{}", i);
                let value = vec![0u8; config.memory_size_kb * 1024];
                query_cache.put(key, value, None).await.unwrap();
            }
        });
        
        group.throughput(Throughput::Elements(config.agent_count as u64));
        group.bench_with_input(
            BenchmarkId::new("cache_operations", config.agent_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    // Mix of cache hits and misses
                    for i in 0..config.agent_count {
                        let key = if i % 3 == 0 {
                            format!("bench_key_{}", i) // Cache hit
                        } else {
                            format!("new_key_{}", i) // Cache miss
                        };
                        
                        let result = query_cache.get(&key).await;
                        black_box(result);
                        
                        // For cache misses, add new entries
                        if i % 3 != 0 {
                            let value = vec![0u8; config.memory_size_kb * 1024];
                            query_cache.put(key, value, None).await.unwrap();
                        }
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark throughput optimization
fn bench_throughput_optimization(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("throughput_optimization");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        // Start throughput processor
        rt.block_on(async {
            let processor = |messages: Vec<String>| async move {
                // Simulate processing time
                tokio::time::sleep(Duration::from_micros(10)).await;
                Ok::<(), eyre::Report>(())
            };
            harness.throughput_optimizer.start_processing(processor).await.unwrap();
        });
        
        group.throughput(Throughput::Elements(config.message_count as u64));
        group.bench_with_input(
            BenchmarkId::new("message_submission", config.message_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.message_count {
                        let message = PrioritizedMessage::new(
                            format!("bench_message_{}", i),
                            (i % 3) as u8,
                            Address::random(),
                        );
                        
                        black_box(
                            harness.throughput_optimizer
                                .submit_message(message)
                                .await
                                .unwrap()
                        );
                    }
                    
                    // Wait for processing to complete
                    tokio::time::sleep(Duration::from_millis(100)).await;
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark error handling performance
fn bench_error_handling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("error_handling");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Elements(config.message_count as u64));
        group.bench_with_input(
            BenchmarkId::new("circuit_breaker", config.message_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.message_count {
                        let result = harness.circuit_breaker.call(async {
                            if i % 10 == 0 {
                                Err("Simulated failure")
                            } else {
                                Ok::<i32, &str>(42)
                            }
                        }).await;
                        
                        black_box(result);
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark monitoring performance
fn bench_monitoring_performance(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("monitoring_performance");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium(), BenchmarkConfig::large()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        // Start monitoring systems
        rt.block_on(async {
            harness.production_monitor.start_monitoring().await.unwrap();
            harness.health_checker.start().await.unwrap();
        });
        
        group.throughput(Throughput::Elements(config.agent_count as u64));
        group.bench_with_input(
            BenchmarkId::new("health_checks", config.agent_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.agent_count {
                        let component = format!("bench_component_{}", i);
                        
                        let result = harness.health_checker
                            .perform_health_check(&component)
                            .await
                            .unwrap();
                        
                        black_box(result);
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark end-to-end integrated workflow
fn bench_end_to_end_integration(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("end_to_end_integration");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(10);
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Elements(config.agent_count as u64));
        group.bench_with_input(
            BenchmarkId::new("full_workflow", config.agent_count),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    for i in 0..config.agent_count {
                        let agent_id = format!("bench_agent_{:04}", i);
                        
                        // Step 1: RAG context retrieval
                        let query = format!("benchmark transaction for agent {}", agent_id);
                        let context = harness.context_retriever
                            .retrieve_context(&agent_id, &query, 3)
                            .await
                            .unwrap();
                        
                        // Step 2: Memory storage
                        let memory_data = format!("Context: {:?}", context).into_bytes();
                        let memory_hash = harness.agent_memory_integration
                            .store_agent_memory(&agent_id, "working", memory_data)
                            .await
                            .unwrap();
                        
                        // Step 3: State synchronization
                        let alh_query = ALHQuery {
                            query_id: format!("e2e_query_{}", i),
                            query_type: QueryType::MemoryState,
                            agent_id: agent_id.clone(),
                            parameters: serde_json::json!({
                                "memory_hash": memory_hash,
                                "workflow": "end_to_end"
                            }),
                            timestamp: chrono::Utc::now().timestamp() as u64,
                        };
                        
                        let alh_result = harness.alh_coordinator
                            .process_query(alh_query)
                            .await
                            .unwrap();
                        
                        // Step 4: Cross-ExEx message
                        let message = CrossExExMessage::TransactionAnalysis {
                            tx_hash: reth_primitives::H256::random(),
                            routing_decision: RoutingDecision::ProcessWithRAG,
                            context: context.join(" ").into_bytes(),
                            timestamp: std::time::Instant::now(),
                        };
                        
                        harness.performance_optimizer
                            .send_optimized_message("memory_exex".to_string(), message)
                            .await
                            .unwrap();
                        
                        black_box((context, memory_hash, alh_result));
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark reorg handling performance
fn bench_reorg_handling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let harness = rt.block_on(IntegrationBenchmarkHarness::new()).unwrap();
    
    let mut group = c.benchmark_group("reorg_handling");
    
    for config in [BenchmarkConfig::small(), BenchmarkConfig::medium()] {
        rt.block_on(harness.setup(&config)).unwrap();
        
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("reorg_recovery", format!("{}agents", config.agent_count)),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    let reorg_info = monmouth_rag_memory_exex::reorg::coordination::ReorgInfo {
                        reorg_id: format!("bench_reorg_{}", fastrand::u64(..)),
                        old_chain_tip: B256::random(),
                        new_chain_tip: B256::random(),
                        common_ancestor: B256::random(),
                        depth: 3,
                        severity: monmouth_rag_memory_exex::reorg::coordination::ReorgSeverity::Minor,
                        affected_agents: (0..config.agent_count)
                            .map(|i| format!("bench_agent_{:04}", i))
                            .collect(),
                        timestamp: chrono::Utc::now().timestamp() as u64,
                    };
                    
                    let result = harness.reorg_coordinator
                        .handle_reorg(reorg_info, ReorgStrategy::Rollback)
                        .await
                        .unwrap();
                    
                    black_box(result);
                });
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_cross_exex_communication,
    bench_rag_context_retrieval,
    bench_memory_operations,
    bench_state_synchronization,
    bench_caching_performance,
    bench_throughput_optimization,
    bench_error_handling,
    bench_monitoring_performance,
    bench_end_to_end_integration,
    bench_reorg_handling
);

criterion_main!(benches);