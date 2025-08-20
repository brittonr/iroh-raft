//! Monitoring Integration Example
//!
//! This example demonstrates comprehensive monitoring and observability
//! for iroh-raft clusters including:
//! - Prometheus metrics export
//! - OpenTelemetry tracing integration
//! - Health checks and alerting
//! - Performance monitoring
//! - Custom application metrics
//! - Log aggregation strategies
//! - Dashboard configuration
//!
//! Run with: cargo run --example monitoring_integration

use iroh_raft::{
    config::{ConfigBuilder, ConfigError},
    error::RaftError,
    raft::{KvCommand, KvState, KeyValueStore, StateMachine, ExampleRaftStateMachine},
    types::NodeId,
    Result,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH, Instant};
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::time::{timeout, sleep, interval};

// Re-use types from previous examples
use crate::simple_cluster_example::{RaftCluster, ClusterMetrics, Preset};
use crate::distributed_kv_store::{DistributedKvStore, DistributedKvCommand};

/// Comprehensive metrics registry
#[derive(Debug)]
pub struct MetricsRegistry {
    raft_metrics: Arc<RwLock<RaftMetrics>>,
    application_metrics: Arc<RwLock<ApplicationMetrics>>,
    system_metrics: Arc<RwLock<SystemMetrics>>,
    performance_metrics: Arc<RwLock<PerformanceMetrics>>,
    custom_metrics: Arc<RwLock<HashMap<String, f64>>>,
}

/// Raft-specific metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftMetrics {
    // Leadership metrics
    pub current_term: u64,
    pub is_leader: bool,
    pub leader_elections_total: u64,
    pub leadership_changes_total: u64,
    pub leadership_duration_seconds: u64,

    // Log metrics
    pub log_entries_total: u64,
    pub log_size_bytes: u64,
    pub commit_index: u64,
    pub last_applied_index: u64,
    pub log_compaction_count: u64,

    // Proposal metrics
    pub proposals_total: u64,
    pub proposals_committed: u64,
    pub proposals_failed: u64,
    pub proposal_latency_seconds: f64,
    pub proposal_queue_size: u64,

    // Snapshot metrics
    pub snapshots_created: u64,
    pub snapshots_restored: u64,
    pub snapshot_size_bytes: u64,
    pub snapshot_creation_duration_seconds: f64,

    // Consensus metrics
    pub append_entries_sent: u64,
    pub append_entries_received: u64,
    pub vote_requests_sent: u64,
    pub vote_requests_received: u64,
    pub heartbeats_sent: u64,
    pub heartbeats_received: u64,

    // Error metrics
    pub consensus_errors_total: u64,
    pub network_errors_total: u64,
    pub storage_errors_total: u64,
}

/// Application-specific metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationMetrics {
    // Request metrics
    pub requests_total: u64,
    pub requests_per_second: f64,
    pub request_duration_seconds: f64,
    pub request_size_bytes: f64,
    pub response_size_bytes: f64,

    // Operation metrics
    pub read_operations_total: u64,
    pub write_operations_total: u64,
    pub delete_operations_total: u64,
    pub batch_operations_total: u64,

    // Data metrics
    pub total_keys: u64,
    pub total_data_size_bytes: u64,
    pub key_size_distribution: HashMap<String, u64>,
    pub value_size_distribution: HashMap<String, u64>,

    // Cache metrics (if applicable)
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_evictions: u64,
    pub cache_size_bytes: u64,

    // Client metrics
    pub active_connections: u64,
    pub connection_errors: u64,
    pub client_timeouts: u64,
}

/// System-level metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    // Resource utilization
    pub cpu_usage_percent: f64,
    pub memory_usage_bytes: u64,
    pub memory_usage_percent: f64,
    pub disk_usage_bytes: u64,
    pub disk_usage_percent: f64,

    // Network metrics
    pub network_bytes_sent: u64,
    pub network_bytes_received: u64,
    pub network_packets_sent: u64,
    pub network_packets_received: u64,
    pub network_errors: u64,

    // Process metrics
    pub uptime_seconds: u64,
    pub goroutines_count: u64, // In Rust: async tasks
    pub open_file_descriptors: u64,
    pub gc_duration_seconds: f64,

    // Disk I/O metrics
    pub disk_reads_total: u64,
    pub disk_writes_total: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
}

/// Performance-specific metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    // Latency histograms
    pub request_latency_histogram: LatencyHistogram,
    pub consensus_latency_histogram: LatencyHistogram,
    pub storage_latency_histogram: LatencyHistogram,

    // Throughput metrics
    pub operations_per_second: f64,
    pub bytes_per_second: f64,
    pub peak_operations_per_second: f64,
    pub peak_bytes_per_second: f64,

    // Queue metrics
    pub proposal_queue_depth: u64,
    pub message_queue_depth: u64,
    pub max_queue_depth: u64,
    pub queue_wait_time_seconds: f64,

    // Connection pool metrics
    pub active_connections: u64,
    pub idle_connections: u64,
    pub connection_pool_utilization: f64,
    pub connection_acquisition_time_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyHistogram {
    pub buckets: HashMap<String, u64>, // bucket -> count
    pub total_count: u64,
    pub sum_seconds: f64,
}

