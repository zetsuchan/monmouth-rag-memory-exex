use alloy::primitives::{Address, B256};
use eyre::Result;
use monmouth_rag_memory_exex::{
    performance::{
        PerformanceConfig, CrossExExOptimizer, LoadBalancingStrategy,
        cache::{CacheManager, CacheConfig},
        throughput::{ThroughputOptimizer, ThroughputConfig, PrioritizedMessage},
        batching::{BatchedMessageProcessor, BatchingConfig, BatchedMessage},
    },
    error_handling::{
        ErrorHandlingConfig, CrossExExErrorHandler, CrossExExErrorConfig,
        circuit_breaker::{CircuitBreaker, CircuitBreakerConfig},
        retry::{RetryPolicy, ExponentialBackoff},
        recovery::{RecoveryCoordinator, RecoveryConfig, ComponentInfo, ComponentStatus},
    },
    monitoring::{
        MonitoringConfig, AlertChannel,
        production::{ProductionMonitor, ComponentHealth, ComponentMetrics},
        alerts::{AlertManager, AlertRule, AlertSeverity},
        health::{HealthChecker, HealthCheckConfig, HealthStatus},
        dashboard::{MetricsDashboard, DashboardConfig},
    },
    shared::communication::CrossExExMessage,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{info, warn, error};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Performance & Production Demo");

    let demo_components = initialize_components().await?;
    
    run_demo(&demo_components).await?;

    Ok(())
}

struct DemoComponents {
    performance_optimizer: Arc<CrossExExOptimizer>,
    cache_manager: Arc<CacheManager>,
    throughput_optimizer: Arc<ThroughputOptimizer<String>>,
    batch_processor: Arc<BatchedMessageProcessor<String>>,
    error_handler: Arc<CrossExExErrorHandler>,
    circuit_breaker: Arc<CircuitBreaker>,
    retry_policy: Arc<RetryPolicy>,
    recovery_coordinator: Arc<RecoveryCoordinator>,
    production_monitor: Arc<ProductionMonitor>,
    alert_manager: Arc<AlertManager>,
    health_checker: Arc<HealthChecker>,
    dashboard: Arc<MetricsDashboard>,
}

