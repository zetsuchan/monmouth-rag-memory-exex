# Integration Guide - Monmouth RAG x Memory ExEx

This guide provides comprehensive instructions for setting up, running, and integrating the Monmouth RAG x Memory ExEx system in production environments.

## Table of Contents

1. [Overview](#overview)
2. [System Architecture](#system-architecture)
3. [Prerequisites](#prerequisites)
4. [Installation & Setup](#installation--setup)
5. [Configuration](#configuration)
6. [Running the System](#running-the-system)
7. [Integration Examples](#integration-examples)
8. [Performance Tuning](#performance-tuning)
9. [Monitoring & Observability](#monitoring--observability)
10. [Troubleshooting](#troubleshooting)
11. [Production Deployment](#production-deployment)

## Overview

The Monmouth RAG x Memory ExEx system provides enterprise-grade AI agent coordination with:

- **RAG ExEx**: Real-time context retrieval and knowledge graph management
- **Memory ExEx**: Persistent agent memory with state synchronization
- **Cross-ExEx Communication**: High-performance message routing and coordination
- **State Synchronization**: ALH-based state coordination with reorg handling
- **Performance Optimization**: Connection pooling, caching, and throughput optimization
- **Error Handling**: Circuit breakers, retry policies, and automatic recovery
- **Production Monitoring**: Comprehensive metrics, alerting, and health checking

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Reth Execution Extensions                    │
├─────────────────────────┬───────────────────────────────────────┤
│      RAG ExEx          │           Memory ExEx                 │
│                        │                                       │
│ ┌─────────────────────┐ │ ┌───────────────────────────────────┐ │
│ │ Context Retrieval   │ │ │ Agent Memory Store                │ │
│ │ Vector Store        │ │ │ Checkpointing                     │ │
│ │ Knowledge Graph     │ │ │ State Synchronization             │ │
│ │ Embedding Pipeline  │ │ │ Memory Lattice Hash               │ │
│ └─────────────────────┘ │ └───────────────────────────────────┘ │
└─────────────────────────┴───────────────────────────────────────┤
│                    Shared Infrastructure                        │
│                                                                 │
│ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ │
│ │Performance  │ │Error        │ │Monitoring   │ │State Sync   │ │
│ │Optimization │ │Handling     │ │& Alerting   │ │& Recovery   │ │
│ └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

## Prerequisites

### System Requirements

- **Operating System**: Linux (Ubuntu 20.04+, CentOS 8+) or macOS
- **Memory**: Minimum 16GB RAM, 32GB+ recommended for production
- **Storage**: 100GB+ SSD storage (scales with agent count and memory retention)
- **CPU**: 8+ cores recommended for production workloads
- **Network**: High-bandwidth connection for cross-ExEx communication

### Software Dependencies

- **Rust**: 1.70+ with stable toolchain
- **Reth**: v0.2.0-beta or compatible version
- **Database**: PostgreSQL 14+ (for persistence) or MDBX (embedded)
- **Vector Database**: ChromaDB v0.2+ (for RAG functionality)
- **Monitoring**: Prometheus + Grafana (optional but recommended)

### Development Tools

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install additional tools
cargo install cargo-watch cargo-nextest
```

## Installation & Setup

### 1. Clone and Build

```bash
# Clone the repository
git clone https://github.com/your-org/monmouth-rag-memory-exex.git
cd monmouth-rag-memory-exex

# Build the project
cargo build --release

# Run tests to verify installation
cargo test --all
```

### 2. Database Setup

#### Option A: Embedded MDBX (Development)
```bash
# No additional setup required - databases are created automatically
export MEMORY_STORE_PATH="./data/memory.mdb"
export RAG_STORE_PATH="./data/rag.mdb"
```

#### Option B: PostgreSQL (Production)
```bash
# Install PostgreSQL
sudo apt-get install postgresql postgresql-contrib

# Create database and user
sudo -u postgres createdb monmouth_exex
sudo -u postgres createuser monmouth_user --pwprompt

# Set environment variables
export DATABASE_URL="postgresql://monmouth_user:password@localhost/monmouth_exex"
```

### 3. Vector Store Setup

```bash
# Install ChromaDB
pip install chromadb

# Start ChromaDB server
chroma run --host 0.0.0.0 --port 8000 --path ./data/chroma

# Set environment variable
export CHROMA_URL="http://localhost:8000"
```

## Configuration

### Environment Variables

Create a `.env` file in the project root:

```bash
# Core Configuration
RUST_LOG=info
METRICS_PORT=9090
HEALTH_CHECK_PORT=8080

# Database Configuration
MEMORY_STORE_PATH="./data/memory.mdb"
RAG_STORE_PATH="./data/rag.mdb"
CHROMA_URL="http://localhost:8000"

# Performance Configuration
MAX_CONNECTIONS_PER_EXEX=20
MESSAGE_BATCH_SIZE=100
CACHE_SIZE_MB=512
ENABLE_COMPRESSION=true

# Monitoring Configuration
ENABLE_PRODUCTION_MONITORING=true
METRICS_COLLECTION_INTERVAL=5
HEALTH_CHECK_INTERVAL=15
ALERT_EVALUATION_INTERVAL=30

# Error Handling Configuration
ENABLE_CIRCUIT_BREAKER=true
CIRCUIT_BREAKER_THRESHOLD=5
MAX_RETRY_ATTEMPTS=3
ENABLE_AUTOMATIC_FAILOVER=true
```

### Configuration Files

#### `config/performance.toml`
```toml
[performance]
enable_connection_pooling = true
max_connections_per_exex = 20
connection_timeout = "30s"
message_batch_size = 100
batch_timeout = "50ms"
enable_compression = true
compression_threshold = 1024
cache_size_mb = 512
cache_ttl = "600s"
enable_parallel_processing = true
max_parallel_workers = 8
load_balancing_strategy = "AdaptiveLoad"
```

#### `config/monitoring.toml`
```toml
[monitoring]
enable_production_monitoring = true
metrics_collection_interval = "5s"
health_check_interval = "15s"
alert_evaluation_interval = "30s"
dashboard_refresh_interval = "2s"
retention_period = "3600s"
enable_real_time_alerts = true

[[monitoring.alert_channels]]
type = "Log"

[[monitoring.alert_channels]]
type = "Slack"
webhook_url = "https://hooks.slack.com/your-webhook-url"
```

#### `config/error_handling.toml`
```toml
[error_handling]
enable_circuit_breaker = true
circuit_breaker_threshold = 5
circuit_breaker_timeout = "60s"
enable_retry = true
max_retry_attempts = 3
initial_retry_delay = "100ms"
max_retry_delay = "10s"
enable_graceful_degradation = true
enable_automatic_failover = true
quarantine_duration = "300s"
```

## Running the System

### Development Mode

```bash
# Start in development mode with auto-reload
cargo watch -x 'run --example integrated_agent_example'

# Run integration tests
cargo test tests::integration --release

# Run performance benchmarks
cargo bench --bench integration_benchmarks
```

### Production Mode

```bash
# Build optimized release
cargo build --release --all-features

# Run the integrated system
./target/release/monmouth-rag-memory-exex \
  --config config/ \
  --data-dir ./data \
  --log-level info \
  --metrics-port 9090

# Run with systemd (recommended)
sudo systemctl start monmouth-exex
sudo systemctl enable monmouth-exex
```

### Docker Deployment

```dockerfile
# Dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/monmouth-rag-memory-exex /usr/local/bin/
COPY config/ /app/config/

EXPOSE 8080 9090
CMD ["monmouth-rag-memory-exex", "--config", "/app/config"]
```

```bash
# Build and run with Docker
docker build -t monmouth-exex .
docker run -p 8080:8080 -p 9090:9090 -v ./data:/app/data monmouth-exex
```

## Integration Examples

### Basic Agent Transaction

```rust
use monmouth_rag_memory_exex::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the integrated system
    let demo = IntegratedAgentDemo::new().await?;
    
    // Create and register an agent
    let agent_id = "my_agent_001";
    demo.agent_state_manager.register_agent(
        agent_id.to_string(), 
        Address::random()
    ).await?;
    
    // Process a transaction with full integration
    demo.process_agent_transaction(agent_id, "user_transaction").await?;
    
    Ok(())
}
```

### RAG Context Retrieval

```rust
// Query agent context
let context = context_retriever.retrieve_context(
    "agent_001",
    "What are the latest DeFi trends?",
    5 // max results
).await?;

// Store context in agent memory
let memory_data = format!("Context: {:?}", context).into_bytes();
let memory_hash = agent_memory_integration.store_agent_memory(
    "agent_001",
    "working",
    memory_data
).await?;
```

### Cross-ExEx Communication

```rust
// Send message between ExExs
let message = CrossExExMessage::TransactionAnalysis {
    tx_hash: H256::random(),
    routing_decision: RoutingDecision::ProcessWithRAG,
    context: "transaction context".as_bytes().to_vec(),
    timestamp: Instant::now(),
};

performance_optimizer.send_optimized_message(
    "memory_exex".to_string(),
    message
).await?;
```

### State Synchronization

```rust
// Create ALH query for state sync
let query = ALHQuery {
    query_id: "sync_query_001".to_string(),
    query_type: QueryType::MemoryState,
    agent_id: "agent_001".to_string(),
    parameters: serde_json::json!({
        "memory_hash": memory_hash,
        "sync_type": "incremental"
    }),
    timestamp: Utc::now().timestamp() as u64,
};

let result = alh_coordinator.process_query(query).await?;
```

## Performance Tuning

### Memory Optimization

```bash
# Configure cache sizes based on available memory
export CACHE_SIZE_MB=1024           # For 16GB+ RAM systems
export AGENT_CACHE_SIZE=10000       # Number of agents to cache
export CONTEXT_CACHE_SIZE=50000     # Number of contexts to cache
```

### Connection Pooling

```toml
[performance.connection_pools]
rag_exex_pool_size = 20
memory_exex_pool_size = 20
coordination_pool_size = 10
max_idle_connections = 5
connection_timeout = "30s"
```

### Batch Processing

```toml
[performance.batching]
message_batch_size = 100
batch_timeout = "50ms"
enable_adaptive_sizing = true
compression_threshold = 1024
max_pending_batches = 1000
```

### Throughput Optimization

```toml
[performance.throughput]
target_tps = 1000.0
max_batch_size = 50
queue_size = 10000
worker_count = 8
backpressure_threshold = 0.8
adaptive_batching = true
priority_levels = 3
```

## Monitoring & Observability

### Metrics Endpoints

- **Health Check**: `http://localhost:8080/health`
- **Metrics**: `http://localhost:9090/metrics` (Prometheus format)
- **Dashboard**: `http://localhost:8080/dashboard`

### Key Metrics to Monitor

```prometheus
# System Health
monmouth_system_health_score
monmouth_components_healthy_total
monmouth_components_degraded_total

# Performance
monmouth_transaction_latency_ms_histogram
monmouth_throughput_tps
monmouth_cache_hit_rate
monmouth_connection_pool_utilization

# Error Rates
monmouth_errors_total
monmouth_circuit_breaker_state
monmouth_retry_attempts_total

# Resource Usage
monmouth_memory_usage_mb
monmouth_cpu_usage_percent
monmouth_disk_usage_percent
```

### Grafana Dashboard

Import the provided dashboard configuration:

```json
{
  "dashboard": {
    "title": "Monmouth ExEx Monitoring",
    "panels": [
      {
        "title": "System Health Score",
        "type": "gauge",
        "targets": [
          {
            "expr": "monmouth_system_health_score",
            "legendFormat": "Health Score"
          }
        ]
      },
      {
        "title": "Transaction Latency",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, monmouth_transaction_latency_ms_histogram)",
            "legendFormat": "95th Percentile"
          }
        ]
      }
    ]
  }
}
```

### Alerting Rules

```yaml
# alerts.yml
groups:
  - name: monmouth_exex
    rules:
      - alert: HighLatency
        expr: histogram_quantile(0.95, monmouth_transaction_latency_ms_histogram) > 100
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "High transaction latency detected"
          
      - alert: SystemHealthDegraded
        expr: monmouth_system_health_score < 0.8
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "System health score below threshold"
          
      - alert: HighErrorRate
        expr: rate(monmouth_errors_total[5m]) > 0.1
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "Error rate above 10%"
```

## Troubleshooting

### Common Issues

#### 1. High Memory Usage

```bash
# Check memory usage
curl http://localhost:8080/health | jq '.memory_usage_mb'

# Reduce cache sizes
export CACHE_SIZE_MB=256
export AGENT_CACHE_SIZE=5000
```

#### 2. Connection Pool Exhaustion

```bash
# Check connection pool status
curl http://localhost:9090/metrics | grep connection_pool

# Increase pool size
export MAX_CONNECTIONS_PER_EXEX=30
```

#### 3. Slow Context Retrieval

```bash
# Check vector store performance
curl http://localhost:8000/health

# Check embedding cache hit rate
curl http://localhost:9090/metrics | grep embedding_cache_hit_rate
```

#### 4. Circuit Breaker Activation

```bash
# Check circuit breaker status
curl http://localhost:9090/metrics | grep circuit_breaker_state

# Reset circuit breaker (if safe)
curl -X POST http://localhost:8080/admin/circuit_breaker/reset
```

### Log Analysis

```bash
# View real-time logs
tail -f logs/monmouth-exex.log | grep ERROR

# Search for specific errors
grep "timeout" logs/monmouth-exex.log | tail -10

# Monitor performance logs
grep "latency_ms" logs/monmouth-exex.log | awk '{print $NF}' | sort -n
```

### Performance Debugging

```bash
# Run integration tests with profiling
cargo test --release --test integration -- --nocapture

# Run benchmarks
cargo bench --bench integration_benchmarks

# Profile memory usage
valgrind --tool=massif ./target/release/monmouth-rag-memory-exex
```

## Production Deployment

### High Availability Setup

```yaml
# kubernetes/deployment.yml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: monmouth-exex
spec:
  replicas: 3
  selector:
    matchLabels:
      app: monmouth-exex
  template:
    metadata:
      labels:
        app: monmouth-exex
    spec:
      containers:
      - name: monmouth-exex
        image: monmouth-exex:latest
        ports:
        - containerPort: 8080
        - containerPort: 9090
        env:
        - name: CACHE_SIZE_MB
          value: "1024"
        - name: ENABLE_PRODUCTION_MONITORING
          value: "true"
        resources:
          requests:
            memory: "2Gi"
            cpu: "1000m"
          limits:
            memory: "4Gi"
            cpu: "2000m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5
```

### Load Balancing

```nginx
# nginx.conf
upstream monmouth_exex {
    least_conn;
    server 10.0.1.10:8080 max_fails=3 fail_timeout=30s;
    server 10.0.1.11:8080 max_fails=3 fail_timeout=30s;
    server 10.0.1.12:8080 max_fails=3 fail_timeout=30s;
}

server {
    listen 80;
    server_name monmouth-exex.example.com;
    
    location / {
        proxy_pass http://monmouth_exex;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_connect_timeout 30s;
        proxy_send_timeout 30s;
        proxy_read_timeout 30s;
    }
    
    location /metrics {
        proxy_pass http://monmouth_exex;
        allow 10.0.0.0/8;
        deny all;
    }
}
```

### Backup and Recovery

```bash
#!/bin/bash
# backup.sh

# Backup memory store
cp -r ./data/memory.mdb ./backups/memory_$(date +%Y%m%d_%H%M%S).mdb

# Backup configuration
tar -czf ./backups/config_$(date +%Y%m%d_%H%M%S).tar.gz config/

# Export metrics
curl http://localhost:9090/metrics > ./backups/metrics_$(date +%Y%m%d_%H%M%S).txt

# Cleanup old backups (keep last 7 days)
find ./backups -type f -mtime +7 -delete
```

### Security Considerations

1. **Network Security**: Use TLS/SSL for all communications
2. **Access Control**: Implement proper authentication and authorization
3. **Data Encryption**: Encrypt sensitive data at rest and in transit
4. **Resource Limits**: Set appropriate resource limits to prevent DoS
5. **Regular Updates**: Keep dependencies updated for security patches

### Scaling Guidelines

- **Horizontal Scaling**: Deploy multiple instances behind a load balancer
- **Vertical Scaling**: Increase memory and CPU based on workload
- **Database Scaling**: Use read replicas for heavy read workloads
- **Cache Scaling**: Implement distributed caching for large deployments

For more advanced topics and enterprise features, contact the Monmouth team at support@monmouth.ai.