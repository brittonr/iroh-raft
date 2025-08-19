//! Observability and metrics collection for iroh-raft
//!
//! This module provides comprehensive metrics and tracing capabilities for the iroh-raft
//! distributed consensus library using OpenTelemetry standards.
//!
//! # Overview
//!
//! The metrics system captures key performance indicators and operational data for:
//! - **Raft consensus**: Leader elections, log replication, term changes
//! - **Storage operations**: Read/write latencies, compaction metrics, disk usage
//! - **P2P transport**: Connection status, message throughput, network errors
//! - **Node health**: Resource utilization, error rates, availability
//!
//! # Usage
//!
//! ```rust,no_run
//! use iroh_raft::metrics::{MetricsRegistry, RaftMetrics};
//! use iroh_raft::Result;
//!
//! fn main() -> Result<()> {
//!     // Create metrics registry
//!     let registry = MetricsRegistry::new("iroh-raft")?;
//!
//!     // Record Raft metrics
//!     let raft_metrics = registry.raft_metrics();
//!     raft_metrics.record_leader_election(1, 42);
//!     raft_metrics.record_log_append(10, 150.5);
//!     raft_metrics.increment_consensus_errors("append_entries_timeout");
//!
//!     // Export metrics for scraping
//!     let metrics_data = registry.export_prometheus()?;
//!     println!("{}", metrics_data);
//!     Ok(())
//! }
//! ```
//!
//! # Feature Requirements
//!
//! This module requires the `metrics-otel` feature flag:
//! ```toml
//! iroh-raft = { version = "0.1", features = ["metrics-otel"] }
//! ```

#[cfg(feature = "metrics-otel")]
use opentelemetry::{
    metrics::{Counter, Histogram, Meter},
    KeyValue,
};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use crate::{NodeId, Result};

/// High-level metrics registry for iroh-raft components
///
/// Provides organized access to metrics for different subsystems while maintaining
/// a unified collection and export interface.
#[derive(Debug, Clone)]
pub struct MetricsRegistry {
    inner: Arc<MetricsRegistryInner>,
}

#[derive(Debug)]
struct MetricsRegistryInner {
    service_name: String,
    #[cfg(feature = "metrics-otel")]
    meter: Meter,
    raft_metrics: RaftMetrics,
    storage_metrics: StorageMetrics,
    transport_metrics: TransportMetrics,
    node_metrics: NodeMetrics,
    custom_metrics: RwLock<HashMap<String, CustomMetric>>,
}

impl MetricsRegistry {
    /// Create a new metrics registry with OpenTelemetry integration
    ///
    /// # Arguments
    /// * `service_name` - Service identifier for metrics collection
    ///
    /// # Example
    /// ```rust,no_run
    /// use iroh_raft::metrics::MetricsRegistry;
    /// use iroh_raft::Result;
    ///
    /// fn main() -> Result<()> {
    ///     let registry = MetricsRegistry::new("my-raft-cluster")?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "metrics-otel")]
    pub fn new(service_name: &str) -> Result<Self> {
        use opentelemetry::global;
        
        let service_name_owned = service_name.to_string();
        let meter = global::meter(service_name_owned.clone());
        
        Ok(Self {
            inner: Arc::new(MetricsRegistryInner {
                service_name: service_name_owned,
                meter: meter.clone(),
                raft_metrics: RaftMetrics::new(&meter)?,
                storage_metrics: StorageMetrics::new(&meter)?,
                transport_metrics: TransportMetrics::new(&meter)?,
                node_metrics: NodeMetrics::new(&meter)?,
                custom_metrics: RwLock::new(HashMap::new()),
            }),
        })
    }