async fn initialize_components() -> Result<DemoComponents> {
    info!("Initializing Performance & Production components...");

    let perf_config = PerformanceConfig {
        enable_connection_pooling: true,
        max_connections_per_exex: 10,
        connection_timeout: Duration::from_secs(30),
        message_batch_size: 50,
        batch_timeout: Duration::from_millis(100),
        enable_compression: true,
        compression_threshold: 1024,
        cache_size_mb: 128,
        cache_ttl: Duration::from_secs(300),
        enable_parallel_processing: true,
        max_parallel_workers: 4,
        load_balancing_strategy: LoadBalancingStrategy::AdaptiveLoad,
    };

    let performance_optimizer = Arc::new(CrossExExOptimizer::new(perf_config));

    let cache_config = CacheConfig {
        max_size_mb: 64,
        default_ttl: Duration::from_secs(600),
        cleanup_interval: Duration::from_secs(120),
        enable_compression: true,
        compression_threshold: 512,
        enable_persistence: false,
        persistence_path: None,
    };
    let cache_manager = Arc::new(CacheManager::new(cache_config));

    let throughput_config = ThroughputConfig {
        target_tps: 500.0,
        max_batch_size: 25,
        batch_timeout: Duration::from_millis(50),
        queue_size: 5000,
        worker_count: 4,
        backpressure_threshold: 0.8,
        adaptive_batching: true,
        priority_levels: 3,
    };
    let throughput_optimizer = Arc::new(ThroughputOptimizer::new(throughput_config));

    let batching_config = BatchingConfig {
        min_batch_size: 1,
        max_batch_size: 20,
        batch_timeout: Duration::from_millis(25),
        enable_adaptive_sizing: true,
        enable_compression: true,
        compression_threshold: 512,
        max_pending_batches: 1000,
        batch_priority_levels: 3,
    };
    let batch_processor = Arc::new(BatchedMessageProcessor::new(batching_config));

    let error_config = CrossExExErrorConfig {
        base_config: ErrorHandlingConfig {
            enable_circuit_breaker: true,
            circuit_breaker_threshold: 5,
            circuit_breaker_timeout: Duration::from_secs(60),
            enable_retry: true,
            max_retry_attempts: 3,
            initial_retry_delay: Duration::from_millis(100),
            max_retry_delay: Duration::from_secs(10),
            enable_graceful_degradation: true,
            degradation_timeout: Duration::from_secs(30),
            enable_error_classification: true,
            critical_error_patterns: vec![
                "connection refused".to_string(),
                "timeout".to_string(),
                "out of memory".to_string(),
            ],
        },
        exex_health_check_interval: Duration::from_secs(15),
        max_failed_health_checks: 3,
        failover_timeout: Duration::from_secs(30),
        enable_automatic_failover: true,
        enable_load_redistribution: true,
        quarantine_duration: Duration::from_secs(300),
    };
    let error_handler = Arc::new(CrossExExErrorHandler::new(error_config));

    let circuit_breaker_config = CircuitBreakerConfig {
        failure_threshold: 5,
        recovery_timeout: Duration::from_secs(60),
        success_threshold: 3,
        request_timeout: Duration::from_secs(30),
        max_concurrent_requests: 10,
    };
    let circuit_breaker = Arc::new(CircuitBreaker::new(circuit_breaker_config));

    let retry_policy = Arc::new(
        ExponentialBackoff::new()
            .max_attempts(3)
            .initial_delay(Duration::from_millis(100))
            .max_delay(Duration::from_secs(5))
            .multiplier(2.0)
            .with_jitter(true)
            .build()
    );

    let recovery_config = RecoveryConfig {
        base_config: ErrorHandlingConfig::default(),
        auto_recovery_enabled: true,
        recovery_timeout: Duration::from_secs(300),
        max_recovery_attempts: 3,
        health_check_interval: Duration::from_secs(30),
        graceful_shutdown_timeout: Duration::from_secs(60),
        component_dependency_map: std::collections::HashMap::new(),
    };
    let recovery_coordinator = Arc::new(RecoveryCoordinator::new(recovery_config));

    let monitoring_config = MonitoringConfig {
        enable_production_monitoring: true,
        metrics_collection_interval: Duration::from_secs(10),
        health_check_interval: Duration::from_secs(30),
        alert_evaluation_interval: Duration::from_secs(60),
        dashboard_refresh_interval: Duration::from_secs(5),
        retention_period: Duration::from_secs(3600),
        enable_remote_export: false,
        export_endpoints: vec![],
        enable_real_time_alerts: true,
        alert_channels: vec![AlertChannel::Log],
    };
    let production_monitor = Arc::new(ProductionMonitor::new(monitoring_config.clone()));
    let alert_manager = Arc::new(AlertManager::new(monitoring_config.clone()));
    let health_checker = Arc::new(HealthChecker::new(monitoring_config));

    let dashboard_config = DashboardConfig {
        refresh_interval: Duration::from_secs(5),
        data_retention: Duration::from_secs(3600),
        max_chart_points: 300,
        enable_real_time_updates: true,
        enable_export: true,
        export_formats: vec!["json".to_string(), "html".to_string(), "csv".to_string()],
    };
    let dashboard = Arc::new(MetricsDashboard::new(dashboard_config));

    info!("All components initialized successfully");

    Ok(DemoComponents {
        performance_optimizer,
        cache_manager,
        throughput_optimizer,
        batch_processor,
        error_handler,
        circuit_breaker,
        retry_policy,
        recovery_coordinator,
        production_monitor,
        alert_manager,
        health_checker,
        dashboard,
    })
}

async fn run_demo(components: &DemoComponents) -> Result<()> {
    info!("=== Starting Performance & Production Demo ===");

    demo_1_performance_optimization(components).await?;
    demo_2_advanced_caching(components).await?;
    demo_3_throughput_optimization(components).await?;
    demo_4_batch_processing(components).await?;
    demo_5_error_handling(components).await?;
    demo_6_circuit_breaker(components).await?;
    demo_7_retry_mechanisms(components).await?;
    demo_8_recovery_system(components).await?;
    demo_9_production_monitoring(components).await?;
    demo_10_alerting_system(components).await?;
    demo_11_health_checking(components).await?;
    demo_12_dashboard_export(components).await?;

    info!("=== Demo completed successfully ===");
    Ok(())
}

