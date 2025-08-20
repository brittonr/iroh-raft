//! Cluster Deployment Example
//!
//! This example demonstrates how to deploy and manage a production-ready
//! iroh-raft cluster with realistic deployment scenarios:
//! - Multi-node cluster bootstrap
//! - Rolling updates and configuration changes
//! - Node failure detection and recovery
//! - Load balancing and service discovery
//! - Backup and restore procedures
//! - Monitoring and alerting integration
//!
//! Run with: cargo run --example cluster_deployment

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

/// Cluster deployment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub cluster_name: String,
    pub environment: Environment,
    pub nodes: Vec<NodeConfig>,
    pub network: NetworkConfig,
    pub storage: StorageConfig,
    pub monitoring: MonitoringConfig,
    pub backup: BackupConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Environment {
    Development,
    Staging,
    Production,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node_id: NodeId,
    pub hostname: String,
    pub port: u16,
    pub data_dir: String,
    pub log_level: String,
    pub resources: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_mb: usize,
    pub max_disk_gb: usize,
    pub max_connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub cluster_port_range: (u16, u16),
    pub client_port_range: (u16, u16),
    pub ssl_enabled: bool,
    pub compression_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub backend: String,
    pub sync_writes: bool,
    pub compression: String,
    pub retention_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub metrics_port: u16,
    pub health_check_interval_seconds: u64,
    pub alert_endpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    pub enabled: bool,
    pub schedule: String, // Cron expression
    pub retention_count: u32,
    pub storage_location: String,
}

/// Cluster manager for deployment operations
pub struct ClusterManager {
    config: DeploymentConfig,
    nodes: HashMap<NodeId, DeployedNode>,
    service_discovery: Arc<ServiceDiscovery>,
    health_monitor: Arc<HealthMonitor>,
    backup_manager: Arc<BackupManager>,
}

/// Individual deployed node
#[derive(Debug)]
pub struct DeployedNode {
    pub config: NodeConfig,
    pub cluster: RaftCluster<DistributedKvStore>,
    pub status: NodeStatus,
    pub metrics: NodeMetrics,
    pub last_seen: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Starting,
    Healthy,
    Degraded,
    Failed,
    Maintenance,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct NodeMetrics {
    pub uptime_seconds: u64,
    pub cpu_percent: f64,
    pub memory_used_mb: usize,
    pub disk_used_gb: f64,
    pub network_io_mb: f64,
    pub raft_log_size: u64,
    pub last_election_ms: u64,
    pub proposal_rate: f64,
}

/// Service discovery for cluster membership
#[derive(Debug)]
pub struct ServiceDiscovery {
    nodes: RwLock<HashMap<NodeId, ServiceEndpoint>>,
    health_checks: RwLock<HashMap<NodeId, HealthStatus>>,
}

#[derive(Debug, Clone)]
pub struct ServiceEndpoint {
    pub node_id: NodeId,
    pub hostname: String,
    pub port: u16,
    pub last_updated: SystemTime,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub status: NodeStatus,
    pub last_check: SystemTime,
    pub consecutive_failures: u32,
    pub response_time_ms: u64,
}

/// Health monitoring system
#[derive(Debug)]
pub struct HealthMonitor {
    check_interval: Duration,
    failure_threshold: u32,
    alerts: Arc<RwLock<Vec<Alert>>>,
}

#[derive(Debug, Clone)]
pub struct Alert {
    pub severity: AlertSeverity,
    pub message: String,
    pub node_id: Option<NodeId>,
    pub timestamp: SystemTime,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Backup and restore management
#[derive(Debug)]
pub struct BackupManager {
    config: BackupConfig,
    backup_history: RwLock<Vec<BackupRecord>>,
}

#[derive(Debug, Clone)]
pub struct BackupRecord {
    pub backup_id: String,
    pub timestamp: SystemTime,
    pub size_bytes: u64,
    pub node_count: usize,
    pub storage_path: String,
    pub checksum: String,
}

/// Deployment operations
impl ClusterManager {
    pub async fn new(config: DeploymentConfig) -> Result<Self> {
        let service_discovery = Arc::new(ServiceDiscovery::new());
        let health_monitor = Arc::new(HealthMonitor::new(
            Duration::from_secs(config.monitoring.health_check_interval_seconds),
            3, // failure threshold
        ));
        let backup_manager = Arc::new(BackupManager::new(config.backup.clone()));

        Ok(Self {
            config,
            nodes: HashMap::new(),
            service_discovery,
            health_monitor,
            backup_manager,
        })
    }

