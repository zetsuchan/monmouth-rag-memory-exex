//! Othentic Integration Module
//! 
//! Enhanced with MCP server, pre-batching, leader election, validation service,
//! and hybrid execution strategy.

pub mod mcp_server;
pub mod pre_batching;
pub mod leader_election;
pub mod validation_service;
pub mod hybrid_execution;

// Re-export main types
pub use mcp_server::{MCPServer, MCPConfig, MCPTool, ToolResult};
pub use pre_batching::{PreBatchingEngine, BatchConfig, OptimizationTarget};
pub use leader_election::{LeaderElection, ElectionAlgorithm, OperatorInfo};
pub use validation_service::{ValidationService, ValidationEndpoint, ValidationRule};
pub use hybrid_execution::{HybridExecutionCoordinator, ExecutionStrategy};