async fn demo_1_performance_optimization(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 1: Performance Optimization ===");

    components.performance_optimizer.initialize_connection_pool("rag_exex".to_string(), 5).await?;
    components.performance_optimizer.initialize_connection_pool("memory_exex".to_string(), 5).await?;
    components.performance_optimizer.initialize_connection_pool("coordination_exex".to_string(), 3).await?;

    for i in 0..10 {
        let message = CrossExExMessage::TransactionAnalysis {
            tx_hash: reth_primitives::H256::random(),
            routing_decision: crate::shared::ai_agent::RoutingDecision::ProcessWithRAG,
            context: format!("test_context_{}", i).into_bytes(),
            timestamp: std::time::Instant::now(),
        };
        
        let target_exex = if i % 3 == 0 {
            "rag_exex"
        } else if i % 3 == 1 {
            "memory_exex"
        } else {
            "coordination_exex"
        };

        components.performance_optimizer
            .send_optimized_message(target_exex.to_string(), message)
            .await?;
    }

    components.performance_optimizer.optimize_load_balancing().await?;
    
    let metrics = components.performance_optimizer.get_performance_metrics().await?;
    info!("Performance metrics: messages_processed={}, avg_latency={}ms, cache_hit_rate={:.2}%",
        metrics.total_messages_processed,
        metrics.average_latency_ms,
        metrics.cache_hit_rate * 100.0
    );

    info!("Demo 1 completed successfully");
    Ok(())
}

async fn demo_2_advanced_caching(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 2: Advanced Caching ===");

    components.cache_manager.start_all_cleanup_tasks().await;

    let query_cache = components.cache_manager.query_cache();
    
    for i in 0..20 {
        let key = format!("query_{}", i);
        let value = format!("result_data_{}", i).into_bytes();
        
        query_cache.put(key.clone(), value, None).await?;
    }

    for i in 0..15 {
        let key = format!("query_{}", i);
        let result = query_cache.get(&key).await;
        if result.is_some() {
            info!("Cache hit for key: {}", key);
        }
    }

    let stats = components.cache_manager.get_combined_stats().await;
    info!("Cache statistics: hit_rate={:.2}%, total_memory={}MB",
        stats.query_cache.hit_rate() * 100.0,
        stats.total_memory_mb
    );

    info!("Demo 2 completed successfully");
    Ok(())
}

async fn demo_3_throughput_optimization(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 3: Throughput Optimization ===");

    let processor = |messages: Vec<String>| async move {
        info!("Processing batch of {} messages", messages.len());
        sleep(Duration::from_millis(50)).await;
        Ok::<(), eyre::Report>(())
    };

    components.throughput_optimizer.start_processing(processor).await?;

    for i in 0..50 {
        let priority = (i % 3) as u8;
        let message = PrioritizedMessage::new(
            format!("message_{}", i),
            priority,
            Address::random(),
        );
        
        components.throughput_optimizer.submit_message(message).await?;
    }

    sleep(Duration::from_secs(2)).await;

    let metrics = components.throughput_optimizer.get_metrics().await;
    info!("Throughput metrics: current_tps={:.2}, avg_tps={:.2}, queue_depth={}, worker_utilization={:.2}%",
        metrics.current_tps,
        metrics.average_tps,
        metrics.queue_depth,
        metrics.worker_utilization * 100.0
    );

    info!("Demo 3 completed successfully");
    Ok(())
}

async fn demo_4_batch_processing(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 4: Batch Processing ===");

    let processor = |messages: Vec<String>| async move {
        info!("Batch processing {} messages", messages.len());
        sleep(Duration::from_millis(30)).await;
        Ok::<(), eyre::Report>(())
    };

    components.batch_processor.start_processing(processor).await?;

    for i in 0..30 {
        let priority = (i % 3) as u8;
        let message = BatchedMessage::new(
            format!("batch_message_{}", i),
            priority,
            Address::random(),
            format!("batch_message_{}", i).len(),
        );
        
        components.batch_processor.submit_message(message).await?;
    }

    sleep(Duration::from_secs(1)).await;

    let status = components.batch_processor.get_status().await;
    info!("Batch processing status: efficiency={:.2}%, avg_batch_size={:.1}, timeout_batches={}",
        status.metrics.batch_efficiency * 100.0,
        status.metrics.average_batch_size,
        status.metrics.timeout_batches
    );

    info!("Demo 4 completed successfully");
    Ok(())
}

