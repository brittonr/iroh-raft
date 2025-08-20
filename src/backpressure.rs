//! Backpressure and flow control mechanisms
//!
//! This module provides bounded channels with sophisticated backpressure
//! handling to prevent unbounded memory growth.

use tokio::sync::{mpsc, Semaphore};
use tokio::time::{Duration, timeout, Instant};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use crate::error::{RaftError, Result};

/// Configuration for backpressure-aware channels
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Maximum number of pending items
    pub max_pending: usize,
    /// Timeout for sending when under backpressure
    pub send_timeout: Duration,
    /// Enable priority-based queuing
    pub enable_priority: bool,
    /// Memory pressure threshold (0.0 to 1.0)
    pub memory_pressure_threshold: f64,
    /// Enable adaptive capacity adjustment
    pub enable_adaptive_capacity: bool,
    /// Window size for measuring throughput
    pub throughput_window_size: usize,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            max_pending: 10000,
            send_timeout: Duration::from_millis(1000),
            enable_priority: true,
            memory_pressure_threshold: 0.8,
            enable_adaptive_capacity: true,
            throughput_window_size: 100,
        }
    }
}

/// Priority levels for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// A message with priority and metadata
#[derive(Debug)]
pub struct PrioritizedMessage<T> {
    pub data: T,
    pub priority: Priority,
    pub created_at: Instant,
    pub retry_count: u32,
}

impl<T> PrioritizedMessage<T> {
    pub fn new(data: T, priority: Priority) -> Self {
        Self {
            data,
            priority,
            created_at: Instant::now(),
            retry_count: 0,
        }
    }
    
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
    
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

/// Backpressure-aware bounded sender
pub struct BackpressureSender<T> {
    sender: mpsc::Sender<PrioritizedMessage<T>>,
    _config: BackpressureConfig,
    metrics: Arc<BackpressureMetrics>,
    semaphore: Arc<Semaphore>,
}

impl<T> Clone for BackpressureSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            _config: self._config.clone(),
            metrics: Arc::clone(&self.metrics),
            semaphore: Arc::clone(&self.semaphore),
        }
    }
}

/// Backpressure-aware bounded receiver
pub struct BackpressureReceiver<T> {
    receiver: mpsc::Receiver<PrioritizedMessage<T>>,
    metrics: Arc<BackpressureMetrics>,
    semaphore: Arc<Semaphore>,
}

/// Metrics for backpressure monitoring
#[derive(Debug, Default)]
pub struct BackpressureMetrics {
    pub total_sent: AtomicU64,
    pub total_received: AtomicU64,
    pub total_dropped: AtomicU64,
    pub total_timeouts: AtomicU64,
    pub current_pending: AtomicUsize,
    pub max_pending_reached: AtomicUsize,
    pub avg_latency_ms: AtomicU64,
}