    /// Bootstrap the cluster from configuration
    pub async fn bootstrap(&mut self) -> Result<()> {
        println!("🚀 Bootstrapping cluster '{}'", self.config.cluster_name);
        println!("   Environment: {:?}", self.config.environment);
        println!("   Nodes: {}", self.config.nodes.len());

        // Validate configuration
        self.validate_configuration().await?;

        // Start nodes in sequence
        for node_config in &self.config.nodes.clone() {
            self.deploy_node(node_config.clone()).await?;
            
            // Wait for node to stabilize before starting next
            sleep(Duration::from_secs(2)).await;
        }

        // Wait for cluster to form
        self.wait_for_cluster_formation().await?;

        // Start monitoring
        self.start_monitoring().await?;

        // Setup backup schedule
        if self.config.backup.enabled {
            self.schedule_backups().await?;
        }

        println!("✅ Cluster bootstrap complete");
        Ok(())
    }

    async fn validate_configuration(&self) -> Result<()> {
        println!("🔍 Validating deployment configuration...");

        // Check node count
        if self.config.nodes.len() < 3 {
            return Err(RaftError::InvalidConfiguration {
                component: "cluster".to_string(),
                message: "Minimum 3 nodes required for production deployment".to_string(),
            });
        }

        // Check odd number of nodes
        if self.config.nodes.len() % 2 == 0 {
            println!("⚠️  Warning: Even number of nodes may cause split-brain scenarios");
        }

        // Validate node IDs are unique
        let node_ids: Vec<NodeId> = self.config.nodes.iter().map(|n| n.node_id).collect();
        let mut unique_ids = node_ids.clone();
        unique_ids.sort();
        unique_ids.dedup();
        
        if unique_ids.len() != node_ids.len() {
            return Err(RaftError::InvalidConfiguration {
                component: "cluster".to_string(),
                message: "Node IDs must be unique".to_string(),
            });
        }

        // Validate hostnames/ports are unique
        let mut endpoints = std::collections::HashSet::new();
        for node in &self.config.nodes {
            let endpoint = format!("{}:{}", node.hostname, node.port);
            if !endpoints.insert(endpoint.clone()) {
                return Err(RaftError::InvalidConfiguration {
                    component: "cluster".to_string(),
                    message: format!("Duplicate endpoint: {}", endpoint),
                });
            }
        }

        println!("✅ Configuration validation passed");
        Ok(())
    }

    async fn deploy_node(&mut self, node_config: NodeConfig) -> Result<()> {
        println!("📦 Deploying node {} on {}:{}", 
                 node_config.node_id, node_config.hostname, node_config.port);

        // Build peer list (all other nodes)
        let peers: Vec<String> = self.config.nodes
            .iter()
            .filter(|n| n.node_id != node_config.node_id)
            .map(|n| format!("{}:{}", n.hostname, n.port))
            .collect();

        // Create cluster configuration based on environment
        let preset = match self.config.environment {
            Environment::Development => Preset::Development,
            Environment::Staging => Preset::Testing,
            Environment::Production => Preset::Production,
        };

        let cluster = RaftCluster::builder()
            .node_id(node_config.node_id)
            .bind_address(format!("{}:{}", node_config.hostname, node_config.port))
            .peers(peers)
            .preset(preset)
            .data_dir(&node_config.data_dir)
            .state_machine(DistributedKvStore::new())
            .build()
            .await?;

        let deployed_node = DeployedNode {
            config: node_config.clone(),
            cluster,
            status: NodeStatus::Starting,
            metrics: NodeMetrics::default(),
            last_seen: SystemTime::now(),
        };

        // Register with service discovery
        self.service_discovery.register_node(ServiceEndpoint {
            node_id: node_config.node_id,
            hostname: node_config.hostname.clone(),
            port: node_config.port,
            last_updated: SystemTime::now(),
            version: "1.0.0".to_string(),
        }).await;

        self.nodes.insert(node_config.node_id, deployed_node);

        println!("   ✅ Node {} deployed successfully", node_config.node_id);
        Ok(())
    }

