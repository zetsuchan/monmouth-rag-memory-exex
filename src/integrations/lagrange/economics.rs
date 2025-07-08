//! Economic Model for Query Fees
//! 
//! Manages pricing, payments, and incentives for Lagrange query operations
//! including dynamic pricing, operator rewards, and fee distribution.

use eyre::Result;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;

/// Economic manager for Lagrange queries
#[derive(Debug)]
pub struct EconomicsManager {
    /// Fee calculator
    fee_calculator: Arc<FeeCalculator>,
    /// Payment processor
    payment_processor: Arc<PaymentProcessor>,
    /// Reward distributor
    reward_distributor: Arc<RewardDistributor>,
    /// Fee pool manager
    fee_pool: Arc<FeePoolManager>,
    /// Market dynamics tracker
    market_tracker: Arc<MarketDynamicsTracker>,
}

/// Calculates fees for different query types
#[derive(Debug)]
pub struct FeeCalculator {
    /// Base fees by query type
    base_fees: HashMap<QueryType, BaseFee>,
    /// Complexity multipliers
    complexity_factors: ComplexityFactors,
    /// Dynamic pricing model
    pricing_model: Arc<DynamicPricingModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseFee {
    pub query_type: QueryType,
    pub base_amount: u128,
    pub min_fee: u128,
    pub max_fee: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum QueryType {
    HistoricalMemory,
    RAGDocument,
    AgentMetrics,
    CrossChainState,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct ComplexityFactors {
    /// Data size factor (per KB)
    pub data_size_factor: f64,
    /// Time range factor (per block)
    pub time_range_factor: f64,
    /// Cross-chain factor
    pub cross_chain_multiplier: f64,
    /// Proof type factors
    pub proof_factors: HashMap<String, f64>,
}

/// Dynamic pricing model based on network conditions
#[derive(Debug)]
pub struct DynamicPricingModel {
    /// Current network load
    network_load: Arc<RwLock<NetworkLoad>>,
    /// Price history
    price_history: Arc<RwLock<Vec<PricePoint>>>,
    /// Pricing algorithm
    algorithm: PricingAlgorithm,
}

#[derive(Debug, Clone)]
pub struct NetworkLoad {
    pub active_queries: u64,
    pub pending_queries: u64,
    pub average_wait_time: u64,
    pub available_provers: u64,
    pub total_stake: u128,
}

#[derive(Debug, Clone)]
pub struct PricePoint {
    pub timestamp: u64,
    pub query_type: QueryType,
    pub price: u128,
    pub volume: u64,
}

#[derive(Debug, Clone)]
pub enum PricingAlgorithm {
    /// Fixed pricing
    Fixed,
    /// Supply/demand based
    MarketBased {
        elasticity: f64,
        base_multiplier: f64,
    },
    /// Auction-based
    Auction {
        min_bid: u128,
        time_window: u64,
    },
}

/// Processes payments for queries
#[derive(Debug)]
pub struct PaymentProcessor {
    /// Payment methods
    payment_methods: HashMap<String, Box<dyn PaymentMethod>>,
    /// Payment records
    payment_records: Arc<RwLock<HashMap<String, PaymentRecord>>>,
    /// Escrow manager
    escrow: Arc<EscrowManager>,
}

/// Payment method trait
pub trait PaymentMethod: Send + Sync {
    fn process_payment(&self, payment: &PaymentRequest) -> Result<PaymentReceipt>;
    fn refund(&self, receipt: &PaymentReceipt) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub query_id: String,
    pub payer: [u8; 20],
    pub amount: u128,
    pub token: PaymentToken,
    pub priority: QueryPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaymentToken {
    ETH,
    USDC,
    LagrangeToken,
    Custom([u8; 20]),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryPriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentReceipt {
    pub payment_id: String,
    pub query_id: String,
    pub amount_paid: u128,
    pub token: PaymentToken,
    pub timestamp: u64,
    pub tx_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone)]
pub struct PaymentRecord {
    pub receipt: PaymentReceipt,
    pub status: PaymentStatus,
    pub refundable_until: u64,
}

#[derive(Debug, Clone)]
pub enum PaymentStatus {
    Pending,
    Confirmed,
    Distributed,
    Refunded,
}

/// Manages payment escrow
#[derive(Debug)]
pub struct EscrowManager {
    /// Escrowed funds by query
    escrow_accounts: Arc<RwLock<HashMap<String, EscrowAccount>>>,
    /// Release conditions
    release_conditions: HashMap<String, ReleaseCondition>,
}

#[derive(Debug, Clone)]
pub struct EscrowAccount {
    pub query_id: String,
    pub amount: u128,
    pub token: PaymentToken,
    pub created_at: u64,
    pub status: EscrowStatus,
}

#[derive(Debug, Clone)]
pub enum EscrowStatus {
    Locked,
    ReleasePending,
    Released,
    Refunded,
}

#[derive(Debug, Clone)]
pub enum ReleaseCondition {
    ProofDelivered,
    TimeElapsed(u64),
    MultiSigApproval(u8),
}

/// Distributes rewards to operators
#[derive(Debug)]
pub struct RewardDistributor {
    /// Reward pools
    reward_pools: Arc<RwLock<HashMap<String, RewardPool>>>,
    /// Distribution rules
    distribution_rules: DistributionRules,
    /// Operator performance tracker
    performance_tracker: Arc<OperatorPerformanceTracker>,
}

#[derive(Debug, Clone)]
pub struct RewardPool {
    pub pool_id: String,
    pub total_amount: u128,
    pub token: PaymentToken,
    pub participants: Vec<PoolParticipant>,
    pub distribution_time: u64,
}

#[derive(Debug, Clone)]
pub struct PoolParticipant {
    pub operator_id: String,
    pub contribution_score: u64,
    pub stake_weight: u128,
    pub performance_multiplier: f64,
}

#[derive(Debug, Clone)]
pub struct DistributionRules {
    /// Base reward percentage for operators
    pub operator_share: u8,
    /// Protocol fee percentage
    pub protocol_fee: u8,
    /// Treasury allocation
    pub treasury_share: u8,
    /// Slashing penalties
    pub slashing_rates: HashMap<String, u8>,
}

/// Tracks operator performance
#[derive(Debug)]
pub struct OperatorPerformanceTracker {
    /// Performance metrics by operator
    metrics: Arc<RwLock<HashMap<String, OperatorMetrics>>>,
    /// Performance history
    history: Arc<RwLock<Vec<PerformanceSnapshot>>>,
}

#[derive(Debug, Clone)]
pub struct OperatorMetrics {
    pub operator_id: String,
    pub proofs_generated: u64,
    pub average_proof_time: u64,
    pub success_rate: f64,
    pub uptime_percentage: f64,
    pub reputation_score: u32,
}

#[derive(Debug, Clone)]
pub struct PerformanceSnapshot {
    pub timestamp: u64,
    pub operator_metrics: HashMap<String, OperatorMetrics>,
    pub network_totals: NetworkTotals,
}

#[derive(Debug, Clone)]
pub struct NetworkTotals {
    pub total_queries: u64,
    pub total_proofs: u64,
    pub average_response_time: u64,
    pub total_fees_collected: u128,
}

/// Manages the fee pool
#[derive(Debug)]
pub struct FeePoolManager {
    /// Current pool balance
    pool_balance: Arc<RwLock<HashMap<PaymentToken, u128>>>,
    /// Distribution schedule
    distribution_schedule: DistributionSchedule,
    /// Historical distributions
    distribution_history: Arc<RwLock<Vec<Distribution>>>,
}

#[derive(Debug, Clone)]
pub struct DistributionSchedule {
    pub frequency: DistributionFrequency,
    pub next_distribution: u64,
    pub min_pool_size: u128,
}

#[derive(Debug, Clone)]
pub enum DistributionFrequency {
    Hourly,
    Daily,
    Weekly,
    OnThreshold(u128),
}

#[derive(Debug, Clone)]
pub struct Distribution {
    pub distribution_id: String,
    pub timestamp: u64,
    pub total_amount: u128,
    pub recipients: Vec<DistributionRecipient>,
}

#[derive(Debug, Clone)]
pub struct DistributionRecipient {
    pub address: [u8; 20],
    pub amount: u128,
    pub reason: String,
}

/// Tracks market dynamics
#[derive(Debug)]
pub struct MarketDynamicsTracker {
    /// Query demand metrics
    demand_metrics: Arc<RwLock<DemandMetrics>>,
    /// Supply metrics
    supply_metrics: Arc<RwLock<SupplyMetrics>>,
    /// Price discovery mechanism
    price_discovery: Arc<PriceDiscovery>,
}

#[derive(Debug, Clone)]
pub struct DemandMetrics {
    pub queries_per_hour: u64,
    pub average_fee_paid: u128,
    pub query_type_distribution: HashMap<QueryType, u64>,
    pub priority_distribution: HashMap<QueryPriority, u64>,
}

#[derive(Debug, Clone)]
pub struct SupplyMetrics {
    pub active_operators: u64,
    pub total_compute_capacity: u64,
    pub average_utilization: f64,
    pub geographic_distribution: HashMap<String, u64>,
}

#[derive(Debug)]
pub struct PriceDiscovery {
    /// Order book for queries
    order_book: Arc<RwLock<OrderBook>>,
    /// Market makers
    market_makers: Vec<MarketMaker>,
}

#[derive(Debug, Clone)]
pub struct OrderBook {
    pub bids: Vec<Order>,
    pub asks: Vec<Order>,
    pub last_match_price: u128,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub order_id: String,
    pub order_type: OrderType,
    pub query_type: QueryType,
    pub price: u128,
    pub quantity: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub enum OrderType {
    Bid,
    Ask,
}

#[derive(Debug, Clone)]
pub struct MarketMaker {
    pub maker_id: String,
    pub liquidity_provided: u128,
    pub spread_percentage: f64,
}

impl EconomicsManager {
    pub fn new() -> Self {
        Self {
            fee_calculator: Arc::new(FeeCalculator::new()),
            payment_processor: Arc::new(PaymentProcessor::new()),
            reward_distributor: Arc::new(RewardDistributor::new()),
            fee_pool: Arc::new(FeePoolManager::new()),
            market_tracker: Arc::new(MarketDynamicsTracker::new()),
        }
    }
    
    /// Calculate fee for a query
    pub async fn calculate_fee(
        &self,
        query_type: QueryType,
        complexity: QueryComplexity,
        priority: QueryPriority,
    ) -> Result<QueryFee> {
        self.fee_calculator.calculate(query_type, complexity, priority).await
    }
    
    /// Process payment for a query
    pub async fn process_payment(&self, request: PaymentRequest) -> Result<PaymentReceipt> {
        self.payment_processor.process(request).await
    }
    
    /// Distribute rewards to operators
    pub async fn distribute_rewards(&self, query_id: String, operators: Vec<String>) -> Result<()> {
        self.reward_distributor.distribute(query_id, operators).await
    }
    
    /// Get current market metrics
    pub async fn get_market_metrics(&self) -> Result<MarketMetrics> {
        let demand = self.market_tracker.demand_metrics.read().await;
        let supply = self.market_tracker.supply_metrics.read().await;
        
        Ok(MarketMetrics {
            demand: demand.clone(),
            supply: supply.clone(),
            suggested_price: self.market_tracker.price_discovery.suggest_price().await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryComplexity {
    pub data_size_kb: u64,
    pub time_range_blocks: u64,
    pub cross_chain_hops: u8,
    pub proof_complexity: ProofComplexity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProofComplexity {
    Simple,
    Medium,
    Complex,
    Custom(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryFee {
    pub base_fee: u128,
    pub complexity_fee: u128,
    pub priority_fee: u128,
    pub total_fee: u128,
    pub fee_breakdown: FeeBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBreakdown {
    pub operator_reward: u128,
    pub protocol_fee: u128,
    pub treasury_allocation: u128,
    pub gas_reimbursement: u128,
}

#[derive(Debug, Clone)]
pub struct MarketMetrics {
    pub demand: DemandMetrics,
    pub supply: SupplyMetrics,
    pub suggested_price: u128,
}

impl FeeCalculator {
    fn new() -> Self {
        let mut base_fees = HashMap::new();
        
        // Set base fees for different query types
        base_fees.insert(QueryType::HistoricalMemory, BaseFee {
            query_type: QueryType::HistoricalMemory,
            base_amount: 1_000_000_000_000_000, // 0.001 ETH
            min_fee: 100_000_000_000_000,       // 0.0001 ETH
            max_fee: 10_000_000_000_000_000,    // 0.01 ETH
        });
        
        base_fees.insert(QueryType::CrossChainState, BaseFee {
            query_type: QueryType::CrossChainState,
            base_amount: 5_000_000_000_000_000, // 0.005 ETH
            min_fee: 1_000_000_000_000_000,     // 0.001 ETH
            max_fee: 50_000_000_000_000_000,    // 0.05 ETH
        });
        
        Self {
            base_fees,
            complexity_factors: ComplexityFactors {
                data_size_factor: 0.0001, // Per KB
                time_range_factor: 0.00001, // Per block
                cross_chain_multiplier: 2.0,
                proof_factors: HashMap::new(),
            },
            pricing_model: Arc::new(DynamicPricingModel::new()),
        }
    }
    
    async fn calculate(
        &self,
        query_type: QueryType,
        complexity: QueryComplexity,
        priority: QueryPriority,
    ) -> Result<QueryFee> {
        let base = self.base_fees.get(&query_type)
            .ok_or_else(|| eyre::eyre!("Unknown query type"))?;
        
        // Calculate complexity multiplier
        let complexity_multiplier = 1.0
            + (complexity.data_size_kb as f64 * self.complexity_factors.data_size_factor)
            + (complexity.time_range_blocks as f64 * self.complexity_factors.time_range_factor)
            + if complexity.cross_chain_hops > 0 {
                self.complexity_factors.cross_chain_multiplier
            } else { 0.0 };
        
        let base_fee = base.base_amount;
        let complexity_fee = (base_fee as f64 * complexity_multiplier) as u128 - base_fee;
        
        // Priority fee
        let priority_fee = match priority {
            QueryPriority::Low => 0,
            QueryPriority::Normal => base_fee / 10,
            QueryPriority::High => base_fee / 5,
            QueryPriority::Urgent => base_fee / 2,
        };
        
        let total_fee = base_fee + complexity_fee + priority_fee;
        
        // Apply min/max bounds
        let total_fee = total_fee.max(base.min_fee).min(base.max_fee);
        
        // Calculate breakdown (60% operators, 20% protocol, 15% treasury, 5% gas)
        let fee_breakdown = FeeBreakdown {
            operator_reward: total_fee * 60 / 100,
            protocol_fee: total_fee * 20 / 100,
            treasury_allocation: total_fee * 15 / 100,
            gas_reimbursement: total_fee * 5 / 100,
        };
        
        Ok(QueryFee {
            base_fee,
            complexity_fee,
            priority_fee,
            total_fee,
            fee_breakdown,
        })
    }
}

impl PaymentProcessor {
    fn new() -> Self {
        Self {
            payment_methods: HashMap::new(),
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            escrow: Arc::new(EscrowManager::new()),
        }
    }
    
    async fn process(&self, request: PaymentRequest) -> Result<PaymentReceipt> {
        // In production: Process actual payment
        let receipt = PaymentReceipt {
            payment_id: format!("pay_{}", uuid::Uuid::new_v4()),
            query_id: request.query_id,
            amount_paid: request.amount,
            token: request.token,
            timestamp: chrono::Utc::now().timestamp() as u64,
            tx_hash: Some([0u8; 32]),
        };
        
        // Store payment record
        let mut records = self.payment_records.write().await;
        records.insert(receipt.payment_id.clone(), PaymentRecord {
            receipt: receipt.clone(),
            status: PaymentStatus::Confirmed,
            refundable_until: chrono::Utc::now().timestamp() as u64 + 3600,
        });
        
        Ok(receipt)
    }
}

impl RewardDistributor {
    fn new() -> Self {
        Self {
            reward_pools: Arc::new(RwLock::new(HashMap::new())),
            distribution_rules: DistributionRules {
                operator_share: 60,
                protocol_fee: 20,
                treasury_share: 15,
                slashing_rates: HashMap::new(),
            },
            performance_tracker: Arc::new(OperatorPerformanceTracker::new()),
        }
    }
    
    async fn distribute(&self, query_id: String, operators: Vec<String>) -> Result<()> {
        // In production: Distribute rewards based on performance
        tracing::info!("Distributing rewards for query {} to {} operators", query_id, operators.len());
        Ok(())
    }
}

impl FeePoolManager {
    fn new() -> Self {
        Self {
            pool_balance: Arc::new(RwLock::new(HashMap::new())),
            distribution_schedule: DistributionSchedule {
                frequency: DistributionFrequency::Daily,
                next_distribution: chrono::Utc::now().timestamp() as u64 + 86400,
                min_pool_size: 100_000_000_000_000_000, // 0.1 ETH
            },
            distribution_history: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl MarketDynamicsTracker {
    fn new() -> Self {
        Self {
            demand_metrics: Arc::new(RwLock::new(DemandMetrics {
                queries_per_hour: 100,
                average_fee_paid: 2_000_000_000_000_000,
                query_type_distribution: HashMap::new(),
                priority_distribution: HashMap::new(),
            })),
            supply_metrics: Arc::new(RwLock::new(SupplyMetrics {
                active_operators: 50,
                total_compute_capacity: 10000,
                average_utilization: 0.65,
                geographic_distribution: HashMap::new(),
            })),
            price_discovery: Arc::new(PriceDiscovery::new()),
        }
    }
}

impl DynamicPricingModel {
    fn new() -> Self {
        Self {
            network_load: Arc::new(RwLock::new(NetworkLoad {
                active_queries: 100,
                pending_queries: 20,
                average_wait_time: 5000,
                available_provers: 50,
                total_stake: 100_000_000_000_000_000_000_000, // 100k ETH
            })),
            price_history: Arc::new(RwLock::new(Vec::new())),
            algorithm: PricingAlgorithm::MarketBased {
                elasticity: 1.5,
                base_multiplier: 1.0,
            },
        }
    }
}

impl EscrowManager {
    fn new() -> Self {
        Self {
            escrow_accounts: Arc::new(RwLock::new(HashMap::new())),
            release_conditions: HashMap::new(),
        }
    }
}

impl OperatorPerformanceTracker {
    fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl PriceDiscovery {
    fn new() -> Self {
        Self {
            order_book: Arc::new(RwLock::new(OrderBook {
                bids: Vec::new(),
                asks: Vec::new(),
                last_match_price: 2_000_000_000_000_000, // 0.002 ETH
            })),
            market_makers: Vec::new(),
        }
    }
    
    async fn suggest_price(&self) -> Result<u128> {
        let book = self.order_book.read().await;
        Ok(book.last_match_price)
    }
}