async fn demo_5_error_handling(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 5: Error Handling ===");

    components.error_handler.register_exex("test_exex_1".to_string()).await?;
    components.error_handler.register_exex("test_exex_2".to_string()).await?;
    components.error_handler.start_health_monitoring().await?;

    components.error_handler.record_communication_result(
        "test_exex_1",
        true,
        Duration::from_millis(50),
        None,
    ).await?;

    components.error_handler.record_communication_result(
        "test_exex_2",
        false,
        Duration::from_millis(5000),
        Some("Connection timeout".to_string()),
    ).await?;

    components.error_handler.record_communication_result(
        "test_exex_2",
        false,
        Duration::from_millis(5000),
        Some("Connection refused".to_string()),
    ).await?;

    sleep(Duration::from_millis(100)).await;

    let status1 = components.error_handler.get_exex_status("test_exex_1").await;
    let status2 = components.error_handler.get_exex_status("test_exex_2").await;
    
    info!("ExEx statuses: test_exex_1={:?}, test_exex_2={:?}", status1, status2);

    let metrics = components.error_handler.get_metrics().await;
    info!("Error handling metrics: total_errors={}, recovery_attempts={}",
        metrics.total_errors, metrics.recovery_attempts);

    info!("Demo 5 completed successfully");
    Ok(())
}

async fn demo_6_circuit_breaker(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 6: Circuit Breaker ===");

    for i in 0..3 {
        let result = components.circuit_breaker.call(async {
            if i < 2 {
                Ok::<i32, &str>(42)
            } else {
                Err("Service unavailable")
            }
        }).await;
        
        match result {
            Ok(value) => info!("Circuit breaker call {} succeeded: {}", i, value),
            Err(e) => warn!("Circuit breaker call {} failed: {:?}", i, e),
        }
    }

    for _ in 0..6 {
        let result = components.circuit_breaker.call(async {
            Err::<i32, &str>("Persistent failure")
        }).await;
        
        if let Err(e) = result {
            warn!("Circuit breaker failure: {:?}", e);
        }
    }

    let result = components.circuit_breaker.call(async {
        Ok::<i32, &str>(100)
    }).await;

    match result {
        Ok(value) => info!("Circuit breaker call after opening succeeded: {}", value),
        Err(e) => warn!("Circuit breaker rejected request: {:?}", e),
    }

    let metrics = components.circuit_breaker.get_metrics().await;
    info!("Circuit breaker metrics: state={}, total_requests={}, rejected_requests={}",
        metrics.state, metrics.total_requests, metrics.rejected_requests);

    info!("Demo 6 completed successfully");
    Ok(())
}

async fn demo_7_retry_mechanisms(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 7: Retry Mechanisms ===");

    let attempt_counter = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter_clone = attempt_counter.clone();

    let result = components.retry_policy.execute(move || {
        let counter = counter_clone.clone();
        Box::pin(async move {
            let attempt = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if attempt < 2 {
                Err("Temporary failure")
            } else {
                Ok(format!("Success after {} attempts", attempt + 1))
            }
        })
    }).await;

    match result {
        Ok(value) => info!("Retry succeeded: {}", value),
        Err(e) => error!("Retry failed: {:?}", e),
    }

    let metrics = components.retry_policy.get_metrics().await;
    info!("Retry metrics: total_attempts={}, successful_retries={}, failed_retries={}",
        metrics.total_attempts, metrics.successful_retries, metrics.failed_retries);

    info!("Demo 7 completed successfully");
    Ok(())
}

async fn demo_8_recovery_system(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 8: Recovery System ===");

    let component_info = ComponentInfo {
        component_id: "test_component".to_string(),
        status: ComponentStatus::Healthy,
        last_health_check: std::time::Instant::now(),
        dependencies: vec!["database".to_string()],
        recovery_attempts: 0,
        total_failures: 0,
        last_recovery_attempt: None,
    };

    components.recovery_coordinator.register_component(component_info).await?;
    components.recovery_coordinator.start_recovery_processing().await?;
    components.recovery_coordinator.start_health_monitoring().await?;

    let error_context = monmouth_rag_memory_exex::error_handling::ErrorContext {
        error_id: uuid::Uuid::new_v4().to_string(),
        severity: monmouth_rag_memory_exex::error_handling::ErrorSeverity::High,
        category: monmouth_rag_memory_exex::error_handling::ErrorCategory::Network,
        message: "Connection failed to test_component".to_string(),
        timestamp: chrono::Utc::now().timestamp() as u64,
        component: "test_component".to_string(),
        metadata: serde_json::json!({}),
    };

    components.recovery_coordinator.handle_error(error_context).await?;

    sleep(Duration::from_millis(500)).await;

    let recovery_metrics = components.recovery_coordinator.get_recovery_metrics().await;
    info!("Recovery metrics: total_attempts={}, successful_recoveries={}, components_recovered={}",
        recovery_metrics.total_recovery_attempts,
        recovery_metrics.successful_recoveries,
        recovery_metrics.components_recovered
    );

    info!("Demo 8 completed successfully");
    Ok(())
}

