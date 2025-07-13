use alloy::primitives::{Address, B256, U256};
use async_trait::async_trait;
use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::enhanced_intent_parser::{Intent, IntentType};

/// Fuzzy matching engine for Coincidence of Wants discovery
pub struct CoWMatcher {
    /// Intent pool for matching
    intent_pool: Arc<RwLock<IntentPool>>,
    /// Matching strategies
    strategies: Vec<Box<dyn MatchingStrategy>>,
    /// Match scorer
    scorer: MatchScorer,
    /// Match cache
    cache: Arc<RwLock<MatchCache>>,
    /// Configuration
    config: MatcherConfig,
}

/// Pool of intents available for matching
#[derive(Debug, Clone)]
pub struct IntentPool {
    /// Active intents indexed by ID
    intents: HashMap<B256, PooledIntent>,
    /// Intents indexed by type for faster lookup
    by_type: HashMap<String, Vec<B256>>,
    /// Intents indexed by token for swap matching
    by_token: HashMap<Address, Vec<B256>>,
    /// Time-based index for expiration
    by_expiry: Vec<(u64, B256)>,
}

/// Intent stored in the pool with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PooledIntent {
    pub intent: Intent,
    pub submitter: Address,
    pub added_at: u64,
    pub expires_at: u64,
    pub match_preferences: MatchPreferences,
    pub status: IntentStatus,
}

/// Status of pooled intent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IntentStatus {
    Active,
    Matched,
    PartiallyMatched,
    Expired,
    Cancelled,
}

/// Preferences for matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchPreferences {
    /// Minimum match quality required (0.0 to 1.0)
    pub min_match_quality: f32,
    /// Allow partial matches
    pub allow_partial: bool,
    /// Maximum counterparties
    pub max_counterparties: usize,
    /// Required attributes for counterparty
    pub required_attributes: Vec<String>,
    /// Preferred protocols for execution
    pub preferred_protocols: Vec<String>,
}

/// Configuration for the matcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatcherConfig {
    /// Maximum intents to consider per matching round
    pub max_intents_per_round: usize,
    /// Minimum similarity score for fuzzy matching
    pub min_similarity_score: f32,
    /// Enable multi-party matching
    pub enable_multiparty: bool,
    /// Maximum depth for recursive matching
    pub max_recursion_depth: usize,
    /// Time window for batch matching (ms)
    pub batch_window_ms: u64,
}

/// Trait for matching strategies
#[async_trait]
pub trait MatchingStrategy: Send + Sync {
    /// Name of the strategy
    fn name(&self) -> &str;
    
    /// Find matches for an intent
    async fn find_matches(
        &self,
        intent: &Intent,
        pool: &IntentPool,
        config: &MatcherConfig,
    ) -> Result<Vec<MatchCandidate>>;
    
    /// Score a potential match
    fn score_match(&self, intent: &Intent, candidate: &PooledIntent) -> f32;
}

/// Candidate for matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub intent_id: B256,
    pub match_type: MatchType,
    pub quality_score: f32,
    pub attributes: HashMap<String, serde_json::Value>,
}

/// Types of matches
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MatchType {
    /// Direct swap match (A wants B, B wants A)
    DirectSwap,
    /// Ring match (A->B->C->A)
    Ring(Vec<B256>),
    /// Partial match where amounts don't fully align
    Partial(f32), // percentage matched
    /// Multi-party match
    MultiParty(Vec<B256>),
    /// Cross-chain match
    CrossChain {
        source_chain: u64,
        target_chain: u64,
    },
}

/// Match scoring engine
pub struct MatchScorer {
    /// Scoring weights
    weights: ScoringWeights,
    /// Feature extractors
    feature_extractors: Vec<Box<dyn FeatureExtractor>>,
}

/// Weights for different scoring factors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringWeights {
    pub token_similarity: f32,
    pub amount_alignment: f32,
    pub time_compatibility: f32,
    pub protocol_preference: f32,
    pub gas_efficiency: f32,
    pub risk_alignment: f32,
}

/// Trait for feature extraction
pub trait FeatureExtractor: Send + Sync {
    fn extract_features(
        &self,
        intent1: &Intent,
        intent2: &Intent,
    ) -> HashMap<String, f32>;
}