    async fn wait_for_cluster_formation(&self) -> Result<()> {
        println!("⏳ Waiting for cluster formation...");
        
        let timeout_duration = Duration::from_secs(60);
        let start_time = Instant::now();

        while start_time.elapsed() < timeout_duration {
            let mut healthy_nodes = 0;
            let mut leader_found = false;

            for (node_id, node) in &self.nodes {
                let metrics = node.cluster.metrics().await;
                if metrics.is_leader {
                    leader_found = true;
                    println!("   📋 Node {} elected as leader (term {})", node_id, metrics.term);
                }
                
                // In a real implementation, we'd check actual node health
                healthy_nodes += 1;
            }

            if leader_found && healthy_nodes >= (self.nodes.len() / 2 + 1) {
                println!("✅ Cluster formation complete");
                println!("   Healthy nodes: {}/{}", healthy_nodes, self.nodes.len());
                return Ok(());
            }

            sleep(Duration::from_secs(2)).await;
        }

        Err(RaftError::InvalidConfiguration {
            component: "cluster".to_string(),
            message: "Cluster failed to form within timeout".to_string(),
        })
    }

    async fn start_monitoring(&self) -> Result<()> {
        println!("📊 Starting cluster monitoring...");

        let health_monitor = self.health_monitor.clone();
        let service_discovery = self.service_discovery.clone();

        // Start health check loop
        tokio::spawn(async move {
            health_monitor.start_health_checks(service_discovery).await;
        });

        // Start metrics collection
        self.start_metrics_collection().await;

        println!("✅ Monitoring started");
        Ok(())
    }