async fn demo_9_production_monitoring(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 9: Production Monitoring ===");

    let test_components = vec![
        ComponentHealth {
            component_id: "rag_exex".to_string(),
            status: monmouth_rag_memory_exex::monitoring::production::ComponentStatus::Healthy,
            health_score: 1.0,
            last_check: chrono::Utc::now().timestamp() as u64,
            metrics: ComponentMetrics {
                response_time_ms: 25.0,
                throughput: 500.0,
                error_count: 0,
                resource_utilization: 0.3,
            },
            dependencies: vec!["vector_store".to_string()],
        },
        ComponentHealth {
            component_id: "memory_exex".to_string(),
            status: monmouth_rag_memory_exex::monitoring::production::ComponentStatus::Warning,
            health_score: 0.7,
            last_check: chrono::Utc::now().timestamp() as u64,
            metrics: ComponentMetrics {
                response_time_ms: 75.0,
                throughput: 300.0,
                error_count: 2,
                resource_utilization: 0.8,
            },
            dependencies: vec!["database".to_string()],
        },
    ];

    for component in test_components {
        components.production_monitor.register_component(component).await?;
    }

    components.production_monitor.start_monitoring().await?;

    sleep(Duration::from_secs(2)).await;

    let monitoring_metrics = components.production_monitor.get_metrics().await?;
    info!("Production monitoring metrics: health_score={:.2}, healthy_components={}, degraded_components={}",
        monitoring_metrics.system_health_score,
        monitoring_metrics.healthy_components,
        monitoring_metrics.degraded_components
    );

    let sla_metrics = components.production_monitor.get_sla_metrics().await;
    info!("SLA metrics: availability={:.2}%, error_budget_remaining={:.2}%",
        sla_metrics.availability_percentage,
        sla_metrics.error_budget_remaining * 100.0
    );

    info!("Demo 9 completed successfully");
    Ok(())
}