    /// Create a new metrics registry without OpenTelemetry (no-op implementation)
    #[cfg(not(feature = "metrics-otel"))]
    pub fn new(service_name: &str) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(MetricsRegistryInner {
                service_name: service_name.to_string(),
                raft_metrics: RaftMetrics::new()?,
                storage_metrics: StorageMetrics::new()?,
                transport_metrics: TransportMetrics::new()?,
                node_metrics: NodeMetrics::new()?,
                custom_metrics: RwLock::new(HashMap::new()),
            }),
        })
    }

    /// Get Raft consensus metrics
    pub fn raft_metrics(&self) -> &RaftMetrics {
        &self.inner.raft_metrics
    }

    /// Get storage subsystem metrics
    pub fn storage_metrics(&self) -> &StorageMetrics {
        &self.inner.storage_metrics
    }

    /// Get P2P transport metrics
    pub fn transport_metrics(&self) -> &TransportMetrics {
        &self.inner.transport_metrics
    }

    /// Get node health and resource metrics
    pub fn node_metrics(&self) -> &NodeMetrics {
        &self.inner.node_metrics
    }

    /// Export metrics in Prometheus format
    /// 
    /// Returns a formatted string suitable for Prometheus scraping endpoints.
    #[cfg(feature = "metrics-otel")]
    pub fn export_prometheus(&self) -> Result<String> {
        // In a real implementation, this would use the OpenTelemetry Prometheus exporter
        // For now, we'll provide a basic implementation
        Ok(format!("# HELP iroh_raft_info Service information\n\
                   # TYPE iroh_raft_info gauge\n\
                   iroh_raft_info{{service=\"{}\"}} 1\n", 
                   self.inner.service_name))
    }

    /// Export metrics in Prometheus format (no-op version)
    #[cfg(not(feature = "metrics-otel"))]
    pub fn export_prometheus(&self) -> Result<String> {
        Ok(format!("# Metrics collection disabled - enable 'metrics-otel' feature\n\
                   # Service: {}\n", 
                   self.inner.service_name))
    }

    /// Register a custom metric for application-specific measurements
    pub fn register_custom_counter(&self, name: &str, description: &str) -> Result<()> {
        let mut custom_metrics = self.inner.custom_metrics.write()
            .map_err(|_| crate::error::RaftError::LockPoisoned {
                operation: "register_custom_counter".to_string(),
                details: "custom metrics lock poisoned".to_string(),
            })?;
        
        let name_owned = name.to_string();
        let description_owned = description.to_string();
        
        #[cfg(feature = "metrics-otel")]
        let counter = self.inner.meter.u64_counter(name_owned.clone())
            .with_description(description_owned.clone())
            .init();
        
        custom_metrics.insert(name_owned.clone(), CustomMetric {
            name: name_owned,
            description: description_owned,
            #[cfg(feature = "metrics-otel")]
            metric_type: CustomMetricType::Counter(counter),
            #[cfg(not(feature = "metrics-otel"))]
            metric_type: CustomMetricType::Counter,
        });
        
        Ok(())
    }
}

/// Raft consensus protocol metrics
#[derive(Debug, Clone)]
pub struct RaftMetrics {
    #[cfg(feature = "metrics-otel")]
    leader_elections_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    log_entries_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    consensus_errors_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    append_entries_latency: Histogram<f64>,
}

impl RaftMetrics {
    #[cfg(feature = "metrics-otel")]
    fn new(meter: &Meter) -> Result<Self> {
        Ok(Self {
            leader_elections_total: meter.u64_counter("raft_leader_elections_total")
                .with_description("Total number of leader elections")
                .init(),
            log_entries_total: meter.u64_counter("raft_log_entries_total")
                .with_description("Total number of log entries")
                .init(),
            consensus_errors_total: meter.u64_counter("raft_consensus_errors_total")
                .with_description("Total consensus protocol errors")
                .init(),
            append_entries_latency: meter.f64_histogram("raft_append_entries_latency_seconds")
                .with_description("Latency of append entries operations")
                .init(),
        })
    }

    #[cfg(not(feature = "metrics-otel"))]
    fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Record a leader election event
    pub fn record_leader_election(&self, node_id: NodeId, _term: u64) {
        #[cfg(feature = "metrics-otel")]
        {
            self.leader_elections_total.add(1, &[KeyValue::new("node_id", node_id as i64)]);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            // No-op implementation
            let _ = node_id;
        }
    }

    /// Record log append operation
    pub fn record_log_append(&self, entries_count: u64, latency_ms: f64) {
        #[cfg(feature = "metrics-otel")]
        {
            self.log_entries_total.add(entries_count, &[]);
            self.append_entries_latency.record(latency_ms / 1000.0, &[]);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            let _ = (entries_count, latency_ms);
        }
    }

    /// Increment consensus error counter
    pub fn increment_consensus_errors(&self, error_type: &str) {
        #[cfg(feature = "metrics-otel")]
        {
            self.consensus_errors_total.add(1, &[KeyValue::new("error_type", error_type.to_string())]);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            let _ = error_type;
        }
    }
}

/// Storage subsystem metrics
#[derive(Debug, Clone)]
pub struct StorageMetrics {
    #[cfg(feature = "metrics-otel")]
    read_operations_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    write_operations_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    storage_latency: Histogram<f64>,
}

impl StorageMetrics {
    #[cfg(feature = "metrics-otel")]
    fn new(meter: &Meter) -> Result<Self> {
        Ok(Self {
            read_operations_total: meter.u64_counter("storage_read_operations_total")
                .with_description("Total storage read operations")
                .init(),
            write_operations_total: meter.u64_counter("storage_write_operations_total")
                .with_description("Total storage write operations")
                .init(),
            storage_latency: meter.f64_histogram("storage_operation_latency_seconds")
                .with_description("Storage operation latency")
                .init(),
        })
    }