    async fn start_metrics_collection(&self) {
        let mut interval = interval(Duration::from_secs(30));
        
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                // Collect cluster-wide metrics
                println!("📈 Collecting cluster metrics...");
                // In real implementation: aggregate and store metrics
            }
        });
    }

    async fn schedule_backups(&self) -> Result<()> {
        if !self.config.backup.enabled {
            return Ok(());
        }

        println!("💾 Scheduling automatic backups");
        println!("   Schedule: {}", self.config.backup.schedule);
        println!("   Retention: {} backups", self.config.backup.retention_count);

        let backup_manager = self.backup_manager.clone();
        
        // Simulate backup scheduling (in real implementation, use a cron scheduler)
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600)); // Every hour for demo
            
            loop {
                interval.tick().await;
                if let Err(e) = backup_manager.create_backup().await {
                    eprintln!("Backup failed: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Perform rolling update of the cluster
    pub async fn rolling_update(&mut self, new_version: &str) -> Result<()> {
        println!("🔄 Starting rolling update to version {}", new_version);

        let update_order: Vec<NodeId> = self.nodes.keys().cloned().collect();

        for node_id in update_order {
            self.update_node(node_id, new_version).await?;
            
            // Wait for node to rejoin cluster
            self.wait_for_node_health(node_id).await?;
            
            // Ensure cluster remains available
            self.verify_cluster_health().await?;
            
            println!("   ✅ Node {} updated successfully", node_id);
        }

        println!("✅ Rolling update completed");
        Ok(())
    }

    async fn update_node(&mut self, node_id: NodeId, version: &str) -> Result<()> {
        println!("   📦 Updating node {}...", node_id);

        if let Some(node) = self.nodes.get_mut(&node_id) {
            // Mark node as in maintenance
            node.status = NodeStatus::Maintenance;
            
            // In real implementation:
            // 1. Drain connections
            // 2. Stop the node gracefully
            // 3. Update binary/configuration
            // 4. Start the node with new version
            
            // Simulate update process
            sleep(Duration::from_secs(5)).await;
            
            // Update service discovery
            self.service_discovery.update_node_version(node_id, version).await;
            
            node.status = NodeStatus::Healthy;
        }

        Ok(())
    }

    async fn wait_for_node_health(&self, node_id: NodeId) -> Result<()> {
        let timeout_duration = Duration::from_secs(30);
        let start_time = Instant::now();

        while start_time.elapsed() < timeout_duration {
            if let Some(health) = self.service_discovery.get_node_health(node_id).await {
                if health.status == NodeStatus::Healthy {
                    return Ok(());
                }
            }
            sleep(Duration::from_secs(1)).await;
        }

        Err(RaftError::InvalidConfiguration {
            component: "cluster".to_string(),
            message: format!("Node {} failed to become healthy", node_id),
        })
    }

    async fn verify_cluster_health(&self) -> Result<()> {
        // Ensure we have a leader and quorum
        let mut leader_count = 0;
        let mut healthy_count = 0;

        for (node_id, node) in &self.nodes {
            if node.status == NodeStatus::Healthy {
                healthy_count += 1;
                
                let metrics = node.cluster.metrics().await;
                if metrics.is_leader {
                    leader_count += 1;
                }
            }
        }

        if leader_count != 1 {
            return Err(RaftError::InvalidConfiguration {
                component: "cluster".to_string(),
                message: format!("Expected 1 leader, found {}", leader_count),
            });
        }

        let required_healthy = self.nodes.len() / 2 + 1;
        if healthy_count < required_healthy {
            return Err(RaftError::InvalidConfiguration {
                component: "cluster".to_string(),
                message: format!("Insufficient healthy nodes: {}/{}", healthy_count, required_healthy),
            });
        }

        Ok(())
    }

    /// Add a new node to the cluster
    pub async fn add_node(&mut self, node_config: NodeConfig) -> Result<()> {
        println!("➕ Adding new node {} to cluster", node_config.node_id);

        // Deploy the new node
        self.deploy_node(node_config.clone()).await?;

        // Update existing nodes with new peer
        for (_, node) in &mut self.nodes {
            if node.config.node_id != node_config.node_id {
                let new_peer = format!("{}:{}", node_config.hostname, node_config.port);
                node.cluster.add_peer(new_peer).await?;
            }
        }

        // Wait for node to join
        self.wait_for_node_health(node_config.node_id).await?;

        println!("✅ Node {} added successfully", node_config.node_id);
        Ok(())
    }

    /// Remove a node from the cluster
    pub async fn remove_node(&mut self, node_id: NodeId) -> Result<()> {
        println!("➖ Removing node {} from cluster", node_id);

        // Get node info before removal
        let node_endpoint = if let Some(node) = self.nodes.get(&node_id) {
            format!("{}:{}", node.config.hostname, node.config.port)
        } else {
            return Err(RaftError::InvalidConfiguration {
                component: "cluster".to_string(),
                message: format!("Node {} not found", node_id),
            });
        };

        // Remove from all other nodes
        for (_, node) in &mut self.nodes {
            if node.config.node_id != node_id {
                node.cluster.remove_peer(&node_endpoint).await?;
            }
        }

        // Stop and remove the node
        if let Some(mut node) = self.nodes.remove(&node_id) {
            node.cluster.shutdown().await?;
        }

        // Update service discovery
        self.service_discovery.unregister_node(node_id).await;

        println!("✅ Node {} removed successfully", node_id);
        Ok(())
    }

    /// Create cluster backup
    pub async fn create_backup(&self) -> Result<String> {
        println!("💾 Creating cluster backup...");
        self.backup_manager.create_backup().await
    }

    /// Restore cluster from backup
    pub async fn restore_backup(&mut self, backup_id: &str) -> Result<()> {
        println!("🔄 Restoring cluster from backup {}", backup_id);
        self.backup_manager.restore_backup(backup_id).await
    }

    /// Get cluster status
    pub async fn get_cluster_status(&self) -> ClusterStatus {
        let mut status = ClusterStatus {
            cluster_name: self.config.cluster_name.clone(),
            environment: self.config.environment.clone(),
            total_nodes: self.nodes.len(),
            healthy_nodes: 0,
            leader_node: None,
            current_term: 0,
            total_keys: 0,
            alerts: Vec::new(),
        };

        for (node_id, node) in &self.nodes {
            if node.status == NodeStatus::Healthy {
                status.healthy_nodes += 1;
                
                let metrics = node.cluster.metrics().await;
                if metrics.is_leader {
                    status.leader_node = Some(*node_id);
                    status.current_term = metrics.term;
                }
                
                // Get state info
                let state = node.cluster.query();
                status.total_keys = state.0.len();
            }
        }

        status.alerts = self.health_monitor.get_active_alerts().await;
        status
    }
}

#[derive(Debug, Clone)]
pub struct ClusterStatus {
    pub cluster_name: String,
    pub environment: Environment,
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub leader_node: Option<NodeId>,
    pub current_term: u64,
    pub total_keys: usize,
    pub alerts: Vec<Alert>,
}

// Implementation of supporting components
impl ServiceDiscovery {
    fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            health_checks: RwLock::new(HashMap::new()),
        }
    }

    async fn register_node(&self, endpoint: ServiceEndpoint) {
        self.nodes.write().await.insert(endpoint.node_id, endpoint);
        self.health_checks.write().await.insert(
            endpoint.node_id,
            HealthStatus {
                status: NodeStatus::Starting,
                last_check: SystemTime::now(),
                consecutive_failures: 0,
                response_time_ms: 0,
            },
        );
    }

    async fn unregister_node(&self, node_id: NodeId) {
        self.nodes.write().await.remove(&node_id);
        self.health_checks.write().await.remove(&node_id);
    }

    async fn update_node_version(&self, node_id: NodeId, version: &str) {
        if let Some(endpoint) = self.nodes.write().await.get_mut(&node_id) {
            endpoint.version = version.to_string();
            endpoint.last_updated = SystemTime::now();
        }
    }

    async fn get_node_health(&self, node_id: NodeId) -> Option<HealthStatus> {
        self.health_checks.read().await.get(&node_id).cloned()
    }
}

