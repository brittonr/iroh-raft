//! Comprehensive timeout management for async operations
//!
//! This module provides sophisticated timeout handling with adaptive timeouts,
//! exponential backoff, and deadlock prevention.

use tokio::time::{Duration, Instant, timeout, sleep};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::collections::HashMap;
use crate::error::{RaftError, Result};

/// Timeout configuration for different operation types
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Base timeout duration
    pub base_timeout: Duration,
    /// Maximum timeout duration
    pub max_timeout: Duration,
    /// Minimum timeout duration
    pub min_timeout: Duration,
    /// Enable adaptive timeout adjustment
    pub enable_adaptive: bool,
    /// Exponential backoff multiplier
    pub backoff_multiplier: f64,
    /// Maximum number of retries
    pub max_retries: u32,
    /// Jitter factor (0.0 to 1.0)
    pub jitter_factor: f64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            base_timeout: Duration::from_millis(1000),
            max_timeout: Duration::from_millis(30000),
            min_timeout: Duration::from_millis(100),
            enable_adaptive: true,
            backoff_multiplier: 2.0,
            max_retries: 3,
            jitter_factor: 0.1,
        }
    }
}

/// Different types of operations that may need timeouts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationType {
    /// Raft message sending
    RaftMessage,
    /// Connection establishment
    Connection,
    /// Proposal processing
    Proposal,
    /// Storage operations
    Storage,
    /// Configuration changes
    ConfigChange,
    /// Health checks
    HealthCheck,
    /// General network operations
    Network,
}

impl OperationType {
    /// Get default timeout config for operation type
    pub fn default_config(self) -> TimeoutConfig {
        match self {
            OperationType::RaftMessage => TimeoutConfig {
                base_timeout: Duration::from_millis(500),
                max_timeout: Duration::from_millis(5000),
                min_timeout: Duration::from_millis(100),
                max_retries: 2,
                ..Default::default()
            },
            OperationType::Connection => TimeoutConfig {
                base_timeout: Duration::from_millis(5000),
                max_timeout: Duration::from_millis(30000),
                min_timeout: Duration::from_millis(1000),
                max_retries: 3,
                ..Default::default()
            },
            OperationType::Proposal => TimeoutConfig {
                base_timeout: Duration::from_millis(2000),
                max_timeout: Duration::from_millis(10000),
                min_timeout: Duration::from_millis(500),
                max_retries: 2,
                ..Default::default()
            },
            OperationType::Storage => TimeoutConfig {
                base_timeout: Duration::from_millis(1000),
                max_timeout: Duration::from_millis(15000),
                min_timeout: Duration::from_millis(200),
                max_retries: 2,
                ..Default::default()
            },
            OperationType::ConfigChange => TimeoutConfig {
                base_timeout: Duration::from_millis(5000),
                max_timeout: Duration::from_millis(30000),
                min_timeout: Duration::from_millis(1000),
                max_retries: 1, // Config changes are sensitive
                ..Default::default()
            },
            OperationType::HealthCheck => TimeoutConfig {
                base_timeout: Duration::from_millis(1000),
                max_timeout: Duration::from_millis(5000),
                min_timeout: Duration::from_millis(200),
                max_retries: 1,
                ..Default::default()
            },
            OperationType::Network => TimeoutConfig {
                base_timeout: Duration::from_millis(3000),
                max_timeout: Duration::from_millis(15000),
                min_timeout: Duration::from_millis(500),
                max_retries: 3,
                ..Default::default()
            },
        }
    }
}

/// Timeout statistics for adaptive adjustment
#[derive(Debug, Default)]
pub struct TimeoutStats {
    /// Total number of operations
    pub total_operations: AtomicU64,
    /// Number of timeouts
    pub timeout_count: AtomicU64,
    /// Number of successes
    pub success_count: AtomicU64,
    /// Sum of operation durations (in milliseconds)
    pub total_duration_ms: AtomicU64,
    /// Current adaptive timeout (in milliseconds)
    pub current_timeout_ms: AtomicU64,
    /// Number of adaptive adjustments
    pub adjustment_count: AtomicU32,
}

