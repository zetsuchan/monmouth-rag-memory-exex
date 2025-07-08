//! Agent Reputation Metrics Aggregation
//! 
//! Computes on-chain agent reputation metrics by aggregating historical performance
//! data from Memory ExEx logs, transaction outcomes, and cross-chain activities.

use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

/// Agent reputation aggregator
#[derive(Debug)]
pub struct ReputationAggregator {
    /// Data scanner for chain history
    data_scanner: Arc<HistoricalDataScanner>,
    /// Metric calculators
    calculators: Arc<MetricCalculators>,
    /// Cross-chain data aggregator
    cross_chain_aggregator: Arc<CrossChainAggregator>,
    /// Aggregated results cache
    results_cache: Arc<RwLock<HashMap<String, AggregatedMetrics>>>,
}

/// Scans historical blockchain data
#[derive(Debug)]
pub struct HistoricalDataScanner {
    /// Event log scanner
    event_scanner: Arc<EventLogScanner>,
    /// Transaction outcome analyzer
    tx_analyzer: Arc<TransactionAnalyzer>,
    /// Memory log processor
    memory_processor: Arc<MemoryLogProcessor>,
}

/// Scans and indexes event logs
#[derive(Debug)]
pub struct EventLogScanner {
    /// Indexed events by agent
    agent_events: Arc<RwLock<HashMap<String, Vec<AgentEvent>>>>,
    /// Event signatures to track
    tracked_signatures: HashMap<[u8; 32], EventType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub block_number: u64,
    pub timestamp: u64,
    pub event_type: EventType,
    pub data: EventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    IntentExecuted,
    IntentFailed,
    YieldGenerated,
    LossIncurred,
    GasUsed,
    SlippageRecorded,
    CrossChainAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventData {
    IntentOutcome {
        intent_id: String,
        success: bool,
        gas_used: u128,
        value_moved: u128,
    },
    YieldData {
        amount: u128,
        token: [u8; 20],
        apy: f64,
    },
    SlippageData {
        expected_price: u128,
        actual_price: u128,
        slippage_bps: u16,
    },
    CrossChainData {
        chain_id: String,
        action: String,
        status: String,
    },
}

/// Analyzes transaction outcomes
#[derive(Debug)]
pub struct TransactionAnalyzer {
    /// Transaction receipts by agent
    receipts: Arc<RwLock<HashMap<String, Vec<TxReceipt>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_hash: [u8; 32],
    pub block_number: u64,
    pub from: [u8; 20],
    pub to: Option<[u8; 20]>,
    pub success: bool,
    pub gas_used: u128,
    pub gas_price: u128,
}

/// Processes memory logs for insights
#[derive(Debug)]
pub struct MemoryLogProcessor {
    /// Agent memory snapshots
    memory_snapshots: Arc<RwLock<HashMap<String, Vec<MemorySnapshot>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub agent_id: String,
    pub block_number: u64,
    pub memory_metrics: MemoryMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub decision_count: u64,
    pub planning_depth: u8,
    pub context_utilization: f32,
    pub learning_rate: f32,
}

/// Calculates various metrics
#[derive(Debug)]
pub struct MetricCalculators {
    /// ROI/APY calculator
    roi_calculator: Arc<ROICalculator>,
    /// Success rate calculator
    success_calculator: Arc<SuccessRateCalculator>,
    /// Gas efficiency calculator
    gas_calculator: Arc<GasEfficiencyCalculator>,
    /// Slippage analyzer
    slippage_analyzer: Arc<SlippageAnalyzer>,
}

/// Calculates ROI and APY metrics
#[derive(Debug)]
pub struct ROICalculator;

/// Calculates success rates
#[derive(Debug)]
pub struct SuccessRateCalculator;

/// Analyzes gas efficiency
#[derive(Debug)]
pub struct GasEfficiencyCalculator;

/// Analyzes trading slippage
#[derive(Debug)]
pub struct SlippageAnalyzer;

/// Aggregates cross-chain data
#[derive(Debug)]
pub struct CrossChainAggregator {
    /// Chain-specific data sources
    chain_sources: HashMap<String, Arc<dyn ChainDataSource>>,
    /// Unified metrics
    unified_metrics: Arc<RwLock<HashMap<String, CrossChainMetrics>>>,
}

/// Chain data source trait
pub trait ChainDataSource: Send + Sync {
    fn get_agent_metrics(&self, agent_id: &str, time_range: &TimeRange) -> Result<ChainMetrics>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainMetrics {
    pub chain_id: String,
    pub total_value_locked: u128,
    pub yield_generated: u128,
    pub transactions_count: u64,
    pub gas_spent: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainMetrics {
    pub agent_id: String,
    pub chains: HashMap<String, ChainMetrics>,
    pub total_cross_chain_value: u128,
    pub bridge_fees_paid: u128,
}

/// Time range for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start_block: u64,
    pub end_block: u64,
    pub start_timestamp: Option<u64>,
    pub end_timestamp: Option<u64>,
}

/// Aggregated metrics result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedMetrics {
    pub agent_id: String,
    pub time_range: TimeRange,
    pub performance_metrics: PerformanceMetrics,
    pub efficiency_metrics: EfficiencyMetrics,
    pub reliability_metrics: ReliabilityMetrics,
    pub cross_chain_metrics: Option<CrossChainMetrics>,
    pub reputation_score: ReputationScore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub total_value_managed: u128,
    pub total_yield_generated: u128,
    pub average_apy: f64,
    pub best_apy: f64,
    pub worst_apy: f64,
    pub sharpe_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EfficiencyMetrics {
    pub total_gas_used: u128,
    pub average_gas_per_intent: u128,
    pub gas_optimization_score: f32,
    pub average_slippage_bps: u16,
    pub execution_speed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityMetrics {
    pub total_intents: u64,
    pub successful_intents: u64,
    pub failed_intents: u64,
    pub success_rate: f64,
    pub uptime_percentage: f64,
    pub error_recovery_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationScore {
    pub overall_score: u32, // 0-1000
    pub performance_score: u32,
    pub efficiency_score: u32,
    pub reliability_score: u32,
    pub cross_chain_score: u32,
    pub tier: ReputationTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReputationTier {
    Unrated,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
}

/// Query request for metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsQuery {
    pub agent_id: String,
    pub metric_types: Vec<MetricType>,
    pub time_range: TimeRange,
    pub include_cross_chain: bool,
    pub chains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricType {
    ROI,
    APY,
    GasUsage,
    SuccessRate,
    Slippage,
    CrossChainActivity,
    All,
}

/// Proof of metrics computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsProof {
    pub computation_hash: [u8; 32],
    pub data_sources: Vec<DataSource>,
    pub proof_type: ProofType,
    pub proof_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub source_type: String,
    pub block_range: (u64, u64),
    pub events_count: u64,
    pub data_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProofType {
    ZKAggregation,
    MerkleCommitment,
    AttestationBased,
}

impl ReputationAggregator {
    pub fn new(
        data_scanner: Arc<HistoricalDataScanner>,
        calculators: Arc<MetricCalculators>,
        cross_chain_aggregator: Arc<CrossChainAggregator>,
    ) -> Self {
        Self {
            data_scanner,
            calculators,
            cross_chain_aggregator,
            results_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Compute agent metrics
    pub async fn compute_agent_metrics(
        &self,
        query: MetricsQuery,
    ) -> Result<(AggregatedMetrics, MetricsProof)> {
        // Check cache
        let cache_key = format!("{}_{}_{}", query.agent_id, query.time_range.start_block, query.time_range.end_block);
        if let Some(cached) = self.get_cached_metrics(&cache_key).await {
            return Ok((cached, self.generate_proof(&cached).await?));
        }
        
        // Scan historical data
        let events = self.data_scanner.scan_agent_events(&query.agent_id, &query.time_range).await?;
        let receipts = self.data_scanner.scan_transactions(&query.agent_id, &query.time_range).await?;
        let memory_data = self.data_scanner.scan_memory_logs(&query.agent_id, &query.time_range).await?;
        
        // Calculate metrics
        let performance = self.calculators.calculate_performance(&events, &receipts).await?;
        let efficiency = self.calculators.calculate_efficiency(&receipts, &events).await?;
        let reliability = self.calculators.calculate_reliability(&receipts, &events).await?;
        
        // Cross-chain metrics if requested
        let cross_chain = if query.include_cross_chain {
            Some(self.cross_chain_aggregator.aggregate_cross_chain(&query.agent_id, &query.time_range).await?)
        } else {
            None
        };
        
        // Calculate reputation score
        let reputation_score = self.calculate_reputation_score(
            &performance,
            &efficiency,
            &reliability,
            &cross_chain,
        );
        
        let metrics = AggregatedMetrics {
            agent_id: query.agent_id,
            time_range: query.time_range,
            performance_metrics: performance,
            efficiency_metrics: efficiency,
            reliability_metrics: reliability,
            cross_chain_metrics: cross_chain,
            reputation_score,
        };
        
        // Cache results
        self.cache_metrics(&cache_key, &metrics).await?;
        
        // Generate proof
        let proof = self.generate_proof(&metrics).await?;
        
        Ok((metrics, proof))
    }
    
    /// Calculate overall reputation score
    fn calculate_reputation_score(
        &self,
        performance: &PerformanceMetrics,
        efficiency: &EfficiencyMetrics,
        reliability: &ReliabilityMetrics,
        cross_chain: &Option<CrossChainMetrics>,
    ) -> ReputationScore {
        // Performance score (0-250)
        let perf_score = (performance.average_apy.min(50.0) * 5.0) as u32;
        
        // Efficiency score (0-250)
        let eff_score = ((1.0 - (efficiency.average_slippage_bps as f64 / 10000.0)) * 250.0) as u32;
        
        // Reliability score (0-250)
        let rel_score = (reliability.success_rate * 250.0) as u32;
        
        // Cross-chain score (0-250)
        let cross_score = if cross_chain.is_some() { 200 } else { 0 };
        
        let overall = perf_score + eff_score + rel_score + cross_score;
        
        let tier = match overall {
            0..=199 => ReputationTier::Unrated,
            200..=399 => ReputationTier::Bronze,
            400..=599 => ReputationTier::Silver,
            600..=749 => ReputationTier::Gold,
            750..=899 => ReputationTier::Platinum,
            _ => ReputationTier::Diamond,
        };
        
        ReputationScore {
            overall_score: overall,
            performance_score: perf_score,
            efficiency_score: eff_score,
            reliability_score: rel_score,
            cross_chain_score: cross_score,
            tier,
        }
    }
    
    /// Generate proof of computation
    async fn generate_proof(&self, metrics: &AggregatedMetrics) -> Result<MetricsProof> {
        let data_sources = vec![
            DataSource {
                source_type: "events".to_string(),
                block_range: (metrics.time_range.start_block, metrics.time_range.end_block),
                events_count: metrics.reliability_metrics.total_intents,
                data_hash: [0u8; 32], // Mock
            },
        ];
        
        Ok(MetricsProof {
            computation_hash: self.hash_metrics(metrics),
            data_sources,
            proof_type: ProofType::ZKAggregation,
            proof_data: vec![0u8; 512], // Mock proof
        })
    }
    
    /// Get cached metrics
    async fn get_cached_metrics(&self, key: &str) -> Option<AggregatedMetrics> {
        let cache = self.results_cache.read().await;
        cache.get(key).cloned()
    }
    
    /// Cache computed metrics
    async fn cache_metrics(&self, key: &str, metrics: &AggregatedMetrics) -> Result<()> {
        let mut cache = self.results_cache.write().await;
        cache.insert(key.to_string(), metrics.clone());
        Ok(())
    }
    
    fn hash_metrics(&self, metrics: &AggregatedMetrics) -> [u8; 32] {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        
        // Hash key fields
        hasher.update(metrics.agent_id.as_bytes());
        hasher.update(&metrics.time_range.start_block.to_le_bytes());
        hasher.update(&metrics.time_range.end_block.to_le_bytes());
        hasher.update(&metrics.reputation_score.overall_score.to_le_bytes());
        
        let result = hasher.finalize();
        let mut output = [0u8; 32];
        output.copy_from_slice(&result);
        output
    }
}

impl HistoricalDataScanner {
    /// Scan agent events
    pub async fn scan_agent_events(
        &self,
        agent_id: &str,
        time_range: &TimeRange,
    ) -> Result<Vec<AgentEvent>> {
        let scanner = &self.event_scanner;
        let events = scanner.agent_events.read().await;
        
        Ok(events.get(agent_id)
            .map(|agent_events| {
                agent_events.iter()
                    .filter(|e| e.block_number >= time_range.start_block && e.block_number <= time_range.end_block)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    
    /// Scan transactions
    pub async fn scan_transactions(
        &self,
        agent_id: &str,
        time_range: &TimeRange,
    ) -> Result<Vec<TxReceipt>> {
        let analyzer = &self.tx_analyzer;
        let receipts = analyzer.receipts.read().await;
        
        Ok(receipts.get(agent_id)
            .map(|agent_receipts| {
                agent_receipts.iter()
                    .filter(|r| r.block_number >= time_range.start_block && r.block_number <= time_range.end_block)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    
    /// Scan memory logs
    pub async fn scan_memory_logs(
        &self,
        agent_id: &str,
        time_range: &TimeRange,
    ) -> Result<Vec<MemorySnapshot>> {
        let processor = &self.memory_processor;
        let snapshots = processor.memory_snapshots.read().await;
        
        Ok(snapshots.get(agent_id)
            .map(|agent_snapshots| {
                agent_snapshots.iter()
                    .filter(|s| s.block_number >= time_range.start_block && s.block_number <= time_range.end_block)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl MetricCalculators {
    /// Calculate performance metrics
    pub async fn calculate_performance(
        &self,
        events: &[AgentEvent],
        receipts: &[TxReceipt],
    ) -> Result<PerformanceMetrics> {
        // Mock calculation
        Ok(PerformanceMetrics {
            total_value_managed: 1_000_000_000_000_000_000_000, // 1000 ETH
            total_yield_generated: 50_000_000_000_000_000_000, // 50 ETH
            average_apy: 15.5,
            best_apy: 45.2,
            worst_apy: -5.1,
            sharpe_ratio: Some(1.85),
        })
    }
    
    /// Calculate efficiency metrics
    pub async fn calculate_efficiency(
        &self,
        receipts: &[TxReceipt],
        events: &[AgentEvent],
    ) -> Result<EfficiencyMetrics> {
        let total_gas: u128 = receipts.iter().map(|r| r.gas_used).sum();
        let avg_gas = if !receipts.is_empty() {
            total_gas / receipts.len() as u128
        } else {
            0
        };
        
        Ok(EfficiencyMetrics {
            total_gas_used: total_gas,
            average_gas_per_intent: avg_gas,
            gas_optimization_score: 0.85,
            average_slippage_bps: 25,
            execution_speed_ms: 1200,
        })
    }
    
    /// Calculate reliability metrics
    pub async fn calculate_reliability(
        &self,
        receipts: &[TxReceipt],
        events: &[AgentEvent],
    ) -> Result<ReliabilityMetrics> {
        let total = receipts.len() as u64;
        let successful = receipts.iter().filter(|r| r.success).count() as u64;
        let failed = total - successful;
        
        Ok(ReliabilityMetrics {
            total_intents: total,
            successful_intents: successful,
            failed_intents: failed,
            success_rate: if total > 0 { successful as f64 / total as f64 } else { 0.0 },
            uptime_percentage: 99.5,
            error_recovery_rate: 0.92,
        })
    }
}

impl CrossChainAggregator {
    /// Aggregate cross-chain metrics
    pub async fn aggregate_cross_chain(
        &self,
        agent_id: &str,
        time_range: &TimeRange,
    ) -> Result<CrossChainMetrics> {
        let mut chains = HashMap::new();
        
        // Query each chain
        for (chain_id, source) in &self.chain_sources {
            if let Ok(metrics) = source.get_agent_metrics(agent_id, time_range) {
                chains.insert(chain_id.clone(), metrics);
            }
        }
        
        let total_value: u128 = chains.values().map(|m| m.total_value_locked).sum();
        
        Ok(CrossChainMetrics {
            agent_id: agent_id.to_string(),
            chains,
            total_cross_chain_value: total_value,
            bridge_fees_paid: 5_000_000_000_000_000, // 0.005 ETH
        })
    }
}