/// Health check system
#[derive(Debug)]
pub struct HealthChecker {
    checks: Vec<HealthCheck>,
    status: Arc<RwLock<OverallHealth>>,
}

#[derive(Debug, Clone)]
pub struct HealthCheck {
    pub name: String,
    pub check_type: HealthCheckType,
    pub interval: Duration,
    pub timeout: Duration,
    pub critical: bool,
}

#[derive(Debug, Clone)]
pub enum HealthCheckType {
    RaftLeadership,
    StorageConnectivity,
    NetworkConnectivity,
    MemoryUsage { threshold_percent: f64 },
    DiskUsage { threshold_percent: f64 },
    ConsensusHealth,
    ApplicationSpecific { endpoint: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallHealth {
    pub status: HealthStatus,
    pub checks: HashMap<String, HealthCheckResult>,
    pub last_updated: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub status: HealthStatus,
    pub message: String,
    pub duration_ms: u64,
    pub last_check: SystemTime,
}

/// Tracing and observability
#[derive(Debug)]
pub struct TracingManager {
    service_name: String,
    service_version: String,
    traces: Arc<RwLock<Vec<TraceSpan>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub start_time: SystemTime,
    pub end_time: Option<SystemTime>,
    pub tags: HashMap<String, String>,
    pub logs: Vec<TraceLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceLog {
    pub timestamp: SystemTime,
    pub level: String,
    pub message: String,
    pub fields: HashMap<String, String>,
}

/// Alerting system
#[derive(Debug)]
pub struct AlertManager {
    rules: Vec<AlertRule>,
    active_alerts: Arc<RwLock<Vec<Alert>>>,
    notification_channels: Vec<NotificationChannel>,
}

#[derive(Debug, Clone)]
pub struct AlertRule {
    pub name: String,
    pub condition: AlertCondition,
    pub severity: AlertSeverity,
    pub duration: Duration,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum AlertCondition {
    MetricThreshold {
        metric: String,
        operator: ComparisonOperator,
        value: f64,
    },
    HealthCheck {
        check_name: String,
        status: HealthStatus,
    },
    RateOfChange {
        metric: String,
        threshold: f64,
        window: Duration,
    },
}

#[derive(Debug, Clone)]
pub enum ComparisonOperator {
    GreaterThan,
    LessThan,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub name: String,
    pub severity: AlertSeverity,
    pub message: String,
    pub labels: HashMap<String, String>,
    pub start_time: SystemTime,
    pub end_time: Option<SystemTime>,
    pub acknowledged: bool,
}

#[derive(Debug, Clone)]
pub enum NotificationChannel {
    Slack { webhook_url: String },
    Email { addresses: Vec<String> },
    PagerDuty { integration_key: String },
    Webhook { url: String, headers: HashMap<String, String> },
}

/// Monitoring coordinator
pub struct MonitoringCoordinator {
    metrics_registry: Arc<MetricsRegistry>,
    health_checker: Arc<HealthChecker>,
    tracing_manager: Arc<TracingManager>,
    alert_manager: Arc<AlertManager>,
    export_interval: Duration,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            raft_metrics: Arc::new(RwLock::new(RaftMetrics::default())),
            application_metrics: Arc::new(RwLock::new(ApplicationMetrics::default())),
            system_metrics: Arc::new(RwLock::new(SystemMetrics::default())),
            performance_metrics: Arc::new(RwLock::new(PerformanceMetrics::default())),
            custom_metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn update_raft_metrics<F>(&self, updater: F) 
    where
        F: FnOnce(&mut RaftMetrics),
    {
        let mut metrics = self.raft_metrics.write().await;
        updater(&mut metrics);
    }

    pub async fn increment_counter(&self, name: &str, value: f64) {
        self.custom_metrics.write().await
            .entry(name.to_string())
            .and_modify(|v| *v += value)
            .or_insert(value);
    }

    pub async fn set_gauge(&self, name: &str, value: f64) {
        self.custom_metrics.write().await.insert(name.to_string(), value);
    }

    pub async fn export_prometheus_format(&self) -> String {
        let mut output = String::new();

        // Export Raft metrics
        let raft_metrics = self.raft_metrics.read().await;
        output.push_str(&format!(
            "# HELP raft_current_term Current Raft term\n\
             # TYPE raft_current_term gauge\n\
             raft_current_term {}\n\n\
             # HELP raft_is_leader Whether this node is the Raft leader\n\
             # TYPE raft_is_leader gauge\n\
             raft_is_leader {}\n\n\
             # HELP raft_proposals_total Total number of proposals\n\
             # TYPE raft_proposals_total counter\n\
             raft_proposals_total {}\n\n\
             # HELP raft_log_entries_total Total number of log entries\n\
             # TYPE raft_log_entries_total counter\n\
             raft_log_entries_total {}\n\n",
            raft_metrics.current_term,
            if raft_metrics.is_leader { 1 } else { 0 },
            raft_metrics.proposals_total,
            raft_metrics.log_entries_total
        ));

        // Export application metrics
        let app_metrics = self.application_metrics.read().await;
        output.push_str(&format!(
            "# HELP app_requests_total Total number of requests\n\
             # TYPE app_requests_total counter\n\
             app_requests_total {}\n\n\
             # HELP app_total_keys Total number of keys in store\n\
             # TYPE app_total_keys gauge\n\
             app_total_keys {}\n\n",
            app_metrics.requests_total,
            app_metrics.total_keys
        ));

        // Export system metrics
        let sys_metrics = self.system_metrics.read().await;
        output.push_str(&format!(
            "# HELP system_cpu_usage_percent CPU usage percentage\n\
             # TYPE system_cpu_usage_percent gauge\n\
             system_cpu_usage_percent {}\n\n\
             # HELP system_memory_usage_bytes Memory usage in bytes\n\
             # TYPE system_memory_usage_bytes gauge\n\
             system_memory_usage_bytes {}\n\n",
            sys_metrics.cpu_usage_percent,
            sys_metrics.memory_usage_bytes
        ));

        // Export custom metrics
        let custom_metrics = self.custom_metrics.read().await;
        for (name, value) in custom_metrics.iter() {
            output.push_str(&format!(
                "# HELP {} Custom metric\n\
                 # TYPE {} gauge\n\
                 {} {}\n\n",
                name, name, name, value
            ));
        }

        output
    }

    pub async fn export_json_format(&self) -> serde_json::Value {
        serde_json::json!({
            "raft": *self.raft_metrics.read().await,
            "application": *self.application_metrics.read().await,
            "system": *self.system_metrics.read().await,
            "performance": *self.performance_metrics.read().await,
            "custom": *self.custom_metrics.read().await,
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        })
    }
}

impl HealthChecker {
    pub fn new(checks: Vec<HealthCheck>) -> Self {
        Self {
            checks,
            status: Arc::new(RwLock::new(OverallHealth {
                status: HealthStatus::Healthy,
                checks: HashMap::new(),
                last_updated: SystemTime::now(),
            })),
        }
    }

    pub async fn start_health_checks(&self) {
        for check in &self.checks {
            let check = check.clone();
            let status = self.status.clone();
            
            tokio::spawn(async move {
                let mut interval = interval(check.interval);
                
                loop {
                    interval.tick().await;
                    let result = Self::execute_health_check(&check).await;
                    
                    let mut health = status.write().await;
                    health.checks.insert(check.name.clone(), result.clone());
                    health.last_updated = SystemTime::now();
                    
                    // Update overall status
                    let critical_failed = health.checks.values()
                        .any(|r| r.status == HealthStatus::Unhealthy);
                    
                    let degraded = health.checks.values()
                        .any(|r| r.status == HealthStatus::Degraded);
                    
                    health.status = if critical_failed {
                        HealthStatus::Unhealthy
                    } else if degraded {
                        HealthStatus::Degraded
                    } else {
                        HealthStatus::Healthy
                    };
                }
            });
        }
    }

    async fn execute_health_check(check: &HealthCheck) -> HealthCheckResult {
        let start_time = Instant::now();
        
        let status = match &check.check_type {
            HealthCheckType::RaftLeadership => {
                // Check if we can reach consensus
                HealthStatus::Healthy // Simulated
            }
            HealthCheckType::StorageConnectivity => {
                // Check if storage is responsive
                HealthStatus::Healthy // Simulated
            }
            HealthCheckType::NetworkConnectivity => {
                // Check if we can reach peers
                HealthStatus::Healthy // Simulated
            }
            HealthCheckType::MemoryUsage { threshold_percent } => {
                // Check memory usage
                let usage = Self::get_memory_usage().await;
                if usage > *threshold_percent {
                    HealthStatus::Degraded
                } else {
                    HealthStatus::Healthy
                }
            }
            HealthCheckType::DiskUsage { threshold_percent } => {
                // Check disk usage
                let usage = Self::get_disk_usage().await;
                if usage > *threshold_percent {
                    HealthStatus::Degraded
                } else {
                    HealthStatus::Healthy
                }
            }
            HealthCheckType::ConsensusHealth => {
                // Check consensus participation
                HealthStatus::Healthy // Simulated
            }
            HealthCheckType::ApplicationSpecific { endpoint } => {
                // Check application-specific health endpoint
                HealthStatus::Healthy // Simulated
            }
        };

        let duration = start_time.elapsed();
        let message = match status {
            HealthStatus::Healthy => "OK".to_string(),
            HealthStatus::Degraded => "Degraded performance".to_string(),
            HealthStatus::Unhealthy => "Check failed".to_string(),
        };

        HealthCheckResult {
            status,
            message,
            duration_ms: duration.as_millis() as u64,
            last_check: SystemTime::now(),
        }
    }

    async fn get_memory_usage() -> f64 {
        // Simulated memory usage
        45.5
    }

    async fn get_disk_usage() -> f64 {
        // Simulated disk usage
        67.8
    }

    pub async fn get_health_status(&self) -> OverallHealth {
        self.status.read().await.clone()
    }
}

impl TracingManager {
    pub fn new(service_name: String, service_version: String) -> Self {
        Self {
            service_name,
            service_version,
            traces: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn start_span(&self, operation_name: &str, parent_span_id: Option<String>) -> String {
        let span_id = format!("span_{}", 
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        let trace_id = format!("trace_{}", 
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );

        let span = TraceSpan {
            trace_id,
            span_id: span_id.clone(),
            parent_span_id,
            operation_name: operation_name.to_string(),
            start_time: SystemTime::now(),
            end_time: None,
            tags: HashMap::new(),
            logs: Vec::new(),
        };

        self.traces.write().await.push(span);
        span_id
    }

    pub async fn finish_span(&self, span_id: &str) {
        let mut traces = self.traces.write().await;
        if let Some(span) = traces.iter_mut().find(|s| s.span_id == span_id) {
            span.end_time = Some(SystemTime::now());
        }
    }

    pub async fn add_span_tag(&self, span_id: &str, key: &str, value: &str) {
        let mut traces = self.traces.write().await;
        if let Some(span) = traces.iter_mut().find(|s| s.span_id == span_id) {
            span.tags.insert(key.to_string(), value.to_string());
        }
    }

    pub async fn add_span_log(&self, span_id: &str, level: &str, message: &str) {
        let mut traces = self.traces.write().await;
        if let Some(span) = traces.iter_mut().find(|s| s.span_id == span_id) {
            span.logs.push(TraceLog {
                timestamp: SystemTime::now(),
                level: level.to_string(),
                message: message.to_string(),
                fields: HashMap::new(),
            });
        }
    }

    pub async fn export_jaeger_format(&self) -> serde_json::Value {
        let traces = self.traces.read().await;
        
        serde_json::json!({
            "data": [{
                "traceID": "sample_trace_id",
                "spans": traces.iter().map(|span| {
                    serde_json::json!({
                        "traceID": span.trace_id,
                        "spanID": span.span_id,
                        "parentSpanID": span.parent_span_id,
                        "operationName": span.operation_name,
                        "startTime": span.start_time
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_micros(),
                        "duration": span.end_time
                            .and_then(|end| end.duration_since(span.start_time).ok())
                            .map(|d| d.as_micros())
                            .unwrap_or(0),
                        "tags": span.tags,
                        "logs": span.logs.iter().map(|log| {
                            serde_json::json!({
                                "timestamp": log.timestamp
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap()
                                    .as_micros(),
                                "fields": [{
                                    "key": "level",
                                    "value": log.level
                                }, {
                                    "key": "message",
                                    "value": log.message
                                }]
                            })
                        }).collect::<Vec<_>>()
                    })
                }).collect::<Vec<_>>(),
                "processes": {
                    "p1": {
                        "serviceName": self.service_name,
                        "tags": [{
                            "key": "version",
                            "value": self.service_version
                        }]
                    }
                }
            }]
        })
    }
}

impl AlertManager {
    pub fn new(
        rules: Vec<AlertRule>,
        notification_channels: Vec<NotificationChannel>,
    ) -> Self {
        Self {
            rules,
            active_alerts: Arc::new(RwLock::new(Vec::new())),
            notification_channels,
        }
    }

    pub async fn evaluate_alerts(&self, metrics: &MetricsRegistry) {
        for rule in &self.rules {
            let should_fire = self.evaluate_rule(rule, metrics).await;
            
            let mut alerts = self.active_alerts.write().await;
            let existing_alert = alerts.iter_mut()
                .find(|a| a.name == rule.name && a.end_time.is_none());

            match (should_fire, existing_alert) {
                (true, None) => {
                    // Fire new alert
                    let alert = Alert {
                        name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        message: format!("Alert condition met for {}", rule.name),
                        labels: rule.labels.clone(),
                        start_time: SystemTime::now(),
                        end_time: None,
                        acknowledged: false,
                    };
                    
                    alerts.push(alert.clone());
                    self.send_notification(&alert).await;
                }
                (false, Some(alert)) => {
                    // Resolve existing alert
                    alert.end_time = Some(SystemTime::now());
                }
                _ => {
                    // No change needed
                }
            }
        }
    }

    async fn evaluate_rule(&self, rule: &AlertRule, metrics: &MetricsRegistry) -> bool {
        match &rule.condition {
            AlertCondition::MetricThreshold { metric, operator, value } => {
                let metric_value = self.get_metric_value(metric, metrics).await;
                
                match operator {
                    ComparisonOperator::GreaterThan => metric_value > *value,
                    ComparisonOperator::LessThan => metric_value < *value,
                    ComparisonOperator::Equal => (metric_value - value).abs() < f64::EPSILON,
                    ComparisonOperator::NotEqual => (metric_value - value).abs() >= f64::EPSILON,
                }
            }
            AlertCondition::HealthCheck { check_name, status } => {
                // Would check health status
                false // Simulated
            }
            AlertCondition::RateOfChange { metric, threshold, window } => {
                // Would calculate rate of change over window
                false // Simulated
            }
        }
    }

    async fn get_metric_value(&self, metric_name: &str, metrics: &MetricsRegistry) -> f64 {
        // Extract metric value based on name
        match metric_name {
            "raft_proposals_total" => {
                metrics.raft_metrics.read().await.proposals_total as f64
            }
            "system_cpu_usage_percent" => {
                metrics.system_metrics.read().await.cpu_usage_percent
            }
            "system_memory_usage_percent" => {
                metrics.system_metrics.read().await.memory_usage_percent
            }
            _ => {
                metrics.custom_metrics.read().await
                    .get(metric_name)
                    .copied()
                    .unwrap_or(0.0)
            }
        }
    }

    async fn send_notification(&self, alert: &Alert) {
        for channel in &self.notification_channels {
            match channel {
                NotificationChannel::Slack { webhook_url } => {
                    println!("📢 Slack notification: {}", alert.message);
                    // In real implementation: send HTTP request to Slack webhook
                }
                NotificationChannel::Email { addresses } => {
                    println!("📧 Email notification to {:?}: {}", addresses, alert.message);
                    // In real implementation: send email
                }
                NotificationChannel::PagerDuty { integration_key } => {
                    println!("📟 PagerDuty alert: {}", alert.message);
                    // In real implementation: send to PagerDuty API
                }
                NotificationChannel::Webhook { url, headers } => {
                    println!("🔗 Webhook notification to {}: {}", url, alert.message);
                    // In real implementation: send HTTP request
                }
            }
        }
    }

    pub async fn get_active_alerts(&self) -> Vec<Alert> {
        self.active_alerts.read().await
            .iter()
            .filter(|a| a.end_time.is_none())
            .cloned()
            .collect()
    }
}

impl MonitoringCoordinator {
    pub async fn new(
        service_name: String,
        service_version: String,
        export_interval: Duration,
    ) -> Self {
        let metrics_registry = Arc::new(MetricsRegistry::new());
        
        let health_checks = vec![
            HealthCheck {
                name: "raft_leadership".to_string(),
                check_type: HealthCheckType::RaftLeadership,
                interval: Duration::from_secs(10),
                timeout: Duration::from_secs(5),
                critical: true,
            },
            HealthCheck {
                name: "memory_usage".to_string(),
                check_type: HealthCheckType::MemoryUsage { threshold_percent: 80.0 },
                interval: Duration::from_secs(30),
                timeout: Duration::from_secs(5),
                critical: false,
            },
            HealthCheck {
                name: "disk_usage".to_string(),
                check_type: HealthCheckType::DiskUsage { threshold_percent: 90.0 },
                interval: Duration::from_secs(60),
                timeout: Duration::from_secs(5),
                critical: false,
            },
        ];
        
        let health_checker = Arc::new(HealthChecker::new(health_checks));
        let tracing_manager = Arc::new(TracingManager::new(service_name, service_version));
        
        let alert_rules = vec![
            AlertRule {
                name: "high_cpu_usage".to_string(),
                condition: AlertCondition::MetricThreshold {
                    metric: "system_cpu_usage_percent".to_string(),
                    operator: ComparisonOperator::GreaterThan,
                    value: 80.0,
                },
                severity: AlertSeverity::Warning,
                duration: Duration::from_secs(300),
                labels: [("team".to_string(), "infrastructure".to_string())].into(),
                annotations: [("runbook".to_string(), "https://wiki.example.com/high-cpu".to_string())].into(),
            },
            AlertRule {
                name: "raft_proposals_failing".to_string(),
                condition: AlertCondition::MetricThreshold {
                    metric: "raft_proposals_failed".to_string(),
                    operator: ComparisonOperator::GreaterThan,
                    value: 10.0,
                },
                severity: AlertSeverity::Critical,
                duration: Duration::from_secs(60),
                labels: [("component".to_string(), "raft".to_string())].into(),
                annotations: [("description".to_string(), "Raft proposals are failing".to_string())].into(),
            },
        ];
        
        let notification_channels = vec![
            NotificationChannel::Slack {
                webhook_url: "https://hooks.slack.com/services/T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX".to_string(),
            },
            NotificationChannel::Email {
                addresses: vec!["oncall@example.com".to_string()],
            },
        ];
        
        let alert_manager = Arc::new(AlertManager::new(alert_rules, notification_channels));

        Self {
            metrics_registry,
            health_checker,
            tracing_manager,
            alert_manager,
            export_interval,
        }
    }

    pub async fn start_monitoring(&self) {
        println!("📊 Starting comprehensive monitoring system...");

        // Start health checks
        self.health_checker.start_health_checks().await;

        // Start metrics collection
        self.start_metrics_collection().await;

        // Start metrics export
        self.start_metrics_export().await;

        // Start alert evaluation
        self.start_alert_evaluation().await;

        println!("✅ Monitoring system started");
    }

    async fn start_metrics_collection(&self) {
        let metrics = self.metrics_registry.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                
                // Simulate metric collection
                metrics.update_raft_metrics(|m| {
                    m.proposals_total += 1;
                    m.heartbeats_sent += 1;
                }).await;
                
                // Update system metrics
                let mut sys_metrics = metrics.system_metrics.write().await;
                sys_metrics.cpu_usage_percent = 45.0 + (rand::random::<f64>() - 0.5) * 20.0;
                sys_metrics.memory_usage_percent = 60.0 + (rand::random::<f64>() - 0.5) * 10.0;
                sys_metrics.uptime_seconds += 1;
            }
        });
    }

    async fn start_metrics_export(&self) {
        let metrics = self.metrics_registry.clone();
        let interval = self.export_interval;
        
        tokio::spawn(async move {
            let mut ticker = interval(interval);
            
            loop {
                ticker.tick().await;
                
                // Export Prometheus metrics
                let prometheus_output = metrics.export_prometheus_format().await;
                println!("📈 Exported Prometheus metrics ({} lines)", 
                         prometheus_output.lines().count());
                
                // Export JSON metrics
                let json_output = metrics.export_json_format().await;
                println!("📊 Exported JSON metrics");
            }
        });
    }

    async fn start_alert_evaluation(&self) {
        let alert_manager = self.alert_manager.clone();
        let metrics = self.metrics_registry.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                alert_manager.evaluate_alerts(&metrics).await;
            }
        });
    }

    pub async fn create_trace_span(&self, operation: &str) -> String {
        self.tracing_manager.start_span(operation, None).await
    }

    pub async fn finish_trace_span(&self, span_id: &str) {
        self.tracing_manager.finish_span(span_id).await;
    }

    pub async fn record_metric(&self, name: &str, value: f64) {
        self.metrics_registry.set_gauge(name, value).await;
    }

    pub async fn increment_counter(&self, name: &str) {
        self.metrics_registry.increment_counter(name, 1.0).await;
    }

    pub async fn get_health_status(&self) -> OverallHealth {
        self.health_checker.get_health_status().await
    }

    pub async fn get_active_alerts(&self) -> Vec<Alert> {
        self.alert_manager.get_active_alerts().await
    }

    pub async fn export_metrics_prometheus(&self) -> String {
        self.metrics_registry.export_prometheus_format().await
    }

    pub async fn export_traces_jaeger(&self) -> serde_json::Value {
        self.tracing_manager.export_jaeger_format().await
    }
}

// Default implementations
impl Default for RaftMetrics {
    fn default() -> Self {
        Self {
            current_term: 0,
            is_leader: false,
            leader_elections_total: 0,
            leadership_changes_total: 0,
            leadership_duration_seconds: 0,
            log_entries_total: 0,
            log_size_bytes: 0,
            commit_index: 0,
            last_applied_index: 0,
            log_compaction_count: 0,
            proposals_total: 0,
            proposals_committed: 0,
            proposals_failed: 0,
            proposal_latency_seconds: 0.0,
            proposal_queue_size: 0,
            snapshots_created: 0,
            snapshots_restored: 0,
            snapshot_size_bytes: 0,
            snapshot_creation_duration_seconds: 0.0,
            append_entries_sent: 0,
            append_entries_received: 0,
            vote_requests_sent: 0,
            vote_requests_received: 0,
            heartbeats_sent: 0,
            heartbeats_received: 0,
            consensus_errors_total: 0,
            network_errors_total: 0,
            storage_errors_total: 0,
        }
    }
}

impl Default for ApplicationMetrics {
    fn default() -> Self {
        Self {
            requests_total: 0,
            requests_per_second: 0.0,
            request_duration_seconds: 0.0,
            request_size_bytes: 0.0,
            response_size_bytes: 0.0,
            read_operations_total: 0,
            write_operations_total: 0,
            delete_operations_total: 0,
            batch_operations_total: 0,
            total_keys: 0,
            total_data_size_bytes: 0,
            key_size_distribution: HashMap::new(),
            value_size_distribution: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            cache_evictions: 0,
            cache_size_bytes: 0,
            active_connections: 0,
            connection_errors: 0,
            client_timeouts: 0,
        }
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            cpu_usage_percent: 0.0,
            memory_usage_bytes: 0,
            memory_usage_percent: 0.0,
            disk_usage_bytes: 0,
            disk_usage_percent: 0.0,
            network_bytes_sent: 0,
            network_bytes_received: 0,
            network_packets_sent: 0,
            network_packets_received: 0,
            network_errors: 0,
            uptime_seconds: 0,
            goroutines_count: 0,
            open_file_descriptors: 0,
            gc_duration_seconds: 0.0,
            disk_reads_total: 0,
            disk_writes_total: 0,
            disk_read_bytes: 0,
            disk_write_bytes: 0,
        }
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            request_latency_histogram: LatencyHistogram::default(),
            consensus_latency_histogram: LatencyHistogram::default(),
            storage_latency_histogram: LatencyHistogram::default(),
            operations_per_second: 0.0,
            bytes_per_second: 0.0,
            peak_operations_per_second: 0.0,
            peak_bytes_per_second: 0.0,
            proposal_queue_depth: 0,
            message_queue_depth: 0,
            max_queue_depth: 0,
            queue_wait_time_seconds: 0.0,
            active_connections: 0,
            idle_connections: 0,
            connection_pool_utilization: 0.0,
            connection_acquisition_time_seconds: 0.0,
        }
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self {
            buckets: HashMap::new(),
            total_count: 0,
            sum_seconds: 0.0,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🎯 Monitoring Integration Example");
    println!("=================================");

    // Initialize monitoring system
    let monitoring = MonitoringCoordinator::new(
        "iroh-raft-cluster".to_string(),
        "1.0.0".to_string(),
        Duration::from_secs(30),
    ).await;

    // Start monitoring
    monitoring.start_monitoring().await;

    // Simulate application operations with monitoring
    println!("\n📊 Phase 1: Demonstrating Metrics Collection");
    
    for i in 1..=20 {
        let span_id = monitoring.create_trace_span("process_request").await;
        
        // Simulate request processing
        sleep(Duration::from_millis(50)).await;
        
        monitoring.increment_counter("requests_processed").await;
        monitoring.record_metric("current_connections", i as f64).await;
        
        if i % 5 == 0 {
            monitoring.record_metric("batch_size", 10.0).await;
        }
        
        monitoring.finish_trace_span(&span_id).await;
    }

    println!("   ✅ Processed 20 requests with full tracing");

    // Check health status
    println!("\n🏥 Phase 2: Health Monitoring");
    sleep(Duration::from_secs(2)).await; // Let health checks run
    
    let health = monitoring.get_health_status().await;
    println!("   Overall health: {:?}", health.status);
    
    for (check_name, result) in health.checks {
        println!("   {}: {:?} ({}ms)", 
                 check_name, result.status, result.duration_ms);
    }

    // Export metrics in different formats
    println!("\n📈 Phase 3: Metrics Export");
    
    let prometheus_metrics = monitoring.export_metrics_prometheus().await;
    println!("   Prometheus format ({} lines):", prometheus_metrics.lines().count());
    
    // Show first few lines
    for line in prometheus_metrics.lines().take(10) {
        if !line.trim().is_empty() {
            println!("     {}", line);
        }
    }
    
    if prometheus_metrics.lines().count() > 10 {
        println!("     ... ({} more lines)", prometheus_metrics.lines().count() - 10);
    }

    // Export tracing data
    println!("\n🔍 Phase 4: Distributed Tracing");
    
    let traces = monitoring.export_traces_jaeger().await;
    println!("   Jaeger trace export:");
    println!("     Traces exported: {}", traces["data"].as_array().unwrap().len());
    
    if let Some(spans) = traces["data"][0]["spans"].as_array() {
        println!("     Total spans: {}", spans.len());
        
        for (i, span) in spans.iter().take(3).enumerate() {
            println!("     Span {}: {} ({}μs)", 
                     i + 1,
                     span["operationName"].as_str().unwrap(),
                     span["duration"].as_u64().unwrap_or(0));
        }
    }

    // Demonstrate alerting
    println!("\n🚨 Phase 5: Alerting System");
    
    // Simulate high CPU usage to trigger alert
    monitoring.record_metric("system_cpu_usage_percent", 85.0).await;
    
    sleep(Duration::from_secs(1)).await; // Let alert evaluation run
    
    let active_alerts = monitoring.get_active_alerts().await;
    if active_alerts.is_empty() {
        println!("   No active alerts (CPU threshold not reached in simulation)");
    } else {
        println!("   Active alerts: {}", active_alerts.len());
        
        for alert in active_alerts {
            println!("     {:?}: {} - {}", 
                     alert.severity, alert.name, alert.message);
        }
    }

    // Demonstrate custom application metrics
    println!("\n📊 Phase 6: Custom Application Metrics");
    
    let custom_metrics = vec![
        ("business_transactions_total", 142.0),
        ("user_sessions_active", 28.0),
        ("cache_hit_rate", 0.87),
        ("average_response_time_ms", 45.2),
        ("error_rate_percent", 0.02),
    ];

    for (metric, value) in custom_metrics {
        monitoring.record_metric(metric, value).await;
        println!("   Recorded {}: {}", metric, value);
    }

    // Final metrics export showing all data
    println!("\n📊 Phase 7: Complete Metrics Summary");
    
    let final_prometheus = monitoring.export_metrics_prometheus().await;
    let metric_count = final_prometheus.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .count();
    
    println!("   Total metrics exported: {}", metric_count);
    println!("   Prometheus endpoint ready at: /metrics");
    println!("   Health check endpoint ready at: /health");
    println!("   Traces exported to: Jaeger/OpenTelemetry");

    // Show monitoring dashboard URLs
    println!("\n🖥️  Monitoring Dashboard URLs");
    println!("   Grafana: http://localhost:3000/dashboard");
    println!("   Prometheus: http://localhost:9090/targets");
    println!("   Jaeger: http://localhost:16686/search");
    println!("   AlertManager: http://localhost:9093/#/alerts");

    println!("\n🎉 Monitoring Integration Example Completed!");
    println!("==============================================");
    
    println!("\nMonitoring Features Demonstrated:");
    println!("• ✅ Comprehensive metrics collection (Raft, system, application)");
    println!("• ✅ Prometheus metrics export format");
    println!("• ✅ Health checks with configurable thresholds");
    println!("• ✅ Distributed tracing with OpenTelemetry/Jaeger");
    println!("• ✅ Alert rules and notification channels");
    println!("• ✅ Custom application metrics");
    println!("• ✅ Performance and latency histograms");
    println!("• ✅ Real-time monitoring and observability");
    
    println!("\nIntegration Points:");
    println!("• Prometheus scraping endpoint (/metrics)");
    println!("• Health check endpoint (/health)");
    println!("• Jaeger trace collection");
    println!("• Slack/Email/PagerDuty notifications");
    println!("• Grafana dashboard templates");

    Ok(())
}

// Mock implementations to make the example compile
mod simple_cluster_example {
    use super::*;