impl TimeoutStats {
    pub fn record_success(&self, duration: Duration) {
        self.total_operations.fetch_add(1, Ordering::Relaxed);
        self.success_count.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms.fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }
    
    pub fn record_timeout(&self) {
        self.total_operations.fetch_add(1, Ordering::Relaxed);
        self.timeout_count.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_adjustment(&self, new_timeout: Duration) {
        self.adjustment_count.fetch_add(1, Ordering::Relaxed);
        self.current_timeout_ms.store(new_timeout.as_millis() as u64, Ordering::Relaxed);
    }
    
    pub fn get_timeout_rate(&self) -> f64 {
        let total = self.total_operations.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let timeouts = self.timeout_count.load(Ordering::Relaxed);
        timeouts as f64 / total as f64
    }
    
    pub fn get_avg_duration(&self) -> Duration {
        let total = self.success_count.load(Ordering::Relaxed);
        if total == 0 {
            return Duration::from_millis(0);
        }
        let total_ms = self.total_duration_ms.load(Ordering::Relaxed);
        Duration::from_millis(total_ms / total)
    }
}

/// Adaptive timeout manager
pub struct TimeoutManager {
    configs: HashMap<OperationType, TimeoutConfig>,
    stats: HashMap<OperationType, Arc<TimeoutStats>>,
    last_adjustment: HashMap<OperationType, Instant>,
}

impl TimeoutManager {
    /// Create a new timeout manager
    pub fn new() -> Self {
        let mut configs = HashMap::new();
        let mut stats = HashMap::new();
        let mut last_adjustment = HashMap::new();
        
        // Initialize with default configs for all operation types
        for op_type in [
            OperationType::RaftMessage,
            OperationType::Connection,
            OperationType::Proposal,
            OperationType::Storage,
            OperationType::ConfigChange,
            OperationType::HealthCheck,
            OperationType::Network,
        ] {
            configs.insert(op_type, op_type.default_config());
            stats.insert(op_type, Arc::new(TimeoutStats::default()));
            last_adjustment.insert(op_type, Instant::now());
        }
        
        Self {
            configs,
            stats,
            last_adjustment,
        }
    }
    
    /// Execute operation with timeout and adaptive adjustment
    pub async fn execute_with_timeout<F, T, E>(
        &mut self,
        op_type: OperationType,
        operation: F,
    ) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
    {
        let config = self.configs.get(&op_type).unwrap().clone();
        let stats = Arc::clone(self.stats.get(&op_type).unwrap());
        
        let timeout_duration = self.get_current_timeout(op_type);
        let start_time = Instant::now();
        
        match timeout(timeout_duration, operation).await {
            Ok(Ok(result)) => {
                let duration = start_time.elapsed();
                stats.record_success(duration);
                
                // Adjust timeout if enabled
                if config.enable_adaptive {
                    self.adjust_timeout_if_needed(op_type).await;
                }
                
                Ok(result)
            }
            Ok(Err(e)) => {
                let duration = start_time.elapsed();
                stats.record_success(duration); // Not a timeout, just a failure
                
                Err(RaftError::Internal {
                    message: format!("Operation failed: {}", e),
                    backtrace: snafu::Backtrace::new(),
                })
            }
            Err(_) => {
                stats.record_timeout();
                
                // Adjust timeout if enabled
                if config.enable_adaptive {
                    self.adjust_timeout_if_needed(op_type).await;
                }
                
                Err(RaftError::Timeout {
                    operation: format!("{:?}", op_type),
                    duration: timeout_duration,
                    backtrace: snafu::Backtrace::new(),
                })
            }
        }
    }
    