/// Cache for match results
pub struct MatchCache {
    /// Cached matches
    matches: HashMap<(B256, B256), CachedMatch>,
    /// Cache TTL
    ttl: u64,
    /// Maximum cache size
    max_size: usize,
}

/// Cached match result
#[derive(Debug, Clone)]
pub struct CachedMatch {
    pub score: f32,
    pub match_type: MatchType,
    pub timestamp: u64,
}

/// Result of matching operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub match_id: B256,
    pub intents: Vec<B256>,
    pub match_type: MatchType,
    pub quality_score: f32,
    pub execution_plan: ExecutionPlan,
    pub estimated_gas: U256,
    pub estimated_savings: U256,
}

/// Execution plan for matched intents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionStep>,
    pub required_protocols: Vec<String>,
    pub atomicity_requirements: AtomicityRequirement,
    pub fallback_options: Vec<FallbackOption>,
}

/// Single execution step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub step_type: StepType,
    pub protocol: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub dependencies: Vec<usize>,
}

/// Types of execution steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepType {
    Swap,
    Transfer,
    Bridge,
    Aggregate,
    Split,
}

/// Atomicity requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AtomicityRequirement {
    /// All steps must succeed or all fail
    AllOrNothing,
    /// Best effort execution
    BestEffort,
    /// Custom atomicity groups
    Groups(Vec<Vec<usize>>),
}

/// Fallback options for failed execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackOption {
    pub trigger_condition: String,
    pub fallback_steps: Vec<ExecutionStep>,
}

/// Implementation of direct swap matching strategy
pub struct DirectSwapStrategy;

#[async_trait]
impl MatchingStrategy for DirectSwapStrategy {
    fn name(&self) -> &str {
        "direct_swap"
    }
    
