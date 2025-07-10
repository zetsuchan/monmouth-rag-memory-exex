use eyre::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub recovery_timeout: Duration,
    pub success_threshold: u32,
    pub request_timeout: Duration,
    pub max_concurrent_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            success_threshold: 3,
            request_timeout: Duration::from_secs(30),
            max_concurrent_requests: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CircuitBreakerState {
    Closed,
    Open { opened_at: Instant },
    HalfOpen { test_requests: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerMetrics {
    pub state: String,
    pub failure_count: u32,
    pub success_count: u32,
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub state_transitions: u64,
    pub average_response_time_ms: f64,
    pub last_failure_time: Option<u64>,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
    failure_count: Arc<RwLock<u32>>,
    success_count: Arc<RwLock<u32>>,
    metrics: Arc<RwLock<CircuitBreakerMetrics>>,
    concurrent_requests: Arc<RwLock<u32>>,
    response_times: Arc<RwLock<Vec<Duration>>>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::Closed)),
            failure_count: Arc::new(RwLock::new(0)),
            success_count: Arc::new(RwLock::new(0)),
            metrics: Arc::new(RwLock::new(CircuitBreakerMetrics {
                state: "Closed".to_string(),
                failure_count: 0,
                success_count: 0,
                total_requests: 0,
                rejected_requests: 0,
                state_transitions: 0,
                average_response_time_ms: 0.0,
                last_failure_time: None,
            })),
            concurrent_requests: Arc::new(RwLock::new(0)),
            response_times: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn call<F, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        if !self.can_execute().await? {
            self.record_rejection().await;
            return Err(CircuitBreakerError::CircuitOpen);
        }

        self.increment_concurrent_requests().await;
        let start_time = Instant::now();

        let result = tokio::time::timeout(self.config.request_timeout, operation).await;
        
        let response_time = start_time.elapsed();
        self.decrement_concurrent_requests().await;

        match result {
            Ok(Ok(value)) => {
                self.record_success(response_time).await;
                Ok(value)
            }
            Ok(Err(error)) => {
                self.record_failure(response_time).await;
                Err(CircuitBreakerError::OperationFailed(error))
            }
            Err(_) => {
                self.record_failure(response_time).await;
                Err(CircuitBreakerError::Timeout)
            }
        }
    }

    async fn can_execute(&self) -> Result<bool> {
        let state = self.state.read().await;
        let concurrent = *self.concurrent_requests.read().await;

        if concurrent >= self.config.max_concurrent_requests {
            return Ok(false);
        }

        match &*state {
            CircuitBreakerState::Closed => Ok(true),
            CircuitBreakerState::Open { opened_at } => {
                if opened_at.elapsed() >= self.config.recovery_timeout {
                    drop(state);
                    self.transition_to_half_open().await;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            CircuitBreakerState::HalfOpen { test_requests } => {
                Ok(*test_requests < self.config.success_threshold)
            }
        }
    }

    async fn record_success(&self, response_time: Duration) {
        self.record_response_time(response_time).await;
        
        let mut success_count = self.success_count.write().await;
        *success_count += 1;

        let mut failure_count = self.failure_count.write().await;
        *failure_count = 0;

        let state = self.state.read().await.clone();
        match state {
            CircuitBreakerState::HalfOpen { test_requests } => {
                if test_requests + 1 >= self.config.success_threshold {
                    drop(state);
                    self.transition_to_closed().await;
                } else {
                    drop(state);
                    *self.state.write().await = CircuitBreakerState::HalfOpen { 
                        test_requests: test_requests + 1 
                    };
                }
            }
            _ => {}
        }

        self.update_metrics().await;
        debug!("Circuit breaker recorded success");
    }

    async fn record_failure(&self, response_time: Duration) {
        self.record_response_time(response_time).await;
        
        let mut failure_count = self.failure_count.write().await;
        *failure_count += 1;

        let current_failures = *failure_count;
        drop(failure_count);

        let state = self.state.read().await.clone();
        match state {
            CircuitBreakerState::Closed => {
                if current_failures >= self.config.failure_threshold {
                    drop(state);
                    self.transition_to_open().await;
                }
            }
            CircuitBreakerState::HalfOpen { .. } => {
                drop(state);
                self.transition_to_open().await;
            }
            _ => {}
        }

        let mut metrics = self.metrics.write().await;
        metrics.last_failure_time = Some(chrono::Utc::now().timestamp() as u64);
        
        self.update_metrics().await;
        warn!("Circuit breaker recorded failure (count: {})", current_failures);
    }

    async fn record_rejection(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.rejected_requests += 1;
        debug!("Circuit breaker rejected request");
    }

    async fn record_response_time(&self, response_time: Duration) {
        let mut response_times = self.response_times.write().await;
        response_times.push(response_time);

        if response_times.len() > 100 {
            response_times.remove(0);
        }
    }

    async fn transition_to_open(&self) {
        *self.state.write().await = CircuitBreakerState::Open { 
            opened_at: Instant::now() 
        };
        
        {
            let mut metrics = self.metrics.write().await;
            metrics.state_transitions += 1;
            metrics.state = "Open".to_string();
        }
        
        info!("Circuit breaker transitioned to OPEN state");
    }

    async fn transition_to_half_open(&self) {
        *self.state.write().await = CircuitBreakerState::HalfOpen { test_requests: 0 };
        
        {
            let mut metrics = self.metrics.write().await;
            metrics.state_transitions += 1;
            metrics.state = "HalfOpen".to_string();
        }
        
        info!("Circuit breaker transitioned to HALF-OPEN state");
    }

    async fn transition_to_closed(&self) {
        *self.state.write().await = CircuitBreakerState::Closed;
        *self.failure_count.write().await = 0;
        
        {
            let mut metrics = self.metrics.write().await;
            metrics.state_transitions += 1;
            metrics.state = "Closed".to_string();
            metrics.failure_count = 0;
        }
        
        info!("Circuit breaker transitioned to CLOSED state");
    }

    async fn increment_concurrent_requests(&self) {
        *self.concurrent_requests.write().await += 1;
    }

    async fn decrement_concurrent_requests(&self) {
        let mut concurrent = self.concurrent_requests.write().await;
        if *concurrent > 0 {
            *concurrent -= 1;
        }
    }

    async fn update_metrics(&self) {
        let state = self.state.read().await;
        let failure_count = *self.failure_count.read().await;
        let success_count = *self.success_count.read().await;
        let response_times = self.response_times.read().await;

        let average_response_time = if !response_times.is_empty() {
            let total_time: Duration = response_times.iter().sum();
            total_time.as_millis() as f64 / response_times.len() as f64
        } else {
            0.0
        };

        let mut metrics = self.metrics.write().await;
        metrics.state = match *state {
            CircuitBreakerState::Closed => "Closed".to_string(),
            CircuitBreakerState::Open { .. } => "Open".to_string(),
            CircuitBreakerState::HalfOpen { .. } => "HalfOpen".to_string(),
        };
        metrics.failure_count = failure_count;
        metrics.success_count = success_count;
        metrics.total_requests = (failure_count + success_count) as u64;
        metrics.average_response_time_ms = average_response_time;
    }

    pub async fn get_state(&self) -> CircuitBreakerState {
        self.state.read().await.clone()
    }

    pub async fn get_metrics(&self) -> CircuitBreakerMetrics {
        self.update_metrics().await;
        self.metrics.read().await.clone()
    }

    pub async fn force_open(&self) {
        self.transition_to_open().await;
        info!("Circuit breaker manually forced to OPEN state");
    }

    pub async fn force_closed(&self) {
        self.transition_to_closed().await;
        info!("Circuit breaker manually forced to CLOSED state");
    }

    pub async fn reset(&self) {
        *self.state.write().await = CircuitBreakerState::Closed;
        *self.failure_count.write().await = 0;
        *self.success_count.write().await = 0;
        *self.concurrent_requests.write().await = 0;
        self.response_times.write().await.clear();
        
        let mut metrics = self.metrics.write().await;
        *metrics = CircuitBreakerMetrics {
            state: "Closed".to_string(),
            failure_count: 0,
            success_count: 0,
            total_requests: 0,
            rejected_requests: 0,
            state_transitions: 0,
            average_response_time_ms: 0.0,
            last_failure_time: None,
        };
        
        info!("Circuit breaker reset to initial state");
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
            failure_count: self.failure_count.clone(),
            success_count: self.success_count.clone(),
            metrics: self.metrics.clone(),
            concurrent_requests: self.concurrent_requests.clone(),
            response_times: self.response_times.clone(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open")]
    CircuitOpen,
    #[error("Operation timed out")]
    Timeout,
    #[error("Operation failed: {0}")]
    OperationFailed(E),
}

#[derive(Debug)]
pub struct CircuitBreakerRegistry {
    breakers: Arc<RwLock<std::collections::HashMap<String, CircuitBreaker>>>,
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            default_config,
        }
    }

    pub async fn get_or_create(&self, name: &str) -> CircuitBreaker {
        let breakers = self.breakers.read().await;
        if let Some(breaker) = breakers.get(name) {
            return breaker.clone();
        }
        drop(breakers);

        let breaker = CircuitBreaker::new(self.default_config.clone());
        self.breakers.write().await.insert(name.to_string(), breaker.clone());
        
        info!("Created new circuit breaker: {}", name);
        breaker
    }

    pub async fn get(&self, name: &str) -> Option<CircuitBreaker> {
        self.breakers.read().await.get(name).cloned()
    }

    pub async fn remove(&self, name: &str) -> Option<CircuitBreaker> {
        self.breakers.write().await.remove(name)
    }

    pub async fn list_all(&self) -> Vec<(String, CircuitBreaker)> {
        self.breakers.read().await.iter()
            .map(|(name, breaker)| (name.clone(), breaker.clone()))
            .collect()
    }

    pub async fn get_all_metrics(&self) -> std::collections::HashMap<String, CircuitBreakerMetrics> {
        let breakers = self.breakers.read().await;
        let mut metrics = std::collections::HashMap::new();
        
        for (name, breaker) in breakers.iter() {
            metrics.insert(name.clone(), breaker.get_metrics().await);
        }
        
        metrics
    }

    pub async fn reset_all(&self) {
        let breakers = self.breakers.read().await;
        for breaker in breakers.values() {
            breaker.reset().await;
        }
        info!("Reset all circuit breakers");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);
        
        let result = breaker.call(async { Ok::<i32, &str>(42) }).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        
        let state = breaker.get_state().await;
        assert_eq!(state, CircuitBreakerState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        
        for _ in 0..2 {
            let _ = breaker.call(async { Err::<i32, &str>("error") }).await;
        }
        
        let state = breaker.get_state().await;
        assert!(matches!(state, CircuitBreakerState::Open { .. }));
    }

    #[tokio::test]
    async fn test_circuit_breaker_rejects_when_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_secs(60),
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        
        let _ = breaker.call(async { Err::<i32, &str>("error") }).await;
        
        let result = breaker.call(async { Ok::<i32, &str>(42) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::CircuitOpen)));
    }

    #[tokio::test]
    async fn test_circuit_breaker_timeout() {
        let config = CircuitBreakerConfig {
            request_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let breaker = CircuitBreaker::new(config);
        
        let result = breaker.call(async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<i32, &str>(42)
        }).await;
        
        assert!(matches!(result, Err(CircuitBreakerError::Timeout)));
    }

    #[tokio::test]
    async fn test_circuit_breaker_registry() {
        let config = CircuitBreakerConfig::default();
        let registry = CircuitBreakerRegistry::new(config);
        
        let breaker1 = registry.get_or_create("test1").await;
        let breaker2 = registry.get_or_create("test1").await;
        
        assert!(std::ptr::eq(&*breaker1.state, &*breaker2.state));
        
        let all_breakers = registry.list_all().await;
        assert_eq!(all_breakers.len(), 1);
    }

    #[tokio::test]
    async fn test_circuit_breaker_metrics() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);
        
        let _ = breaker.call(async { Ok::<i32, &str>(42) }).await;
        let _ = breaker.call(async { Err::<i32, &str>("error") }).await;
        
        let metrics = breaker.get_metrics().await;
        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.success_count, 1);
        assert_eq!(metrics.failure_count, 1);
    }
}