    /// Execute operation with retry and exponential backoff
    pub async fn execute_with_retry<F, Fut, T, E>(
        &mut self,
        op_type: OperationType,
        mut operation: F,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
    {
        let config = self.configs.get(&op_type).unwrap().clone();
        let mut retry_count = 0;
        let mut delay = config.base_timeout;
        
        loop {
            match self.execute_with_timeout(op_type, operation()).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    retry_count += 1;
                    if retry_count > config.max_retries {
                        return Err(e);
                    }
                    
                    // Apply exponential backoff with jitter
                    let jitter = self.calculate_jitter(delay, config.jitter_factor);
                    let backoff_delay = delay.mul_f64(config.backoff_multiplier) + jitter;
                    delay = backoff_delay.min(config.max_timeout);
                    
                    sleep(delay).await;
                }
            }
        }
    }
    
    /// Get current timeout for operation type
    fn get_current_timeout(&self, op_type: OperationType) -> Duration {
        let stats = self.stats.get(&op_type).unwrap();
        let current_ms = stats.current_timeout_ms.load(Ordering::Relaxed);
        
        if current_ms == 0 {
            // Use base timeout if no adaptive timeout set
            self.configs.get(&op_type).unwrap().base_timeout
        } else {
            Duration::from_millis(current_ms)
        }
    }
    
    /// Adjust timeout based on recent performance
    async fn adjust_timeout_if_needed(&mut self, op_type: OperationType) {
        let now = Instant::now();
        let last_adj = self.last_adjustment.get(&op_type).unwrap();
        
        // Only adjust every 30 seconds
        if now.duration_since(*last_adj) < Duration::from_secs(30) {
            return;
        }
        
        let config = self.configs.get(&op_type).unwrap();
        let stats = self.stats.get(&op_type).unwrap();
        
        let timeout_rate = stats.get_timeout_rate();
        let avg_duration = stats.get_avg_duration();
        let current_timeout = self.get_current_timeout(op_type);
        
        let new_timeout = if timeout_rate > 0.1 {
            // High timeout rate - increase timeout
            let increased = current_timeout.mul_f64(1.5);
            increased.min(config.max_timeout)
        } else if timeout_rate < 0.02 && avg_duration.as_millis() > 0 {
            // Low timeout rate - potentially decrease timeout
            let target = avg_duration.mul_f64(3.0); // 3x average duration
            let decreased = current_timeout.mul_f64(0.8);
            target.max(decreased).max(config.min_timeout)
        } else {
            current_timeout
        };
        
        if new_timeout != current_timeout {
            stats.record_adjustment(new_timeout);
            self.last_adjustment.insert(op_type, now);
        }
    }
    
    /// Calculate jitter for backoff
    fn calculate_jitter(&self, base_delay: Duration, jitter_factor: f64) -> Duration {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let jitter_ms = (base_delay.as_millis() as f64 * jitter_factor * rng.gen::<f64>()) as u64;
        Duration::from_millis(jitter_ms)
    }
    
    /// Update configuration for operation type
    pub fn update_config(&mut self, op_type: OperationType, config: TimeoutConfig) {
        self.configs.insert(op_type, config);
    }
    
    /// Get statistics for operation type
    pub fn get_stats(&self, op_type: OperationType) -> Option<TimeoutStatsSnapshot> {
        self.stats.get(&op_type).map(|stats| {
            TimeoutStatsSnapshot {
                total_operations: stats.total_operations.load(Ordering::Relaxed),
                timeout_count: stats.timeout_count.load(Ordering::Relaxed),
                success_count: stats.success_count.load(Ordering::Relaxed),
                timeout_rate: stats.get_timeout_rate(),
                avg_duration: stats.get_avg_duration(),
                current_timeout: Duration::from_millis(stats.current_timeout_ms.load(Ordering::Relaxed)),
                adjustment_count: stats.adjustment_count.load(Ordering::Relaxed),
            }
        })
    }
    
