//! Circuit breaker pattern implementation
//!
//! This module provides circuit breaker patterns to prevent cascading failures
//! and provide graceful degradation during network partitions or overload.

use tokio::time::{Duration, Instant};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use crate::error::{RaftError, Result};

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    /// Normal operation - all requests pass through
    Closed,
    /// Failure threshold exceeded - all requests fail fast
    Open,
    /// Testing if service has recovered - limited requests pass through
    HalfOpen,
}

/// Configuration for circuit breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// Duration to wait before attempting to close circuit
    pub timeout: Duration,
    /// Number of successful requests needed to close circuit from half-open
    pub success_threshold: u32,
    /// Maximum number of requests allowed in half-open state
    pub half_open_max_requests: u32,
    /// Window size for failure rate calculation
    pub failure_rate_window: u32,
    /// Minimum throughput required before calculating failure rate
    pub minimum_throughput: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            timeout: Duration::from_secs(60),
            success_threshold: 3,
            half_open_max_requests: 5,
            failure_rate_window: 20,
            minimum_throughput: 10,
        }
    }
}

/// Circuit breaker metrics
#[derive(Debug, Default)]
pub struct CircuitBreakerMetrics {
    /// Total number of requests
    pub total_requests: AtomicU64,
    /// Total number of successful requests
    pub total_successes: AtomicU64,
    /// Total number of failed requests
    pub total_failures: AtomicU64,
    /// Number of requests rejected by circuit breaker
    pub rejected_requests: AtomicU64,
    /// Number of times circuit has opened
    pub circuit_opened_count: AtomicU32,
    /// Number of times circuit has closed
    pub circuit_closed_count: AtomicU32,
}