    async fn find_matches(
        &self,
        intent: &Intent,
        pool: &IntentPool,
        config: &MatcherConfig,
    ) -> Result<Vec<MatchCandidate>> {
        let mut candidates = Vec::new();
        
        if let IntentType::Swap { token_in, token_out, amount, .. } = &intent.intent_type {
            // Look for opposite swaps
            if let Some(intent_ids) = pool.by_token.get(token_out) {
                for intent_id in intent_ids.iter().take(config.max_intents_per_round) {
                    if let Some(pooled) = pool.intents.get(intent_id) {
                        if let IntentType::Swap { 
                            token_in: other_in, 
                            token_out: other_out, 
                            amount: other_amount, 
                            .. 
                        } = &pooled.intent.intent_type {
                            if other_in == token_out && other_out == token_in {
                                let score = self.score_match(intent, pooled);
                                if score >= config.min_similarity_score {
                                    candidates.push(MatchCandidate {
                                        intent_id: *intent_id,
                                        match_type: MatchType::DirectSwap,
                                        quality_score: score,
                                        attributes: HashMap::new(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(candidates)
    }
    
    fn score_match(&self, intent: &Intent, candidate: &PooledIntent) -> f32 {
        if let (
            IntentType::Swap { amount: amount1, .. },
            IntentType::Swap { amount: amount2, .. }
        ) = (&intent.intent_type, &candidate.intent.intent_type) {
            // Score based on amount alignment
            let amount_ratio = if amount1 > amount2 {
                amount2.as_limbs()[0] as f32 / amount1.as_limbs()[0] as f32
            } else {
                amount1.as_limbs()[0] as f32 / amount2.as_limbs()[0] as f32
            };
            
            // Consider time compatibility
            let time_score = if candidate.expires_at > intent.timestamp {
                1.0
            } else {
                0.5
            };
            
            amount_ratio * 0.7 + time_score * 0.3
        } else {
            0.0
        }
    }
}

/// Ring matching strategy for multi-hop CoW
pub struct RingMatchStrategy {
    max_ring_size: usize,
}

#[async_trait]
impl MatchingStrategy for RingMatchStrategy {
    fn name(&self) -> &str {
        "ring_match"
    }
    
    async fn find_matches(
        &self,
        intent: &Intent,
        pool: &IntentPool,
        config: &MatcherConfig,
    ) -> Result<Vec<MatchCandidate>> {
        // Implementation would find circular trading opportunities
        Ok(vec![])
    }
    
    fn score_match(&self, intent: &Intent, candidate: &PooledIntent) -> f32 {
        // Ring matching scoring logic
        0.5
    }
}

impl CoWMatcher {
    /// Create new CoW matcher
    pub fn new(config: MatcherConfig) -> Self {
        Self {
            intent_pool: Arc::new(RwLock::new(IntentPool::new())),
            strategies: vec![
                Box::new(DirectSwapStrategy),
                Box::new(RingMatchStrategy { max_ring_size: 4 }),
            ],
            scorer: MatchScorer::new(),
            cache: Arc::new(RwLock::new(MatchCache::new())),
            config,
        }
    }
    
    /// Add intent to the pool
    pub async fn add_intent(
        &self,
        intent: Intent,
        submitter: Address,
        preferences: MatchPreferences,
    ) -> Result<()> {
        let mut pool = self.intent_pool.write().await;
        
        let pooled = PooledIntent {
            intent: intent.clone(),
            submitter,
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            expires_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() + 3600, // 1 hour default expiry
            match_preferences: preferences,
            status: IntentStatus::Active,
        };
        
        pool.add_intent(pooled)?;
        
        // Trigger matching for new intent
        self.try_match_intent(&intent).await?;
        
        Ok(())
    }
    
    /// Try to match a specific intent
    pub async fn try_match_intent(&self, intent: &Intent) -> Result<Vec<MatchResult>> {
        let pool = self.intent_pool.read().await;
        let mut all_matches = Vec::new();
        
        // Try each matching strategy
        for strategy in &self.strategies {
            let candidates = strategy.find_matches(intent, &pool, &self.config).await?;
            
            for candidate in candidates {
                if candidate.quality_score >= self.config.min_similarity_score {
                    // Build execution plan
                    let execution_plan = self.build_execution_plan(intent, &candidate, &pool)?;
                    
                    let match_result = MatchResult {
                        match_id: B256::random(),
                        intents: vec![intent.id, candidate.intent_id],
                        match_type: candidate.match_type,
                        quality_score: candidate.quality_score,
                        execution_plan,
                        estimated_gas: U256::from(200_000u64), // Simplified estimate
                        estimated_savings: U256::from(50_000u64), // Simplified estimate
                    };
                    
                    all_matches.push(match_result);
                }
            }
        }
        
        Ok(all_matches)
    }
    
    /// Build execution plan for matched intents
    fn build_execution_plan(
        &self,
        intent: &Intent,
        candidate: &MatchCandidate,
        pool: &IntentPool,
    ) -> Result<ExecutionPlan> {
        let mut steps = Vec::new();
        
        match &candidate.match_type {
            MatchType::DirectSwap => {
                steps.push(ExecutionStep {
                    step_type: StepType::Swap,
                    protocol: "uniswap_v3".to_string(),
                    parameters: HashMap::new(),
                    dependencies: vec![],
                });
            }
            MatchType::Ring(intents) => {
                // Build ring execution steps
                for (i, intent_id) in intents.iter().enumerate() {
                    steps.push(ExecutionStep {
                        step_type: StepType::Swap,
                        protocol: "cow_protocol".to_string(),
                        parameters: HashMap::new(),
                        dependencies: if i > 0 { vec![i - 1] } else { vec![] },
                    });
                }
            }
            _ => {}
        }
        
        Ok(ExecutionPlan {
            steps,
            required_protocols: vec!["cow_protocol".to_string()],
            atomicity_requirements: AtomicityRequirement::AllOrNothing,
            fallback_options: vec![],
        })
    }
    
    /// Run batch matching process
    pub async fn run_batch_matching(&self) -> Result<Vec<MatchResult>> {
        let pool = self.intent_pool.read().await;
        let active_intents: Vec<_> = pool.intents.values()
            .filter(|i| i.status == IntentStatus::Active)
            .collect();
        
        let mut all_matches = Vec::new();
        
        // Try to match all active intents
        for pooled in active_intents {
            let matches = self.try_match_intent(&pooled.intent).await?;
            all_matches.extend(matches);
        }
        
        // Deduplicate and optimize matches
        let optimized = self.optimize_matches(all_matches)?;
        
        Ok(optimized)
    }
    
    /// Optimize set of matches to maximize overall utility
    fn optimize_matches(&self, matches: Vec<MatchResult>) -> Result<Vec<MatchResult>> {
        // Simple deduplication for now
        let mut seen_intents = HashSet::new();
        let mut optimized = Vec::new();
        
        for match_result in matches {
            let mut is_duplicate = false;
            for intent_id in &match_result.intents {
                if seen_intents.contains(intent_id) {
                    is_duplicate = true;
                    break;
                }
            }
            
            if !is_duplicate {
                for intent_id in &match_result.intents {
                    seen_intents.insert(*intent_id);
                }
                optimized.push(match_result);
            }
        }
        
        Ok(optimized)
    }
}

impl IntentPool {
    pub fn new() -> Self {
        Self {
            intents: HashMap::new(),
            by_type: HashMap::new(),
            by_token: HashMap::new(),
            by_expiry: Vec::new(),
        }
    }
    
    pub fn add_intent(&mut self, pooled: PooledIntent) -> Result<()> {
        let intent_id = pooled.intent.id;
        
        // Index by type
        let type_key = match &pooled.intent.intent_type {
            IntentType::Swap { .. } => "swap",
            IntentType::Bridge { .. } => "bridge",
            _ => "other",
        };
        
        self.by_type.entry(type_key.to_string())
            .or_insert_with(Vec::new)
            .push(intent_id);
        
        // Index by token for swaps
        if let IntentType::Swap { token_in, token_out, .. } = &pooled.intent.intent_type {
            self.by_token.entry(*token_in)
                .or_insert_with(Vec::new)
                .push(intent_id);
            self.by_token.entry(*token_out)
                .or_insert_with(Vec::new)
                .push(intent_id);
        }
        
        // Add to expiry index
        self.by_expiry.push((pooled.expires_at, intent_id));
        self.by_expiry.sort_by_key(|&(expiry, _)| expiry);
        
        // Store the intent
        self.intents.insert(intent_id, pooled);
        
        Ok(())
    }
    
    pub fn remove_expired(&mut self, current_time: u64) -> Vec<B256> {
        let mut removed = Vec::new();
        
        while let Some(&(expiry, intent_id)) = self.by_expiry.first() {
            if expiry <= current_time {
                self.by_expiry.remove(0);
                if let Some(mut pooled) = self.intents.remove(&intent_id) {
                    pooled.status = IntentStatus::Expired;
                    removed.push(intent_id);
                }
            } else {
                break;
            }
        }
        
        removed
    }
}

impl MatchScorer {
    pub fn new() -> Self {
        Self {
            weights: ScoringWeights {
                token_similarity: 0.3,
                amount_alignment: 0.3,
                time_compatibility: 0.1,
                protocol_preference: 0.1,
                gas_efficiency: 0.1,
                risk_alignment: 0.1,
            },
            feature_extractors: vec![],
        }
    }
    
    pub fn score(&self, intent1: &Intent, intent2: &Intent) -> f32 {
        let mut total_score = 0.0;
        
        // Extract and weight features
        for extractor in &self.feature_extractors {
            let features = extractor.extract_features(intent1, intent2);
            for (feature_name, feature_value) in features {
                // Apply appropriate weight based on feature name
                total_score += feature_value * 0.1; // Simplified
            }
        }
        
        total_score.min(1.0).max(0.0)
    }
}

impl MatchCache {
    pub fn new() -> Self {
        Self {
            matches: HashMap::new(),
            ttl: 300, // 5 minutes
            max_size: 10000,
        }
    }
    
    pub fn get(&self, intent1: &B256, intent2: &B256) -> Option<&CachedMatch> {
        let key = if intent1 < intent2 {
            (*intent1, *intent2)
        } else {
            (*intent2, *intent1)
        };
        
        self.matches.get(&key).filter(|cached| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            now - cached.timestamp < self.ttl
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_cow_matcher_creation() {
        let config = MatcherConfig {
            max_intents_per_round: 100,
            min_similarity_score: 0.7,
            enable_multiparty: true,
            max_recursion_depth: 3,
            batch_window_ms: 1000,
        };
        
        let matcher = CoWMatcher::new(config);
        assert!(matcher.intent_pool.read().await.intents.is_empty());
    }
    
    #[test]
    fn test_match_type_equality() {
        assert_eq!(MatchType::DirectSwap, MatchType::DirectSwap);
        assert_ne!(MatchType::DirectSwap, MatchType::Partial(0.5));
    }
}