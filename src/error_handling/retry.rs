use eyre::Result;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub jitter: bool,
    pub retry_on_timeout: bool,
    pub retry_predicates: Vec<String>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
            jitter: true,
            retry_on_timeout: true,
            retry_predicates: vec![
                "connection".to_string(),
                "timeout".to_string(),
                "unavailable".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryMetrics {
    pub total_attempts: u64,
    pub successful_retries: u64,
    pub failed_retries: u64,
    pub average_attempts_per_operation: f64,
    pub total_retry_delay_ms: u64,
    pub max_attempts_reached: u64,
}

impl Default for RetryMetrics {
    fn default() -> Self {
        Self {
            total_attempts: 0,
            successful_retries: 0,
            failed_retries: 0,
            average_attempts_per_operation: 0.0,
            total_retry_delay_ms: 0,
            max_attempts_reached: 0,
        }
    }
}

#[derive(Debug)]
pub struct RetryPolicy {
    config: RetryConfig,
    metrics: std::sync::Arc<tokio::sync::RwLock<RetryMetrics>>,
}

impl RetryPolicy {
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            metrics: std::sync::Arc::new(tokio::sync::RwLock::new(RetryMetrics::default())),
        }
    }

    pub async fn execute<F, T, E>(&self, operation: F) -> Result<T, RetryError<E>>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
        E: std::fmt::Display + Send + Sync + 'static,
    {
        let mut attempt = 1;
        let mut total_delay = Duration::ZERO;
        let start_time = Instant::now();

        loop {
            debug!("Retry attempt {} of {}", attempt, self.config.max_attempts);
            
            let attempt_start = Instant::now();
            let result = operation().await;
            let attempt_duration = attempt_start.elapsed();

            self.record_attempt().await;

            match result {
                Ok(value) => {
                    if attempt > 1 {
                        self.record_successful_retry(attempt, total_delay).await;
                        info!("Operation succeeded after {} attempts in {:?}", attempt, start_time.elapsed());
                    }
                    return Ok(value);
                }
                Err(error) => {
                    if !self.should_retry(&error, attempt).await {
                        self.record_failed_retry(attempt, total_delay).await;
                        return Err(RetryError::MaxAttemptsExceeded {
                            attempts: attempt,
                            last_error: Box::new(error),
                        });
                    }

                    if attempt >= self.config.max_attempts {
                        self.record_max_attempts_reached().await;
                        return Err(RetryError::MaxAttemptsExceeded {
                            attempts: attempt,
                            last_error: Box::new(error),
                        });
                    }

                    let delay = self.calculate_delay(attempt);
                    total_delay += delay;

                    warn!("Attempt {} failed: {}. Retrying in {:?}", attempt, error, delay);
                    
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn should_retry<E>(&self, error: &E, attempt: u32) -> bool
    where
        E: std::fmt::Display,
    {
        if attempt >= self.config.max_attempts {
            return false;
        }

        let error_str = error.to_string().to_lowercase();
        
        for predicate in &self.config.retry_predicates {
            if error_str.contains(predicate) {
                return true;
            }
        }

        false
    }

    fn calculate_delay(&self, attempt: u32) -> Duration {
        let mut delay = Duration::from_millis(
            (self.config.initial_delay.as_millis() as f64 * 
             self.config.multiplier.powi((attempt - 1) as i32)) as u64
        );

        if delay > self.config.max_delay {
            delay = self.config.max_delay;
        }

        if self.config.jitter {
            let jitter_amount = delay.as_millis() as f64 * 0.1;
            let jitter = (rand::random::<f64>() - 0.5) * 2.0 * jitter_amount;
            let jittered_delay = (delay.as_millis() as f64 + jitter).max(0.0) as u64;
            delay = Duration::from_millis(jittered_delay);
        }

        delay
    }

    async fn record_attempt(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.total_attempts += 1;
    }

    async fn record_successful_retry(&self, attempts: u32, total_delay: Duration) {
        let mut metrics = self.metrics.write().await;
        metrics.successful_retries += 1;
        metrics.total_retry_delay_ms += total_delay.as_millis() as u64;
        
        let total_operations = metrics.successful_retries + metrics.failed_retries;
        if total_operations > 0 {
            metrics.average_attempts_per_operation = 
                metrics.total_attempts as f64 / total_operations as f64;
        }
    }

    async fn record_failed_retry(&self, attempts: u32, total_delay: Duration) {
        let mut metrics = self.metrics.write().await;
        metrics.failed_retries += 1;
        metrics.total_retry_delay_ms += total_delay.as_millis() as u64;
        
        let total_operations = metrics.successful_retries + metrics.failed_retries;
        if total_operations > 0 {
            metrics.average_attempts_per_operation = 
                metrics.total_attempts as f64 / total_operations as f64;
        }
    }

    async fn record_max_attempts_reached(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.max_attempts_reached += 1;
    }

    pub async fn get_metrics(&self) -> RetryMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn reset_metrics(&self) {
        *self.metrics.write().await = RetryMetrics::default();
    }
}

impl Clone for RetryPolicy {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RetryError<E> {
    #[error("Maximum retry attempts ({attempts}) exceeded. Last error: {last_error}")]
    MaxAttemptsExceeded {
        attempts: u32,
        last_error: Box<E>,
    },
}

#[derive(Debug)]
pub struct ExponentialBackoff {
    config: RetryConfig,
}

impl ExponentialBackoff {
    pub fn new() -> Self {
        Self {
            config: RetryConfig::default(),
        }
    }

    pub fn with_config(config: RetryConfig) -> Self {
        Self { config }
    }

    pub fn max_attempts(mut self, max_attempts: u32) -> Self {
        self.config.max_attempts = max_attempts;
        self
    }

    pub fn initial_delay(mut self, delay: Duration) -> Self {
        self.config.initial_delay = delay;
        self
    }

    pub fn max_delay(mut self, delay: Duration) -> Self {
        self.config.max_delay = delay;
        self
    }

    pub fn multiplier(mut self, multiplier: f64) -> Self {
        self.config.multiplier = multiplier;
        self
    }

    pub fn with_jitter(mut self, enable: bool) -> Self {
        self.config.jitter = enable;
        self
    }

    pub fn retry_on_timeout(mut self, enable: bool) -> Self {
        self.config.retry_on_timeout = enable;
        self
    }

    pub fn add_retry_predicate(mut self, predicate: String) -> Self {
        self.config.retry_predicates.push(predicate);
        self
    }

    pub fn build(self) -> RetryPolicy {
        RetryPolicy::new(self.config)
    }
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn retry_with_backoff<F, T, E>(
    operation: F,
    config: RetryConfig,
) -> Result<T, RetryError<E>>
where
    F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
    E: std::fmt::Display + Send + Sync + 'static,
{
    let policy = RetryPolicy::new(config);
    policy.execute(operation).await
}

pub fn retry_async<F, T, E, Fut>(
    operation: F,
) -> impl std::future::Future<Output = Result<T, RetryError<E>>>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display + Send + Sync + 'static,
{
    let policy = RetryPolicy::new(RetryConfig::default());
    
    policy.execute(move || {
        Box::pin(operation()) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>
    })
}

#[derive(Debug)]
pub struct RetryPolicyRegistry {
    policies: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, RetryPolicy>>>,
    default_config: RetryConfig,
}

impl RetryPolicyRegistry {
    pub fn new(default_config: RetryConfig) -> Self {
        Self {
            policies: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            default_config,
        }
    }

    pub async fn get_or_create(&self, name: &str) -> RetryPolicy {
        let policies = self.policies.read().await;
        if let Some(policy) = policies.get(name) {
            return policy.clone();
        }
        drop(policies);

        let policy = RetryPolicy::new(self.default_config.clone());
        self.policies.write().await.insert(name.to_string(), policy.clone());
        
        info!("Created new retry policy: {}", name);
        policy
    }

    pub async fn register(&self, name: String, policy: RetryPolicy) {
        self.policies.write().await.insert(name.clone(), policy);
        info!("Registered retry policy: {}", name);
    }

    pub async fn get(&self, name: &str) -> Option<RetryPolicy> {
        self.policies.read().await.get(name).cloned()
    }

    pub async fn remove(&self, name: &str) -> Option<RetryPolicy> {
        self.policies.write().await.remove(name)
    }

    pub async fn list_all(&self) -> Vec<(String, RetryPolicy)> {
        self.policies.read().await.iter()
            .map(|(name, policy)| (name.clone(), policy.clone()))
            .collect()
    }

    pub async fn get_all_metrics(&self) -> std::collections::HashMap<String, RetryMetrics> {
        let policies = self.policies.read().await;
        let mut metrics = std::collections::HashMap::new();
        
        for (name, policy) in policies.iter() {
            metrics.insert(name.clone(), policy.get_metrics().await);
        }
        
        metrics
    }

    pub async fn reset_all_metrics(&self) {
        let policies = self.policies.read().await;
        for policy in policies.values() {
            policy.reset_metrics().await;
        }
        info!("Reset all retry policy metrics");
    }
}

macro_rules! retry {
    ($operation:expr) => {
        retry_async(|| async { $operation }).await
    };
    ($operation:expr, $config:expr) => {
        retry_with_backoff(|| Box::pin(async { $operation }), $config).await
    };
}

pub use retry;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_retry_policy_success_on_first_attempt() {
        let config = RetryConfig::default();
        let policy = RetryPolicy::new(config);
        
        let result = policy.execute(|| Box::pin(async { Ok::<i32, &str>(42) })).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        
        let metrics = policy.get_metrics().await;
        assert_eq!(metrics.total_attempts, 1);
        assert_eq!(metrics.successful_retries, 0);
    }

    #[tokio::test]
    async fn test_retry_policy_success_after_retries() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(1),
            ..Default::default()
        };
        let policy = RetryPolicy::new(config);
        
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        
        let result = policy.execute(move || {
            let counter = counter_clone.clone();
            Box::pin(async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err("temporary failure")
                } else {
                    Ok(42)
                }
            })
        }).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
        
        let metrics = policy.get_metrics().await;
        assert_eq!(metrics.total_attempts, 3);
        assert_eq!(metrics.successful_retries, 1);
    }

    #[tokio::test]
    async fn test_retry_policy_max_attempts_exceeded() {
        let config = RetryConfig {
            max_attempts: 2,
            initial_delay: Duration::from_millis(1),
            retry_predicates: vec!["failure".to_string()],
            ..Default::default()
        };
        let policy = RetryPolicy::new(config);
        
        let result = policy.execute(|| Box::pin(async { Err::<i32, &str>("persistent failure") })).await;
        
        assert!(result.is_err());
        match result {
            Err(RetryError::MaxAttemptsExceeded { attempts, .. }) => {
                assert_eq!(attempts, 2);
            }
        }
        
        let metrics = policy.get_metrics().await;
        assert_eq!(metrics.total_attempts, 2);
        assert_eq!(metrics.failed_retries, 1);
        assert_eq!(metrics.max_attempts_reached, 1);
    }

    #[tokio::test]
    async fn test_exponential_backoff_builder() {
        let policy = ExponentialBackoff::new()
            .max_attempts(5)
            .initial_delay(Duration::from_millis(50))
            .max_delay(Duration::from_secs(5))
            .multiplier(2.5)
            .with_jitter(true)
            .add_retry_predicate("test_error".to_string())
            .build();
        
        assert_eq!(policy.config.max_attempts, 5);
        assert_eq!(policy.config.initial_delay, Duration::from_millis(50));
        assert_eq!(policy.config.max_delay, Duration::from_secs(5));
        assert_eq!(policy.config.multiplier, 2.5);
        assert!(policy.config.jitter);
        assert!(policy.config.retry_predicates.contains(&"test_error".to_string()));
    }

    #[tokio::test]
    async fn test_retry_policy_registry() {
        let config = RetryConfig::default();
        let registry = RetryPolicyRegistry::new(config);
        
        let policy1 = registry.get_or_create("test1").await;
        let policy2 = registry.get_or_create("test1").await;
        
        assert!(std::ptr::eq(&*policy1.metrics, &*policy2.metrics));
        
        let all_policies = registry.list_all().await;
        assert_eq!(all_policies.len(), 1);
    }

    #[tokio::test]
    async fn test_delay_calculation() {
        let config = RetryConfig {
            initial_delay: Duration::from_millis(100),
            multiplier: 2.0,
            max_delay: Duration::from_secs(1),
            jitter: false,
            ..Default::default()
        };
        let policy = RetryPolicy::new(config);
        
        assert_eq!(policy.calculate_delay(1), Duration::from_millis(100));
        assert_eq!(policy.calculate_delay(2), Duration::from_millis(200));
        assert_eq!(policy.calculate_delay(3), Duration::from_millis(400));
        assert_eq!(policy.calculate_delay(4), Duration::from_millis(800));
        assert_eq!(policy.calculate_delay(5), Duration::from_secs(1));
        assert_eq!(policy.calculate_delay(6), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_retry_macro() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        
        let result = retry!({
            let count = counter_clone.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Err::<i32, &str>("temporary error")
            } else {
                Ok(42)
            }
        });
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }
}