impl CircuitBreakerMetrics {
    pub fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_success(&self) {
        self.total_successes.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_failure(&self) {
        self.total_failures.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_rejection(&self) {
        self.rejected_requests.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_circuit_opened(&self) {
        self.circuit_opened_count.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_circuit_closed(&self) {
        self.circuit_closed_count.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn get_failure_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let failures = self.total_failures.load(Ordering::Relaxed);
        failures as f64 / total as f64
    }
    
    pub fn get_snapshot(&self) -> CircuitBreakerSnapshot {
        CircuitBreakerSnapshot {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_successes: self.total_successes.load(Ordering::Relaxed),
            total_failures: self.total_failures.load(Ordering::Relaxed),
            rejected_requests: self.rejected_requests.load(Ordering::Relaxed),
            circuit_opened_count: self.circuit_opened_count.load(Ordering::Relaxed),
            circuit_closed_count: self.circuit_closed_count.load(Ordering::Relaxed),
            failure_rate: self.get_failure_rate(),
        }
    }
}

/// Snapshot of circuit breaker metrics
#[derive(Debug, Clone)]
pub struct CircuitBreakerSnapshot {
    pub total_requests: u64,
    pub total_successes: u64,
    pub total_failures: u64,
    pub rejected_requests: u64,
    pub circuit_opened_count: u32,
    pub circuit_closed_count: u32,
    pub failure_rate: f64,
}

/// Circuit breaker implementation
pub struct CircuitBreaker {
    state: Arc<tokio::sync::RwLock<CircuitBreakerState>>,
    config: CircuitBreakerConfig,
    metrics: Arc<CircuitBreakerMetrics>,
    last_failure_time: Arc<tokio::sync::RwLock<Option<Instant>>>,
    half_open_requests: Arc<AtomicU32>,
    half_open_successes: Arc<AtomicU32>,
    recent_failures: Arc<tokio::sync::RwLock<std::collections::VecDeque<Instant>>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(tokio::sync::RwLock::new(CircuitBreakerState::Closed)),
            config,
            metrics: Arc::new(CircuitBreakerMetrics::default()),
            last_failure_time: Arc::new(tokio::sync::RwLock::new(None)),
            half_open_requests: Arc::new(AtomicU32::new(0)),
            half_open_successes: Arc::new(AtomicU32::new(0)),
            recent_failures: Arc::new(tokio::sync::RwLock::new(std::collections::VecDeque::new())),
        }
    }
    
    /// Execute a function with circuit breaker protection
    pub async fn call<F, T, E>(&self, f: F) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
    {
        // Check if request should be allowed
        if !self.allow_request().await {
            self.metrics.record_rejection();
            return Err(RaftError::CircuitBreakerOpen {
                message: "Circuit breaker is open".to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }
        
        self.metrics.record_request();
        
        // Execute the function
        match f.await {
            Ok(result) => {
                self.on_success().await;
                self.metrics.record_success();
                Ok(result)
            }
            Err(e) => {
                self.on_failure().await;
                self.metrics.record_failure();
                Err(RaftError::Internal {
                    message: format!("Circuit breaker protected operation failed: {}", e),
                    backtrace: snafu::Backtrace::new(),
                })
            }
        }
    }
    
    /// Check if request should be allowed
    async fn allow_request(&self) -> bool {
        let state = *self.state.read().await;
        
        match state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = *self.last_failure_time.read().await {
                    if last_failure.elapsed() > self.config.timeout {
                        // Transition to half-open
                        *self.state.write().await = CircuitBreakerState::HalfOpen;
                        self.half_open_requests.store(0, Ordering::Relaxed);
                        self.half_open_successes.store(0, Ordering::Relaxed);
                        return true;
                    }
                }
                false
            }
            CircuitBreakerState::HalfOpen => {
                // Allow limited requests in half-open state
                let current_requests = self.half_open_requests.load(Ordering::Relaxed);
                if current_requests < self.config.half_open_max_requests {
                    self.half_open_requests.fetch_add(1, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
        }
    }
    
    /// Handle successful request
    async fn on_success(&self) {
        let state = *self.state.read().await;
        
        match state {
            CircuitBreakerState::HalfOpen => {
                let successes = self.half_open_successes.fetch_add(1, Ordering::Relaxed) + 1;
                if successes >= self.config.success_threshold {
                    // Close circuit
                    *self.state.write().await = CircuitBreakerState::Closed;
                    *self.last_failure_time.write().await = None;
                    self.recent_failures.write().await.clear();
                    self.metrics.record_circuit_closed();
                }
            }
            _ => {
                // In closed state, just clear old failures
                self.clean_old_failures().await;
            }
        }
    }
    
    /// Handle failed request
    async fn on_failure(&self) {
        let now = Instant::now();
        *self.last_failure_time.write().await = Some(now);
        
        // Add to recent failures
        self.recent_failures.write().await.push_back(now);
        self.clean_old_failures().await;
        
        let state = *self.state.read().await;
        
        match state {
            CircuitBreakerState::Closed => {
                // Check if we should open circuit
                let recent_failures = self.recent_failures.read().await;
                if recent_failures.len() >= self.config.failure_threshold as usize {
                    drop(recent_failures);
                    *self.state.write().await = CircuitBreakerState::Open;
                    self.metrics.record_circuit_opened();
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Any failure in half-open state opens circuit
                *self.state.write().await = CircuitBreakerState::Open;
                self.metrics.record_circuit_opened();
            }
            CircuitBreakerState::Open => {
                // Already open, just update timing
            }
        }
    }
    
    /// Clean old failures outside the window
    async fn clean_old_failures(&self) {
        let mut failures = self.recent_failures.write().await;
        let cutoff = Instant::now() - Duration::from_secs(60); // 1 minute window
        
        while let Some(&front) = failures.front() {
            if front < cutoff {
                failures.pop_front();
            } else {
                break;
            }
        }
    }
    
    /// Get current state
    pub async fn state(&self) -> CircuitBreakerState {
        *self.state.read().await
    }
    
    /// Get metrics snapshot
    pub fn metrics(&self) -> CircuitBreakerSnapshot {
        self.metrics.get_snapshot()
    }
    
    /// Force circuit to open (for testing)
    pub async fn force_open(&self) {
        *self.state.write().await = CircuitBreakerState::Open;
        *self.last_failure_time.write().await = Some(Instant::now());
        self.metrics.record_circuit_opened();
    }
    
    /// Force circuit to close (for testing)
    pub async fn force_close(&self) {
        *self.state.write().await = CircuitBreakerState::Closed;
        *self.last_failure_time.write().await = None;
        self.recent_failures.write().await.clear();
        self.metrics.record_circuit_closed();
    }
}

impl Clone for CircuitBreaker {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            config: self.config.clone(),
            metrics: Arc::clone(&self.metrics),
            last_failure_time: Arc::clone(&self.last_failure_time),
            half_open_requests: Arc::clone(&self.half_open_requests),
            half_open_successes: Arc::clone(&self.half_open_successes),
            recent_failures: Arc::clone(&self.recent_failures),
        }
    }
}

/// Circuit breaker manager for multiple services
pub struct CircuitBreakerManager {
    breakers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Arc<CircuitBreaker>>>>,
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerManager {
    /// Create a new circuit breaker manager
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            default_config,
        }
    }
    
    /// Get or create circuit breaker for a service
    pub async fn get_breaker(&self, service_name: &str) -> Arc<CircuitBreaker> {
        {
            let breakers = self.breakers.read().await;
            if let Some(breaker) = breakers.get(service_name) {
                return Arc::clone(breaker);
            }
        }
        
        // Create new breaker
        let breaker = Arc::new(CircuitBreaker::new(self.default_config.clone()));
        
        {
            let mut breakers = self.breakers.write().await;
            breakers.insert(service_name.to_string(), Arc::clone(&breaker));
        }
        
        breaker
    }
    
    /// Get all circuit breaker states
    pub async fn get_all_states(&self) -> std::collections::HashMap<String, CircuitBreakerState> {
        let breakers = self.breakers.read().await;
        let mut states = std::collections::HashMap::new();
        
        for (name, breaker) in breakers.iter() {
            states.insert(name.clone(), breaker.state().await);
        }
        
        states
    }
    
    /// Get all metrics
    pub async fn get_all_metrics(&self) -> std::collections::HashMap<String, CircuitBreakerSnapshot> {
        let breakers = self.breakers.read().await;
        let mut metrics = std::collections::HashMap::new();
        
        for (name, breaker) in breakers.iter() {
            metrics.insert(name.clone(), breaker.metrics());
        }
        
        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;
    
    #[tokio::test]
    async fn test_circuit_breaker_basic() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout: Duration::from_millis(100),
            success_threshold: 1,
            ..Default::default()
        };
        
        let breaker = CircuitBreaker::new(config);
        
        // Should start closed
        assert_eq!(breaker.state().await, CircuitBreakerState::Closed);
        
        // Successful calls should work
        let result = breaker.call(async { Ok::<_, &str>("success") }).await;
        assert!(result.is_ok());
        
        // Failures should eventually open circuit
        let _ = breaker.call(async { Err::<&str, _>("failure1") }).await;
        let _ = breaker.call(async { Err::<&str, _>("failure2") }).await;
        
        assert_eq!(breaker.state().await, CircuitBreakerState::Open);
        
        // Requests should be rejected
        let result = breaker.call(async { Ok::<_, &str>("success") }).await;
        assert!(result.is_err());
        
        // Wait for timeout
        sleep(Duration::from_millis(150)).await;
        
        // Should allow requests in half-open
        let result = breaker.call(async { Ok::<_, &str>("success") }).await;
        assert!(result.is_ok());
        
        // Should close circuit after success
        assert_eq!(breaker.state().await, CircuitBreakerState::Closed);
    }
    
    #[tokio::test]
    async fn test_circuit_breaker_manager() {
        let config = CircuitBreakerConfig::default();
        let manager = CircuitBreakerManager::new(config);
        
        let breaker1 = manager.get_breaker("service1").await;
        let breaker2 = manager.get_breaker("service2").await;
        
        // Should be different instances
        assert!(!Arc::ptr_eq(&breaker1, &breaker2));
        
        // Should return same instance for same service
        let breaker1_again = manager.get_breaker("service1").await;
        assert!(Arc::ptr_eq(&breaker1, &breaker1_again));
        
        // Test states
        breaker1.force_open().await;
        let states = manager.get_all_states().await;
        assert_eq!(states["service1"], CircuitBreakerState::Open);
        assert_eq!(states["service2"], CircuitBreakerState::Closed);
    }
}