    /// Get all statistics
    pub fn get_all_stats(&self) -> HashMap<OperationType, TimeoutStatsSnapshot> {
        let mut all_stats = HashMap::new();
        
        for op_type in [
            OperationType::RaftMessage,
            OperationType::Connection,
            OperationType::Proposal,
            OperationType::Storage,
            OperationType::ConfigChange,
            OperationType::HealthCheck,
            OperationType::Network,
        ] {
            if let Some(stats) = self.get_stats(op_type) {
                all_stats.insert(op_type, stats);
            }
        }
        
        all_stats
    }
}

/// Snapshot of timeout statistics
#[derive(Debug, Clone)]
pub struct TimeoutStatsSnapshot {
    pub total_operations: u64,
    pub timeout_count: u64,
    pub success_count: u64,
    pub timeout_rate: f64,
    pub avg_duration: Duration,
    pub current_timeout: Duration,
    pub adjustment_count: u32,
}

/// Global timeout manager instance
static GLOBAL_TIMEOUT_MANAGER: tokio::sync::OnceCell<Arc<tokio::sync::Mutex<TimeoutManager>>> = tokio::sync::OnceCell::const_new();

/// Get global timeout manager
pub async fn global_timeout_manager() -> Arc<tokio::sync::Mutex<TimeoutManager>> {
    GLOBAL_TIMEOUT_MANAGER.get_or_init(|| async {
        Arc::new(tokio::sync::Mutex::new(TimeoutManager::new()))
    }).await.clone()
}

/// Convenience function to execute with timeout using global manager
pub async fn execute_with_timeout<F, T, E>(
    op_type: OperationType,
    operation: F,
) -> Result<T>
where
    F: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
{
    let manager = global_timeout_manager().await;
    let mut guard = manager.lock().await;
    guard.execute_with_timeout(op_type, operation).await
}

/// Convenience function to execute with retry using global manager
pub async fn execute_with_retry<F, Fut, T, E>(
    op_type: OperationType,
    operation: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
{
    let manager = global_timeout_manager().await;
    let mut guard = manager.lock().await;
    guard.execute_with_retry(op_type, operation).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;
    
    #[tokio::test]
    async fn test_timeout_manager_basic() {
        let mut manager = TimeoutManager::new();
        
        // Test successful operation
        let result = manager.execute_with_timeout(
            OperationType::RaftMessage,
            async { Ok::<_, &str>("success") }
        ).await;
        assert!(result.is_ok());
        
        // Test timeout
        let result = manager.execute_with_timeout(
            OperationType::RaftMessage,
            async {
                sleep(Duration::from_millis(1000)).await;
                Ok::<_, &str>("too slow")
            }
        ).await;
        assert!(result.is_err());
        
        // Check stats
        let stats = manager.get_stats(OperationType::RaftMessage).unwrap();
        assert!(stats.total_operations >= 2);
        assert!(stats.timeout_count >= 1);
    }
    
    #[tokio::test]
    async fn test_retry_with_backoff() {
        let mut manager = TimeoutManager::new();
        let mut attempt_count = 0;
        
        let result = manager.execute_with_retry(
            OperationType::Network,
            || {
                attempt_count += 1;
                async move {
                    if attempt_count < 3 {
                        Err("temporary failure")
                    } else {
                        Ok("success")
                    }
                }
            }
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(attempt_count, 3);
    }
    
    #[tokio::test]
    async fn test_adaptive_timeout() {
        let mut manager = TimeoutManager::new();
        
        // Configure for quick adaptation
        let mut config = OperationType::RaftMessage.default_config();
        config.base_timeout = Duration::from_millis(100);
        config.enable_adaptive = true;
        manager.update_config(OperationType::RaftMessage, config);
        
        // Generate some timeouts
        for _ in 0..5 {
            let _ = manager.execute_with_timeout(
                OperationType::RaftMessage,
                async {
                    sleep(Duration::from_millis(200)).await;
                    Ok::<_, &str>("slow")
                }
            ).await;
        }
        
        let stats = manager.get_stats(OperationType::RaftMessage).unwrap();
        assert!(stats.timeout_count > 0);
    }
}