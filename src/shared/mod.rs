pub mod agent_standard;
pub mod metrics;
pub mod coordination;
pub mod ai_agent;
pub mod communication;
pub mod transaction_analyzer;
pub mod eip7702;
pub mod memory_root;

pub use agent_standard::AgentStandard;
pub use metrics::Metrics;
pub use coordination::BLSCoordinator;
pub use ai_agent::{AIAgent, UnifiedAIDecisionEngine, RoutingDecision};
pub use communication::{InterExExChannel, CrossExExMessage, CrossExExCoordinator};
pub use transaction_analyzer::TransactionAnalyzer;
pub use eip7702::{SelfPayingTransactionManager, AgentVault, SelfPayingTransaction};
pub use memory_root::{MemoryRootTracker, MemoryRootInfo, MEMORY_ROOT_SLOT};