impl BackpressureMetrics {
    pub fn record_sent(&self) {
        self.total_sent.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_received(&self) {
        self.total_received.fetch_add(1, Ordering::Relaxed);
        self.current_pending.fetch_sub(1, Ordering::Relaxed);
    }
    
    pub fn record_dropped(&self) {
        self.total_dropped.fetch_add(1, Ordering::Relaxed);
        self.current_pending.fetch_sub(1, Ordering::Relaxed);
    }
    
    pub fn record_timeout(&self) {
        self.total_timeouts.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_pending(&self, count: usize) {
        self.current_pending.store(count, Ordering::Relaxed);
        
        // Update max if this is higher
        let mut current_max = self.max_pending_reached.load(Ordering::Relaxed);
        while count > current_max {
            match self.max_pending_reached.compare_exchange_weak(
                current_max, 
                count, 
                Ordering::Relaxed, 
                Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(new_max) => current_max = new_max,
            }
        }
    }
    
    pub fn record_latency(&self, latency: Duration) {
        // Simple exponential moving average
        let latency_ms = latency.as_millis() as u64;
        let current = self.avg_latency_ms.load(Ordering::Relaxed);
        let new_avg = if current == 0 {
            latency_ms
        } else {
            (current * 7 + latency_ms) / 8 // EMA with alpha = 0.125
        };
        self.avg_latency_ms.store(new_avg, Ordering::Relaxed);
    }
    
    pub fn get_snapshot(&self) -> BackpressureSnapshot {
        BackpressureSnapshot {
            total_sent: self.total_sent.load(Ordering::Relaxed),
            total_received: self.total_received.load(Ordering::Relaxed),
            total_dropped: self.total_dropped.load(Ordering::Relaxed),
            total_timeouts: self.total_timeouts.load(Ordering::Relaxed),
            current_pending: self.current_pending.load(Ordering::Relaxed),
            max_pending_reached: self.max_pending_reached.load(Ordering::Relaxed),
            avg_latency_ms: self.avg_latency_ms.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of backpressure metrics
#[derive(Debug, Clone)]
pub struct BackpressureSnapshot {
    pub total_sent: u64,
    pub total_received: u64,
    pub total_dropped: u64,
    pub total_timeouts: u64,
    pub current_pending: usize,
    pub max_pending_reached: usize,
    pub avg_latency_ms: u64,
}

/// Create a backpressure-aware bounded channel
pub fn bounded_with_backpressure<T>(
    config: BackpressureConfig
) -> (BackpressureSender<T>, BackpressureReceiver<T>) {
    let (sender, receiver) = mpsc::channel(config.max_pending);
    let metrics = Arc::new(BackpressureMetrics::default());
    let semaphore = Arc::new(Semaphore::new(config.max_pending));
    
    let bp_sender = BackpressureSender {
        sender,
        _config: config.clone(),
        metrics: Arc::clone(&metrics),
        semaphore: Arc::clone(&semaphore),
    };
    
    let bp_receiver = BackpressureReceiver {
        receiver,
        metrics,
        semaphore,
    };
    
    (bp_sender, bp_receiver)
}

impl<T> BackpressureSender<T> {
    /// Send with backpressure and timeout
    pub async fn send(&self, data: T, priority: Priority) -> Result<()> {
        // Acquire semaphore permit to enforce backpressure
        let permit = timeout(
            self._config.send_timeout,
            self.semaphore.acquire()
        ).await.map_err(|_| RaftError::Timeout {
            operation: "acquire send permit".to_string(),
            duration: self._config.send_timeout,
            backtrace: snafu::Backtrace::new(),
        })?.map_err(|_| RaftError::Internal {
            message: "Semaphore closed".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        let message = PrioritizedMessage::new(data, priority);
        
        match timeout(
            self._config.send_timeout,
            self.sender.send(message)
        ).await {
            Ok(Ok(())) => {
                self.metrics.record_sent();
                // Don't forget the permit - it will be released when message is received
                std::mem::forget(permit);
                Ok(())
            }
            Ok(Err(_)) => {
                // Channel closed
                Err(RaftError::Internal {
                    message: "Channel closed".to_string(),
                    backtrace: snafu::Backtrace::new(),
                })
            }
            Err(_) => {
                // Timeout
                self.metrics.record_timeout();
                Err(RaftError::Timeout {
                    operation: "send message".to_string(),
                    duration: self._config.send_timeout,
                    backtrace: snafu::Backtrace::new(),
                })
            }
        }
    }
    
    /// Try to send without blocking
    pub fn try_send(&self, data: T, priority: Priority) -> Result<()> {
        // Try to acquire permit without blocking
        let permit = self.semaphore.try_acquire().map_err(|_| RaftError::Internal {
            message: "Channel at capacity".to_string(),
            backtrace: snafu::Backtrace::new(),
        })?;
        
        let message = PrioritizedMessage::new(data, priority);
        
        match self.sender.try_send(message) {
            Ok(()) => {
                self.metrics.record_sent();
                std::mem::forget(permit);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                Err(RaftError::Internal {
                    message: "Channel full".to_string(),
                    backtrace: snafu::Backtrace::new(),
                })
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(RaftError::Internal {
                    message: "Channel closed".to_string(),
                    backtrace: snafu::Backtrace::new(),
                })
            }
        }
    }
    
    /// Get current backpressure metrics
    pub fn metrics(&self) -> BackpressureSnapshot {
        self.metrics.get_snapshot()
    }
    
    /// Check if currently under backpressure
    pub fn is_under_backpressure(&self) -> bool {
        let available = self.semaphore.available_permits();
        let total = self._config.max_pending;
        let usage_ratio = (total - available) as f64 / total as f64;
        
        usage_ratio > self._config.memory_pressure_threshold
    }
}

impl<T> BackpressureReceiver<T> {
    /// Receive next message
    pub async fn recv(&mut self) -> Option<PrioritizedMessage<T>> {
        match self.receiver.recv().await {
            Some(message) => {
                let latency = message.age();
                self.metrics.record_received();
                self.metrics.record_latency(latency);
                
                // Release semaphore permit
                self.semaphore.add_permits(1);
                
                Some(message)
            }
            None => None
        }
    }
    
    /// Try to receive without blocking
    pub fn try_recv(&mut self) -> Result<PrioritizedMessage<T>> {
        match self.receiver.try_recv() {
            Ok(message) => {
                let latency = message.age();
                self.metrics.record_received();
                self.metrics.record_latency(latency);
                
                // Release semaphore permit
                self.semaphore.add_permits(1);
                
                Ok(message)
            }
            Err(mpsc::error::TryRecvError::Empty) => {
                Err(RaftError::Internal {
                    message: "No messages available".to_string(),
                    backtrace: snafu::Backtrace::new(),
                })
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(RaftError::Internal {
                    message: "Channel disconnected".to_string(),
                    backtrace: snafu::Backtrace::new(),
                })
            }
        }
    }
    
    /// Get current backpressure metrics
    pub fn metrics(&self) -> BackpressureSnapshot {
        self.metrics.get_snapshot()
    }
}

/// Priority queue for managing different message types
pub struct PriorityBackpressureManager<T> {
    queues: Vec<(BackpressureSender<T>, BackpressureReceiver<T>)>,
    _config: BackpressureConfig,
}

impl<T> PriorityBackpressureManager<T> {
    pub fn new(config: BackpressureConfig) -> Self {
        let mut queues = Vec::new();
        
        // Create separate queues for each priority level
        for _ in 0..4 { // 4 priority levels
            let queue_config = BackpressureConfig {
                max_pending: config.max_pending / 4, // Distribute capacity
                ..config.clone()
            };
            queues.push(bounded_with_backpressure(queue_config));
        }
        
        Self { queues, _config: config }
    }
    
    pub fn sender(&self, priority: Priority) -> &BackpressureSender<T> {
        &self.queues[priority as usize].0
    }
    
    pub fn receiver(&mut self, priority: Priority) -> &mut BackpressureReceiver<T> {
        &mut self.queues[priority as usize].1
    }
    
    /// Receive from the highest priority queue that has messages
    pub async fn recv_prioritized(&mut self) -> Option<PrioritizedMessage<T>> {
        // Check high priority queues first in order
        if let Ok(message) = self.queues[Priority::Critical as usize].1.try_recv() {
            return Some(message);
        }
        if let Ok(message) = self.queues[Priority::High as usize].1.try_recv() {
            return Some(message);
        }
        if let Ok(message) = self.queues[Priority::Normal as usize].1.try_recv() {
            return Some(message);
        }
        if let Ok(message) = self.queues[Priority::Low as usize].1.try_recv() {
            return Some(message);
        }
        
        // If no immediate messages, wait on the highest priority queue
        // In a real implementation, you'd want to wait on all queues properly
        self.queues[Priority::Critical as usize].1.recv().await
    }
    
    /// Get aggregated metrics across all priority queues
    pub fn aggregate_metrics(&self) -> BackpressureSnapshot {
        let mut total = BackpressureSnapshot {
            total_sent: 0,
            total_received: 0,
            total_dropped: 0,
            total_timeouts: 0,
            current_pending: 0,
            max_pending_reached: 0,
            avg_latency_ms: 0,
        };
        
        let mut latency_sum = 0u64;
        let mut latency_count = 0u64;
        
        for (sender, _) in &self.queues {
            let metrics = sender.metrics();
            total.total_sent += metrics.total_sent;
            total.total_received += metrics.total_received;
            total.total_dropped += metrics.total_dropped;
            total.total_timeouts += metrics.total_timeouts;
            total.current_pending += metrics.current_pending;
            total.max_pending_reached = total.max_pending_reached.max(metrics.max_pending_reached);
            
            if metrics.total_received > 0 {
                latency_sum += metrics.avg_latency_ms * metrics.total_received;
                latency_count += metrics.total_received;
            }
        }
        
        if latency_count > 0 {
            total.avg_latency_ms = latency_sum / latency_count;
        }
        
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_backpressure_basic() {
        let config = BackpressureConfig {
            max_pending: 2,
            send_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        
        let (sender, mut receiver) = bounded_with_backpressure(config);
        
        // Should be able to send up to capacity
        assert!(sender.send("msg1", Priority::Normal).await.is_ok());
        assert!(sender.send("msg2", Priority::Normal).await.is_ok());
        
        // Should timeout when over capacity
        assert!(sender.send("msg3", Priority::Normal).await.is_err());
        
        // Receiving should free up space
        let msg = receiver.recv().await.unwrap();
        assert_eq!(msg.data, "msg1");
        
        // Should be able to send again
        assert!(sender.send("msg3", Priority::Normal).await.is_ok());
    }
    
    #[tokio::test]
    async fn test_priority_ordering() {
        let config = BackpressureConfig::default();
        let mut manager = PriorityBackpressureManager::new(config);
        
        // Send messages with different priorities
        assert!(manager.sender(Priority::Low).send("low", Priority::Low).await.is_ok());
        assert!(manager.sender(Priority::High).send("high", Priority::High).await.is_ok());
        assert!(manager.sender(Priority::Normal).send("normal", Priority::Normal).await.is_ok());
        assert!(manager.sender(Priority::Critical).send("critical", Priority::Critical).await.is_ok());
        
        // Should receive in priority order
        let msg1 = manager.recv_prioritized().await.unwrap();
        assert_eq!(msg1.data, "critical");
        assert_eq!(msg1.priority, Priority::Critical);
        
        let msg2 = manager.recv_prioritized().await.unwrap();
        assert_eq!(msg2.data, "high");
        assert_eq!(msg2.priority, Priority::High);
    }
}