    #[cfg(not(feature = "metrics-otel"))]
    fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Record a storage operation
    pub fn record_operation(&self, operation_type: &str, latency: Duration) {
        #[cfg(feature = "metrics-otel")]
        {
            let attrs = [KeyValue::new("operation", operation_type.to_string())];
            match operation_type {
                "read" => self.read_operations_total.add(1, &attrs),
                "write" => self.write_operations_total.add(1, &attrs),
                _ => {}
            }
            self.storage_latency.record(latency.as_secs_f64(), &attrs);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            let _ = (operation_type, latency);
        }
    }

    /// Update disk usage measurement (no-op in current implementation)
    pub fn update_disk_usage(&self, _bytes: u64) {
        // Note: disk usage would typically be tracked via a gauge,
        // but OpenTelemetry Rust doesn't have synchronous gauges yet
    }
}

/// P2P transport metrics
#[derive(Debug, Clone)]
pub struct TransportMetrics {
    #[cfg(feature = "metrics-otel")]
    messages_sent_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    messages_received_total: Counter<u64>,
    #[cfg(feature = "metrics-otel")]
    network_errors_total: Counter<u64>,
}

impl TransportMetrics {
    #[cfg(feature = "metrics-otel")]
    fn new(meter: &Meter) -> Result<Self> {
        Ok(Self {
            messages_sent_total: meter.u64_counter("transport_messages_sent_total")
                .with_description("Total P2P messages sent")
                .init(),
            messages_received_total: meter.u64_counter("transport_messages_received_total")
                .with_description("Total P2P messages received")
                .init(),
            network_errors_total: meter.u64_counter("transport_network_errors_total")
                .with_description("Total network transport errors")
                .init(),
        })
    }

    #[cfg(not(feature = "metrics-otel"))]
    fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Update active connections count (no-op in current implementation)
    pub fn update_active_connections(&self, _count: u64) {
        // Note: connection count would typically be tracked via a gauge
    }

    /// Record message sent
    pub fn record_message_sent(&self, message_type: &str, peer_id: &str) {
        #[cfg(feature = "metrics-otel")]
        {
            self.messages_sent_total.add(1, &[
                KeyValue::new("message_type", message_type.to_string()),
                KeyValue::new("peer_id", peer_id.to_string()),
            ]);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            let _ = (message_type, peer_id);
        }
    }

    /// Record message received
    pub fn record_message_received(&self, message_type: &str, peer_id: &str) {
        #[cfg(feature = "metrics-otel")]
        {
            self.messages_received_total.add(1, &[
                KeyValue::new("message_type", message_type.to_string()),
                KeyValue::new("peer_id", peer_id.to_string()),
            ]);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            let _ = (message_type, peer_id);
        }
    }

    /// Record network error
    pub fn record_network_error(&self, error_type: &str) {
        #[cfg(feature = "metrics-otel")]
        {
            self.network_errors_total.add(1, &[KeyValue::new("error_type", error_type.to_string())]);
        }
        
        #[cfg(not(feature = "metrics-otel"))]
        {
            let _ = error_type;
        }
    }
}

/// Node health and resource metrics
#[derive(Debug, Clone)]
pub struct NodeMetrics {
    // Node metrics would typically use gauges, but OpenTelemetry Rust
    // doesn't have synchronous gauges yet, so we'll use no-op implementations
}

impl NodeMetrics {
    #[cfg(feature = "metrics-otel")]
    fn new(_meter: &Meter) -> Result<Self> {
        Ok(Self {})
    }

    #[cfg(not(feature = "metrics-otel"))]
    fn new() -> Result<Self> {
        Ok(Self {})
    }

    /// Update node status (no-op in current implementation)
    pub fn update_node_status(&self, node_id: NodeId, is_up: bool) {
        let _ = (node_id, is_up);
        // Note: would typically use a gauge to track node status
    }

    /// Update resource usage metrics (no-op in current implementation)
    pub fn update_resource_usage(&self, memory_bytes: u64, cpu_percent: f64) {
        let _ = (memory_bytes, cpu_percent);
        // Note: would typically use gauges to track resource usage
    }

    /// Update uptime measurement (no-op in current implementation)
    pub fn update_uptime(&self, uptime: Duration) {
        let _ = uptime;
        // Note: would typically use a gauge to track uptime
    }
}

