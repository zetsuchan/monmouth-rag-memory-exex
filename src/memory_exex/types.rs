//! Memory types for the Memory ExEx module

use alloy::primitives::B256;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::MemoryType;

/// A memory entry stored in the memory store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// The serialized content of the memory
    pub content: Vec<u8>,
    
    /// The type of memory (Working, Episodic, Semantic, etc.)
    pub memory_type: MemoryType,
    
    /// Timestamp when the memory was created
    pub timestamp: u64,
    
    /// Additional metadata for the memory
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Request types for memory operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryRequest {
    /// Store a new memory
    Store {
        content: Vec<u8>,
        memory_type: MemoryType,
    },
    
    /// Retrieve a memory by hash
    Retrieve {
        hash: B256,
    },
    
    /// Query memories based on criteria
    Query {
        criteria: QueryCriteria,
    },
}

/// Response types for memory operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryResponse {
    /// Memory was successfully stored
    Stored {
        hash: B256,
    },
    
    /// Memory was retrieved
    Retrieved {
        memory: Option<MemoryEntry>,
    },
    
    /// Query results
    QueryResults {
        results: Vec<MemoryEntry>,
    },
}

/// Criteria for querying memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCriteria {
    /// Filter by memory type
    pub memory_type: Option<MemoryType>,
    
    /// Filter by time range
    pub time_range: Option<(u64, u64)>,
    
    /// Filter by metadata key-value pairs
    pub metadata_filters: HashMap<String, serde_json::Value>,
    
    /// Maximum number of results
    pub limit: Option<usize>,
    
    /// Sort order
    pub sort_by: Option<SortBy>,
}

/// Sort options for query results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortBy {
    /// Sort by timestamp (newest first)
    TimestampDesc,
    
    /// Sort by timestamp (oldest first)
    TimestampAsc,
    
    /// Sort by relevance score
    Relevance,
}