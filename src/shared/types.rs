use alloy::primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// Re-export types from other modules for convenience
pub use crate::memory_exex::MemoryType;
pub use crate::shared::ai_agent::{RoutingDecision, TransactionAnalysis, TransactionFeatures};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub agent_address: Address,
    pub last_action: AgentAction,
    pub reputation_score: u64,
    pub total_interactions: u64,
    pub success_rate: f64,
    pub specialization: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemoryState {
    pub agent_address: Address,
    pub working_memory: Vec<B256>,
    pub episodic_buffer: Vec<B256>,
    pub last_access_time: u64,
    pub context_switches: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentAction {
    Swap {
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        amount_out: U256,
    },
    AddLiquidity {
        token_a: Address,
        token_b: Address,
        amount_a: U256,
        amount_b: U256,
    },
    RemoveLiquidity {
        token_a: Address,
        token_b: Address,
        liquidity: U256,
    },
    Stake {
        token: Address,
        amount: U256,
    },
    Unstake {
        token: Address,
        amount: U256,
    },
    Transfer {
        to: Address,
        amount: U256,
    },
}