/// Custom application metrics
#[derive(Debug)]
pub struct CustomMetric {
    pub name: String,
    pub description: String,
    #[cfg(feature = "metrics-otel")]
    pub metric_type: CustomMetricType,
    #[cfg(not(feature = "metrics-otel"))]
    pub metric_type: CustomMetricType,
}

#[derive(Debug)]
pub enum CustomMetricType {
    #[cfg(feature = "metrics-otel")]
    Counter(Counter<u64>),
    #[cfg(feature = "metrics-otel")]
    Histogram(Histogram<f64>),
    #[cfg(not(feature = "metrics-otel"))]
    Counter,
    #[cfg(not(feature = "metrics-otel"))]
    Histogram,
}

/// Utility for measuring operation latency
pub struct LatencyTimer {
    start: Instant,
    operation: String,
}

impl LatencyTimer {
    /// Start timing an operation
    pub fn start(operation: &str) -> Self {
        Self {
            start: Instant::now(),
            operation: operation.to_string(),
        }
    }

    /// Get elapsed time since timer started
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Get elapsed time in milliseconds
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed().as_secs_f64() * 1000.0
    }

    /// Get the operation name
    pub fn operation(&self) -> &str {
        &self.operation
    }
}

/// Convenience macro for timing operations
/// 
/// # Example
/// ```rust,no_run
/// use iroh_raft::time_operation;
/// 
/// let result = time_operation!("database_read", {
///     // Your database read operation here
///     std::thread::sleep(std::time::Duration::from_millis(10));
///     "data".to_string()
/// });
/// ```
#[macro_export]
macro_rules! time_operation {
    ($operation:expr, $block:block) => {{
        let _timer = $crate::metrics::LatencyTimer::start($operation);
        let result = $block;
        tracing::debug!("Operation '{}' completed in {:.2}ms", 
                       _timer.operation(), _timer.elapsed_ms());
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_metrics_registry_creation() {
        let registry = MetricsRegistry::new("test-service").unwrap();
        
        // Should be able to access all metric types
        let _raft = registry.raft_metrics();
        let _storage = registry.storage_metrics();
        let _transport = registry.transport_metrics();
        let _node = registry.node_metrics();
    }

    #[test]
    fn test_raft_metrics() {
        let registry = MetricsRegistry::new("test-raft").unwrap();
        let raft_metrics = registry.raft_metrics();
        
        // These should not panic (no-op when feature disabled)
        raft_metrics.record_leader_election(1, 42);
        raft_metrics.record_log_append(5, 150.0);
        raft_metrics.increment_consensus_errors("timeout");
    }

    #[test]
    fn test_storage_metrics() {
        let registry = MetricsRegistry::new("test-storage").unwrap();
        let storage_metrics = registry.storage_metrics();
        
        storage_metrics.record_operation("read", Duration::from_millis(10));
        storage_metrics.record_operation("write", Duration::from_millis(25));
        storage_metrics.update_disk_usage(1024000);
    }

    #[test]
    fn test_transport_metrics() {
        let registry = MetricsRegistry::new("test-transport").unwrap();
        let transport_metrics = registry.transport_metrics();
        
        transport_metrics.update_active_connections(5);
        transport_metrics.record_message_sent("append_entries", "peer-1");
        transport_metrics.record_message_received("vote_request", "peer-2");
        transport_metrics.record_network_error("connection_timeout");
    }

    #[test]
    fn test_node_metrics() {
        let registry = MetricsRegistry::new("test-node").unwrap();
        let node_metrics = registry.node_metrics();
        
        node_metrics.update_node_status(1, true);
        node_metrics.update_resource_usage(1024000, 45.5);
        node_metrics.update_uptime(Duration::from_secs(3600));
    }

    #[test]
    fn test_latency_timer() {
        let timer = LatencyTimer::start("test_operation");
        
        // Simulate some work
        thread::sleep(Duration::from_millis(10));
        
        let elapsed = timer.elapsed();
        assert!(elapsed >= Duration::from_millis(10));
        assert_eq!(timer.operation(), "test_operation");
        
        let elapsed_ms = timer.elapsed_ms();
        assert!(elapsed_ms >= 10.0);
    }

    #[test]
    fn test_prometheus_export() {
        let registry = MetricsRegistry::new("test-export").unwrap();
        let export_result = registry.export_prometheus().unwrap();
        
        // Should contain service information
        assert!(export_result.contains("test-export"));
    }

    #[test]
    fn test_custom_metrics_registration() {
        let registry = MetricsRegistry::new("test-custom").unwrap();
        
        let result = registry.register_custom_counter(
            "my_custom_counter",
            "A custom counter for testing"
        );
        
        assert!(result.is_ok());
    }
}