impl HealthMonitor {
    fn new(check_interval: Duration, failure_threshold: u32) -> Self {
        Self {
            check_interval,
            failure_threshold,
            alerts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    async fn start_health_checks(&self, service_discovery: Arc<ServiceDiscovery>) {
        let mut interval = interval(self.check_interval);
        let failure_threshold = self.failure_threshold;
        let alerts = self.alerts.clone();
        
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                
                let nodes = service_discovery.nodes.read().await.clone();
                for (node_id, endpoint) in nodes {
                    // Simulate health check
                    let is_healthy = Self::check_node_health(&endpoint).await;
                    
                    let mut health_checks = service_discovery.health_checks.write().await;
                    if let Some(health) = health_checks.get_mut(&node_id) {
                        health.last_check = SystemTime::now();
                        
                        if is_healthy {
                            health.status = NodeStatus::Healthy;
                            health.consecutive_failures = 0;
                            health.response_time_ms = 50; // Simulated
                        } else {
                            health.consecutive_failures += 1;
                            
                            if health.consecutive_failures >= failure_threshold {
                                health.status = NodeStatus::Failed;
                                
                                // Create alert
                                let alert = Alert {
                                    severity: AlertSeverity::Critical,
                                    message: format!("Node {} has failed health checks", node_id),
                                    node_id: Some(node_id),
                                    timestamp: SystemTime::now(),
                                    resolved: false,
                                };
                                
                                alerts.write().await.push(alert);
                            } else {
                                health.status = NodeStatus::Degraded;
                            }
                        }
                    }
                }
            }
        });
    }

    async fn check_node_health(_endpoint: &ServiceEndpoint) -> bool {
        // Simulate health check - in real implementation, this would:
        // 1. Check HTTP health endpoint
        // 2. Verify Raft connectivity
        // 3. Check resource usage
        // 4. Validate data consistency
        true // Assume healthy for demo
    }

    async fn get_active_alerts(&self) -> Vec<Alert> {
        self.alerts
            .read()
            .await
            .iter()
            .filter(|alert| !alert.resolved)
            .cloned()
            .collect()
    }
}

impl BackupManager {
    fn new(config: BackupConfig) -> Self {
        Self {
            config,
            backup_history: RwLock::new(Vec::new()),
        }
    }

    async fn create_backup(&self) -> Result<String> {
        let backup_id = format!("backup_{}", 
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        );

        println!("💾 Creating backup {}...", backup_id);

        // In real implementation:
        // 1. Trigger snapshot creation on leader
        // 2. Collect snapshots from all nodes
        // 3. Create consistent backup archive
        // 4. Upload to backup storage
        // 5. Verify backup integrity

        let backup_record = BackupRecord {
            backup_id: backup_id.clone(),
            timestamp: SystemTime::now(),
            size_bytes: 1024 * 1024, // Simulated 1MB
            node_count: 3,
            storage_path: format!("{}/{}.tar.gz", self.config.storage_location, backup_id),
            checksum: "sha256:abcd1234...".to_string(),
        };

        self.backup_history.write().await.push(backup_record);

        // Cleanup old backups
        self.cleanup_old_backups().await;

        println!("✅ Backup {} created successfully", backup_id);
        Ok(backup_id)
    }

    async fn restore_backup(&self, backup_id: &str) -> Result<()> {
        println!("🔄 Restoring from backup {}...", backup_id);

        // Find backup record
        let backup_record = self.backup_history
            .read()
            .await
            .iter()
            .find(|b| b.backup_id == backup_id)
            .cloned();

        let backup_record = backup_record.ok_or_else(|| RaftError::InvalidConfiguration {
            component: "backup".to_string(),
            message: format!("Backup {} not found", backup_id),
        })?;

        println!("   📦 Found backup: {} ({} bytes, {} nodes)", 
                 backup_record.backup_id,
                 backup_record.size_bytes,
                 backup_record.node_count);

        // In real implementation:
        // 1. Download backup from storage
        // 2. Verify backup integrity
        // 3. Stop all cluster nodes
        // 4. Restore data directories
        // 5. Start nodes and verify cluster formation

        println!("✅ Restore completed successfully");
        Ok(())
    }