async fn demo_10_alerting_system(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 10: Alerting System ===");

    components.alert_manager.start().await?;

    let test_rule = AlertRule {
        name: "demo_high_latency".to_string(),
        description: "Demo alert for high latency".to_string(),
        condition: "latency_p95_ms > 100.0".to_string(),
        severity: AlertSeverity::Warning,
        evaluation_window: Duration::from_secs(60),
        cooldown: Duration::from_secs(300),
        enabled: true,
    };

    components.alert_manager.add_rule(test_rule).await?;

    let test_metrics = monmouth_rag_memory_exex::monitoring::production::ProductionMetrics {
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

    components.alert_manager.evaluate_metrics(&test_metrics).await?;

    sleep(Duration::from_millis(100)).await;

    let active_alerts = components.alert_manager.get_active_alerts().await;
    info!("Active alerts: {}", active_alerts.len());
    
    for alert in &active_alerts {
        info!("Alert: {} - {} ({})", alert.rule_name, alert.message, alert.severity.to_string());
    }

    let alert_metrics = components.alert_manager.get_alert_metrics().await;
    info!("Alert metrics: total_alerts={}, active_alerts={}",
        alert_metrics.total_alerts, alert_metrics.active_alerts);

    info!("Demo 10 completed successfully");
    Ok(())
}

async fn demo_11_health_checking(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 11: Health Checking ===");

    components.health_checker.start().await?;

    let test_components = vec!["rag_component", "memory_component", "coordination_component"];

    for component in &test_components {
        let result = components.health_checker.perform_health_check(component).await?;
        info!("Health check for {}: status={:?}, score={:.2}, response_time={}ms",
            component, result.status, result.overall_score, result.response_time_ms);
        
        for check in &result.checks_performed {
            info!("  - {}: {:?} ({}ms)", check.name, check.status, check.duration_ms);
        }
    }

    sleep(Duration::from_millis(100)).await;

    let health_metrics = components.health_checker.get_health_metrics().await;
    info!("Health metrics: total_components={}, healthy={}, degraded={}, unhealthy={}",
        health_metrics.total_components,
        health_metrics.healthy_components,
        health_metrics.degraded_components,
        health_metrics.unhealthy_components
    );

    info!("Demo 11 completed successfully");
    Ok(())
}

async fn demo_12_dashboard_export(components: &DemoComponents) -> Result<()> {
    info!("\n=== Demo 12: Dashboard Export ===");

    components.dashboard.start().await?;

    let monitoring_metrics = monmouth_rag_memory_exex::monitoring::MonitoringMetrics {
        system_health_score: 0.85,
        total_components_monitored: 5,
        healthy_components: 4,
        degraded_components: 1,
        failed_components: 0,
        total_alerts_fired: 3,
        active_alerts: 1,
        sla_compliance: 0.998,
        average_response_time_ms: 45.0,
        throughput_ops_per_second: 450.0,
        error_rate: 0.5,
    };

    let production_metrics = Some(monmouth_rag_memory_exex::monitoring::production::ProductionMetrics {
        timestamp: chrono::Utc::now().timestamp() as u64,
        uptime_seconds: 7200,
        cpu_usage_percent: 35.0,
        memory_usage_mb: 1536,
        disk_usage_percent: 25.0,
        network_bytes_in: 2_000_000,
        network_bytes_out: 1_500_000,
        active_connections: 120,
        request_rate: 450.0,
        error_rate: 0.5,
        latency_p50_ms: 30.0,
        latency_p95_ms: 85.0,
        latency_p99_ms: 150.0,
    });

    let alert_metrics = monmouth_rag_memory_exex::monitoring::alerts::AlertMetrics {
        total_alerts: 3,
        active_alerts: 1,
        alerts_by_severity: std::collections::HashMap::new(),
        alert_resolution_time_avg_seconds: 120.0,
        false_positive_rate: 0.1,
        escalations: 0,
    };

    let health_metrics = monmouth_rag_memory_exex::monitoring::health::HealthMetrics {
        total_components: 5,
        healthy_components: 4,
        degraded_components: 1,
        unhealthy_components: 0,
        unknown_components: 0,
        average_response_time_ms: 25.0,
        total_checks_performed: 150,
        failed_checks: 5,
        health_check_success_rate: 96.7,
    };

    let sla_metrics = monmouth_rag_memory_exex::monitoring::production::SLAMetrics {
        availability_percentage: 99.8,
        performance_target_met: true,
        error_budget_remaining: 0.75,
        mttr_minutes: 8.5,
        mtbf_hours: 72.0,
    };

    let active_alerts = vec![];
    let component_health = std::collections::HashMap::new();

    components.dashboard.update_dashboard_data(
        monitoring_metrics,
        production_metrics,
        alert_metrics,
        health_metrics,
        sla_metrics,
        active_alerts,
        component_health,
    ).await?;

    let json_export = components.dashboard.export_dashboard_data("json").await?;
    info!("Dashboard JSON export: {} characters", json_export.len());

    let csv_export = components.dashboard.export_dashboard_data("csv").await?;
    info!("Dashboard CSV export: {} characters", csv_export.len());

    let html_export = components.dashboard.export_dashboard_data("html").await?;
    info!("Dashboard HTML export: {} characters", html_export.len());

    info!("Demo 12 completed successfully");
    Ok(())
}

async fn run_monitoring_task(components: &DemoComponents) -> Result<()> {
    let mut interval = interval(Duration::from_secs(10));
    
    for _ in 0..6 {
        interval.tick().await;
        
        let perf_metrics = components.performance_optimizer.get_performance_metrics().await?;
        let cache_stats = components.cache_manager.get_combined_stats().await;
        let throughput_metrics = components.throughput_optimizer.get_metrics().await;
        
        info!("System status update:");
        info!("  Performance: {}ms avg latency, {:.1}% cache hit rate",
            perf_metrics.average_latency_ms, cache_stats.query_cache.hit_rate() * 100.0);
        info!("  Throughput: {:.1} TPS, {} queue depth, {:.1}% worker utilization",
            throughput_metrics.current_tps, throughput_metrics.queue_depth, 
            throughput_metrics.worker_utilization * 100.0);
        
        if let Some(dashboard_data) = components.dashboard.get_current_dashboard().await {
            info!("  Health: {:.1}% system health, {} active alerts",
                dashboard_data.system_overview.health_score * 100.0,
                dashboard_data.active_alerts.len());
        }
    }
    
    Ok(())
}