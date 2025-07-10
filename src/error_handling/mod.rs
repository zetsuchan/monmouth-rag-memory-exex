use eyre::Result;

pub mod cross_exex;
pub mod recovery;
pub mod circuit_breaker;
pub mod retry;

pub use cross_exex::*;
pub use recovery::*;
pub use circuit_breaker::*;
pub use retry::*;

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHandlingConfig {
    pub enable_circuit_breaker: bool,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_timeout: Duration,
    pub enable_retry: bool,
    pub max_retry_attempts: u32,
    pub initial_retry_delay: Duration,
    pub max_retry_delay: Duration,
    pub enable_graceful_degradation: bool,
    pub degradation_timeout: Duration,
    pub enable_error_classification: bool,
    pub critical_error_patterns: Vec<String>,
}

impl Default for ErrorHandlingConfig {
    fn default() -> Self {
        Self {
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
                "database error".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCategory {
    Network,
    Database,
    Memory,
    Processing,
    Configuration,
    External,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    pub error_id: String,
    pub severity: ErrorSeverity,
    pub category: ErrorCategory,
    pub message: String,
    pub timestamp: u64,
    pub component: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHandlingMetrics {
    pub total_errors: u64,
    pub errors_by_severity: std::collections::HashMap<String, u64>,
    pub errors_by_category: std::collections::HashMap<String, u64>,
    pub recovery_attempts: u64,
    pub successful_recoveries: u64,
    pub circuit_breaker_trips: u64,
    pub retry_attempts: u64,
    pub degradation_events: u64,
}

impl Default for ErrorHandlingMetrics {
    fn default() -> Self {
        Self {
            total_errors: 0,
            errors_by_severity: std::collections::HashMap::new(),
            errors_by_category: std::collections::HashMap::new(),
            recovery_attempts: 0,
            successful_recoveries: 0,
            circuit_breaker_trips: 0,
            retry_attempts: 0,
            degradation_events: 0,
        }
    }
}

pub trait ErrorHandler: Send + Sync {
    async fn handle_error(&self, error: ErrorContext) -> Result<ErrorHandlingResponse>;
    async fn classify_error(&self, error: &str) -> ErrorContext;
    async fn should_retry(&self, error: &ErrorContext) -> bool;
    async fn get_metrics(&self) -> ErrorHandlingMetrics;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorHandlingResponse {
    Retry {
        delay: Duration,
        attempt: u32,
    },
    CircuitBreaker {
        trip_reason: String,
        recovery_time: Duration,
    },
    Graceful {
        fallback_action: String,
        degraded_mode: bool,
    },
    Fatal {
        error_id: String,
        shutdown_required: bool,
    },
}