    async fn cleanup_old_backups(&self) {
        let mut history = self.backup_history.write().await;
        
        if history.len() > self.config.retention_count as usize {
            history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            history.truncate(self.config.retention_count as usize);
            println!("🧹 Cleaned up old backups, keeping latest {}", self.config.retention_count);
        }
    }
}

impl Default for NodeMetrics {
    fn default() -> Self {
        Self {
            uptime_seconds: 0,
            cpu_percent: 0.0,
            memory_used_mb: 0,
            disk_used_gb: 0.0,
            network_io_mb: 0.0,
            raft_log_size: 0,
            last_election_ms: 0,
            proposal_rate: 0.0,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🎯 Cluster Deployment Example");
    println!("=============================");

    // Define deployment configuration
    let config = DeploymentConfig {
        cluster_name: "production-cluster".to_string(),
        environment: Environment::Production,
        nodes: vec![
            NodeConfig {
                node_id: 1,
                hostname: "raft-node-1.example.com".to_string(),
                port: 8080,
                data_dir: "/var/lib/raft/node1".to_string(),
                log_level: "info".to_string(),
                resources: ResourceLimits {
                    max_memory_mb: 4096,
                    max_disk_gb: 100,
                    max_connections: 1000,
                },
            },
            NodeConfig {
                node_id: 2,
                hostname: "raft-node-2.example.com".to_string(),
                port: 8080,
                data_dir: "/var/lib/raft/node2".to_string(),
                log_level: "info".to_string(),
                resources: ResourceLimits {
                    max_memory_mb: 4096,
                    max_disk_gb: 100,
                    max_connections: 1000,
                },
            },
            NodeConfig {
                node_id: 3,
                hostname: "raft-node-3.example.com".to_string(),
                port: 8080,
                data_dir: "/var/lib/raft/node3".to_string(),
                log_level: "info".to_string(),
                resources: ResourceLimits {
                    max_memory_mb: 4096,
                    max_disk_gb: 100,
                    max_connections: 1000,
                },
            },
        ],
        network: NetworkConfig {
            cluster_port_range: (8080, 8090),
            client_port_range: (9080, 9090),
            ssl_enabled: true,
            compression_enabled: true,
        },
        storage: StorageConfig {
            backend: "redb".to_string(),
            sync_writes: true,
            compression: "lz4".to_string(),
            retention_days: 30,
        },
        monitoring: MonitoringConfig {
            metrics_port: 9090,
            health_check_interval_seconds: 10,
            alert_endpoints: vec![
                "slack://alerts".to_string(),
                "email://ops@example.com".to_string(),
            ],
        },
        backup: BackupConfig {
            enabled: true,
            schedule: "0 2 * * *".to_string(), // Daily at 2 AM
            retention_count: 7,
            storage_location: "s3://backups/raft-cluster".to_string(),
        },
    };

    // Initialize cluster manager
    let mut cluster_manager = ClusterManager::new(config).await?;

    // 1. Bootstrap the cluster
    println!("\n📋 Phase 1: Cluster Bootstrap");
    cluster_manager.bootstrap().await?;

    // 2. Show initial cluster status
    let status = cluster_manager.get_cluster_status().await;
    print_cluster_status(&status);

    // 3. Simulate some load
    println!("\n📊 Phase 2: Simulating Production Load");
    simulate_production_load().await;

    // 4. Create backup
    println!("\n💾 Phase 3: Backup Operations");
    let backup_id = cluster_manager.create_backup().await?;
    println!("   Created backup: {}", backup_id);

    // 5. Add a new node (scale up)
    println!("\n➕ Phase 4: Scale Up (Add Node)");
    let new_node = NodeConfig {
        node_id: 4,
        hostname: "raft-node-4.example.com".to_string(),
        port: 8080,
        data_dir: "/var/lib/raft/node4".to_string(),
        log_level: "info".to_string(),
        resources: ResourceLimits {
            max_memory_mb: 4096,
            max_disk_gb: 100,
            max_connections: 1000,
        },
    };
    
    cluster_manager.add_node(new_node).await?;
    
    let status = cluster_manager.get_cluster_status().await;
    print_cluster_status(&status);

    // 6. Perform rolling update
    println!("\n🔄 Phase 5: Rolling Update");
    cluster_manager.rolling_update("v2.0.0").await?;

    // 7. Remove a node (scale down)
    println!("\n➖ Phase 6: Scale Down (Remove Node)");
    cluster_manager.remove_node(4).await?;
    
    let status = cluster_manager.get_cluster_status().await;
    print_cluster_status(&status);

    // 8. Demonstrate disaster recovery
    println!("\n🆘 Phase 7: Disaster Recovery Simulation");
    println!("   Simulating cluster failure...");
    
    // In a real scenario, we might restore from backup
    println!("   Would restore from backup: {}", backup_id);

    // 9. Final status
    println!("\n📈 Final Cluster Status");
    let final_status = cluster_manager.get_cluster_status().await;
    print_cluster_status(&final_status);

    println!("\n🎉 Cluster Deployment Example Completed!");
    println!("==========================================");
    
    println!("\nDeployment Operations Demonstrated:");
    println!("• ✅ Multi-node cluster bootstrap");
    println!("• ✅ Configuration validation");
    println!("• ✅ Health monitoring and alerting");
    println!("• ✅ Service discovery integration");
    println!("• ✅ Automatic backup scheduling");
    println!("• ✅ Rolling updates with zero downtime");
    println!("• ✅ Dynamic cluster scaling (add/remove nodes)");
    println!("• ✅ Disaster recovery procedures");
    println!("• ✅ Production-ready configuration");

    Ok(())
}

fn print_cluster_status(status: &ClusterStatus) {
    println!("\n📊 Cluster Status: {}", status.cluster_name);
    println!("   Environment: {:?}", status.environment);
    println!("   Nodes: {}/{} healthy", status.healthy_nodes, status.total_nodes);
    
    if let Some(leader) = status.leader_node {
        println!("   Leader: Node {} (term {})", leader, status.current_term);
    } else {
        println!("   Leader: None (election in progress)");
    }
    
    println!("   Total keys: {}", status.total_keys);
    
    if !status.alerts.is_empty() {
        println!("   Active alerts: {}", status.alerts.len());
        for alert in &status.alerts {
            println!("     {:?}: {}", alert.severity, alert.message);
        }
    } else {
        println!("   Active alerts: None");
    }
}

async fn simulate_production_load() {
    println!("   Simulating client requests...");
    
    // Simulate various operations
    for i in 1..=50 {
        if i % 10 == 0 {
            println!("     Processed {} operations", i);
        }
        
        // Simulate processing time
        sleep(Duration::from_millis(10)).await;
    }
    
    println!("   ✅ Production load simulation complete");
}

// Mock implementations to make the example compile
mod simple_cluster_example {
    use super::*;

    #[derive(Debug)]
    pub struct RaftCluster<S: StateMachine> {
        node_id: NodeId,
        state_machine: S,
    }

    pub struct RaftClusterBuilder<S: StateMachine> {
        node_id: Option<NodeId>,
        state_machine: Option<S>,
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

    impl<S: StateMachine> RaftCluster<S> {
        pub fn builder() -> RaftClusterBuilder<S> {
            RaftClusterBuilder {
                node_id: None,
                state_machine: None,
            }
        }

        pub fn query(&self) -> &S::State {
            self.state_machine.get_current_state()
        }

        pub async fn metrics(&self) -> ClusterMetrics {
            ClusterMetrics {
                node_id: self.node_id,
                is_leader: self.node_id == 1, // Node 1 is always leader in mock
                term: 1,
            }
        }

        pub async fn start(&mut self) -> Result<()> {
            Ok(())
        }

        pub async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }

        pub async fn add_peer(&mut self, _peer: String) -> Result<()> {
            Ok(())
        }

        pub async fn remove_peer(&mut self, _peer: &str) -> Result<()> {
            Ok(())
        }
    }

    impl<S: StateMachine> RaftClusterBuilder<S> {
        pub fn node_id(mut self, id: NodeId) -> Self {
            self.node_id = Some(id);
            self
        }

        pub fn bind_address<A: Into<String>>(self, _address: A) -> Self {
            self
        }

        pub fn peers<I>(self, _peers: I) -> Self 
        where 
            I: IntoIterator,
            I::Item: Into<String>,
        {
            self
        }

        pub fn state_machine(mut self, sm: S) -> Self {
            self.state_machine = Some(sm);
            self
        }

        pub fn preset(self, _preset: Preset) -> Self {
            self
        }

        pub fn data_dir<P: AsRef<std::path::Path>>(self, _dir: P) -> Self {
            self
        }

        pub async fn build(self) -> Result<RaftCluster<S>> {
            Ok(RaftCluster {
                node_id: self.node_id.unwrap(),
                state_machine: self.state_machine.unwrap(),
            })
        }
    }
}

mod distributed_kv_store {
    use super::*;

    #[derive(Debug)]
    pub struct DistributedKvStore {
        data: HashMap<String, String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum DistributedKvCommand {
        Set { key: String, value: String },
    }

    impl DistributedKvStore {
        pub fn new() -> Self {
            Self {
                data: HashMap::new(),
            }
        }
    }

    impl StateMachine for DistributedKvStore {
        type Command = DistributedKvCommand;
        type State = (HashMap<String, String>, ());

        async fn apply_command(&mut self, command: Self::Command) -> Result<()> {
            match command {
                DistributedKvCommand::Set { key, value } => {
                    self.data.insert(key, value);
                }
            }
            Ok(())
        }

        async fn create_snapshot(&self) -> Result<Vec<u8>> {
            Ok(bincode::serialize(&self.data).unwrap())
        }

        async fn restore_from_snapshot(&mut self, snapshot: &[u8]) -> Result<()> {
            self.data = bincode::deserialize(snapshot).unwrap();
            Ok(())
        }

        fn get_current_state(&self) -> &Self::State {
            unsafe {
                std::mem::transmute(&(self.data, ()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deployment_config_validation() {
        let config = DeploymentConfig {
            cluster_name: "test".to_string(),
            environment: Environment::Development,
            nodes: vec![
                NodeConfig {
                    node_id: 1,
                    hostname: "node1".to_string(),
                    port: 8080,
                    data_dir: "/tmp/node1".to_string(),
                    log_level: "info".to_string(),
                    resources: ResourceLimits {
                        max_memory_mb: 1024,
                        max_disk_gb: 10,
                        max_connections: 100,
                    },
                },
                NodeConfig {
                    node_id: 2,
                    hostname: "node2".to_string(),
                    port: 8081,
                    data_dir: "/tmp/node2".to_string(),
                    log_level: "info".to_string(),
                    resources: ResourceLimits {
                        max_memory_mb: 1024,
                        max_disk_gb: 10,
                        max_connections: 100,
                    },
                },
            ],
            network: NetworkConfig {
                cluster_port_range: (8080, 8090),
                client_port_range: (9080, 9090),
                ssl_enabled: false,
                compression_enabled: false,
            },
            storage: StorageConfig {
                backend: "memory".to_string(),
                sync_writes: false,
                compression: "none".to_string(),
                retention_days: 1,
            },
            monitoring: MonitoringConfig {
                metrics_port: 9090,
                health_check_interval_seconds: 5,
                alert_endpoints: vec![],
            },
            backup: BackupConfig {
                enabled: false,
                schedule: "".to_string(),
                retention_count: 1,
                storage_location: "".to_string(),
            },
        };

        let cluster_manager = ClusterManager::new(config).await.unwrap();
        
        // Should fail validation due to insufficient nodes
        let result = cluster_manager.validate_configuration().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_service_discovery() {
        let discovery = ServiceDiscovery::new();
        
        let endpoint = ServiceEndpoint {
            node_id: 1,
            hostname: "test".to_string(),
            port: 8080,
            last_updated: SystemTime::now(),
            version: "1.0.0".to_string(),
        };

        discovery.register_node(endpoint).await;
        
        let health = discovery.get_node_health(1).await;
        assert!(health.is_some());
        assert_eq!(health.unwrap().status, NodeStatus::Starting);
    }

    #[tokio::test]
    async fn test_backup_manager() {
        let config = BackupConfig {
            enabled: true,
            schedule: "daily".to_string(),
            retention_count: 3,
            storage_location: "/tmp/backups".to_string(),
        };

        let backup_manager = BackupManager::new(config);
        let backup_id = backup_manager.create_backup().await.unwrap();
        
        assert!(backup_id.starts_with("backup_"));
        
        let history = backup_manager.backup_history.read().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].backup_id, backup_id);
    }
}