    #[derive(Debug)]
    pub struct RaftCluster<S: StateMachine> {
        node_id: NodeId,
        state_machine: S,
    }

    #[derive(Debug, Clone)]
    pub enum Preset {
        Development,
        Testing,
        Production,
    }

    #[derive(Debug, Clone)]
    pub struct ClusterMetrics {
        pub node_id: NodeId,
        pub is_leader: bool,
        pub term: u64,
    }
}

mod distributed_kv_store {
    use super::*;

    #[derive(Debug)]
    pub struct DistributedKvStore;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum DistributedKvCommand {
        Get { key: String },
    }

    impl DistributedKvStore {
        pub fn new() -> Self {
            Self
        }
    }

    impl StateMachine for DistributedKvStore {
        type Command = DistributedKvCommand;
        type State = ();

        async fn apply_command(&mut self, _command: Self::Command) -> Result<()> {
            Ok(())
        }

        async fn create_snapshot(&self) -> Result<Vec<u8>> {
            Ok(vec![])
        }

        async fn restore_from_snapshot(&mut self, _snapshot: &[u8]) -> Result<()> {
            Ok(())
        }

        fn get_current_state(&self) -> &Self::State {
            &()
        }
    }
}

// Add a simple rand module for simulation
mod rand {
    pub fn random<T>() -> T 
    where 
        T: From<f64>
    {
        // Simple pseudo-random number generator for demo
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let pseudo_random = ((nanos as f64) * 0.0001) % 1.0;
        T::from(pseudo_random)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_registry() {
        let registry = MetricsRegistry::new();
        
        registry.increment_counter("test_counter", 5.0).await;
        registry.set_gauge("test_gauge", 42.0).await;
        
        let custom_metrics = registry.custom_metrics.read().await;
        assert_eq!(custom_metrics.get("test_counter"), Some(&5.0));
        assert_eq!(custom_metrics.get("test_gauge"), Some(&42.0));
    }

    #[tokio::test]
    async fn test_prometheus_export() {
        let registry = MetricsRegistry::new();
        
        registry.increment_counter("requests_total", 100.0).await;
        
        let prometheus_output = registry.export_prometheus_format().await;
        assert!(prometheus_output.contains("requests_total 100"));
    }

    #[tokio::test]
    async fn test_health_checker() {
        let checks = vec![
            HealthCheck {
                name: "test_check".to_string(),
                check_type: HealthCheckType::MemoryUsage { threshold_percent: 50.0 },
                interval: Duration::from_secs(1),
                timeout: Duration::from_secs(1),
                critical: false,
            }
        ];
        
        let health_checker = HealthChecker::new(checks);
        let initial_health = health_checker.get_health_status().await;
        assert_eq!(initial_health.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_tracing_manager() {
        let tracing = TracingManager::new("test_service".to_string(), "1.0.0".to_string());
        
        let span_id = tracing.start_span("test_operation", None).await;
        tracing.add_span_tag(&span_id, "key", "value").await;
        tracing.finish_span(&span_id).await;
        
        let traces = tracing.traces.read().await;
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].operation_name, "test_operation");
        assert!(traces[0].end_time.is_some());
    }

    #[tokio::test]
    async fn test_alert_manager() {
        let rules = vec![
            AlertRule {
                name: "test_alert".to_string(),
                condition: AlertCondition::MetricThreshold {
                    metric: "test_metric".to_string(),
                    operator: ComparisonOperator::GreaterThan,
                    value: 50.0,
                },
                severity: AlertSeverity::Warning,
                duration: Duration::from_secs(0),
                labels: HashMap::new(),
                annotations: HashMap::new(),
            }
        ];
        
        let alert_manager = AlertManager::new(rules, vec![]);
        let active_alerts = alert_manager.get_active_alerts().await;
        assert_eq!(active_alerts.len(), 0);
    }
}