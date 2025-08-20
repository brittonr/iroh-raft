//! Administrative operations for Raft cluster management
//! 
//! This module provides high-level administrative operations for managing
//! Raft clusters, including health monitoring, maintenance operations,
//! and cluster information gathering.

use crate::cluster::{ClusterInfo, HealthStatus, SnapshotId};
use crate::error::Result;
use std::time::{Duration, SystemTime};

/// Administrative operations trait for advanced cluster management
/// 
/// This trait provides advanced operations for cluster administration,
/// monitoring, and maintenance. These operations are typically used by
/// operators and administrative tools rather than application code.
pub trait AdminOps {
    /// Perform a comprehensive health check of the cluster
    /// 
    /// Returns detailed health information about all cluster components
    /// including consensus, storage, networking, and state machine health.
    async fn health_check(&self) -> HealthStatus;
    
    /// Get detailed information about the cluster
    /// 
    /// Returns comprehensive cluster information including configuration,
    /// membership, performance statistics, and operational metrics.
    async fn cluster_info(&self) -> ClusterInfo;
    
    /// Take a snapshot of the current cluster state
    /// 
    /// Creates a point-in-time snapshot that can be used for:
    /// - Backup and recovery operations
    /// - Bootstrapping new cluster members
    /// - Debugging and analysis
    /// 
    /// Returns a snapshot identifier that can be used to reference this snapshot.
    async fn take_snapshot(&self) -> Result<SnapshotId>;
    
    /// Compact the Raft log, retaining entries up to the specified index
    /// 
    /// This operation helps manage storage space by removing old log entries
    /// that are no longer needed for normal cluster operations. Compaction
    /// is safe to perform as long as:
    /// 
    /// - The retain_index is not greater than the last applied index
    /// - All cluster members are up-to-date (or have snapshots)
    /// 
    /// # Parameters
    /// - `retain_index`: The log index up to which entries should be retained
    /// 
    /// # Safety
    /// This operation is safe but should be performed carefully in production.
    /// Ensure that all cluster members are caught up before compacting.
    async fn compact_log(&mut self, retain_index: u64) -> Result<()>;
}

/// Health check utilities for cluster monitoring
pub struct HealthChecker {
    /// Timeout for individual health checks
    pub check_timeout: Duration,
    /// Interval between health checks
    pub check_interval: Duration,
}

impl HealthChecker {
    /// Create a new health checker with default settings
    pub fn new() -> Self {
        Self {
            check_timeout: Duration::from_secs(5),
            check_interval: Duration::from_secs(30),
        }
    }
    
    /// Create a health checker with custom settings
    pub fn with_timeouts(check_timeout: Duration, check_interval: Duration) -> Self {
        Self {
            check_timeout,
            check_interval,
        }
    }
    
    /// Check if a timestamp is considered fresh
    pub fn is_timestamp_fresh(&self, timestamp: SystemTime, max_age: Duration) -> bool {
        SystemTime::now()
            .duration_since(timestamp)
            .map(|age| age <= max_age)
            .unwrap_or(false)
    }
    
    /// Determine overall health from component health statuses
    pub fn aggregate_health(&self, components: &[(String, crate::cluster::Health)]) -> crate::cluster::Health {
        use crate::cluster::Health;
        
        if components.is_empty() {
            return Health::Unhealthy;
        }
        
        let unhealthy_count = components.iter()
            .filter(|(_, health)| matches!(health, Health::Unhealthy))
            .count();
            
        let degraded_count = components.iter()
            .filter(|(_, health)| matches!(health, Health::Degraded))
            .count();
        
        // If any component is unhealthy, overall health is unhealthy
        if unhealthy_count > 0 {
            Health::Unhealthy
        } 
        // If any component is degraded, overall health is degraded
        else if degraded_count > 0 {
            Health::Degraded
        } 
        // All components are healthy
        else {
            Health::Healthy
        }
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Maintenance operations for cluster upkeep
pub struct MaintenanceOps {
    /// Whether to automatically compact logs based on thresholds
    pub auto_compact: bool,
    /// Log entry threshold for automatic compaction
    pub compact_threshold_entries: u64,
    /// Size threshold for automatic compaction (in bytes)
    pub compact_threshold_bytes: u64,
    /// Whether to automatically create snapshots
    pub auto_snapshot: bool,
    /// Entry threshold for automatic snapshots
    pub snapshot_threshold_entries: u64,
}

impl MaintenanceOps {
    /// Create maintenance operations with default settings
    pub fn new() -> Self {
        Self {
            auto_compact: true,
            compact_threshold_entries: 10000,
            compact_threshold_bytes: 100 * 1024 * 1024, // 100MB
            auto_snapshot: true,
            snapshot_threshold_entries: 50000,
        }
    }
    
    /// Create maintenance operations with custom settings
    pub fn with_thresholds(
        compact_threshold_entries: u64,
        compact_threshold_bytes: u64,
        snapshot_threshold_entries: u64,
    ) -> Self {
        Self {
            auto_compact: true,
            compact_threshold_entries,
            compact_threshold_bytes,
            auto_snapshot: true,
            snapshot_threshold_entries,
        }
    }
    
    /// Disable automatic maintenance operations
    pub fn disable_auto_maintenance(mut self) -> Self {
        self.auto_compact = false;
        self.auto_snapshot = false;
        self
    }
    
    /// Check if log compaction should be triggered
    pub fn should_compact(&self, current_entries: u64, current_size_bytes: u64) -> bool {
        self.auto_compact && (
            current_entries >= self.compact_threshold_entries ||
            current_size_bytes >= self.compact_threshold_bytes
        )
    }
    
    /// Check if snapshot creation should be triggered
    pub fn should_snapshot(&self, entries_since_snapshot: u64) -> bool {
        self.auto_snapshot && entries_since_snapshot >= self.snapshot_threshold_entries
    }
}

impl Default for MaintenanceOps {
    fn default() -> Self {
        Self::new()
    }
}

/// Cluster monitoring utilities
pub struct ClusterMonitor {
    /// Health checker for periodic health monitoring
    pub health_checker: HealthChecker,
    /// Maintenance operations configuration
    pub maintenance: MaintenanceOps,
}

impl ClusterMonitor {
    /// Create a new cluster monitor with default settings
    pub fn new() -> Self {
        Self {
            health_checker: HealthChecker::new(),
            maintenance: MaintenanceOps::new(),
        }
    }
    
    /// Create a cluster monitor with custom components
    pub fn with_components(health_checker: HealthChecker, maintenance: MaintenanceOps) -> Self {
        Self {
            health_checker,
            maintenance,
        }
    }
    
    /// Check if the cluster needs maintenance
    pub fn needs_maintenance(&self, entries: u64, size_bytes: u64, entries_since_snapshot: u64) -> MaintenanceNeeded {
        let compact_needed = self.maintenance.should_compact(entries, size_bytes);
        let snapshot_needed = self.maintenance.should_snapshot(entries_since_snapshot);
        
        MaintenanceNeeded {
            compact_log: compact_needed,
            take_snapshot: snapshot_needed,
        }
    }
}

impl Default for ClusterMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about what maintenance operations are needed
#[derive(Debug, Clone)]
pub struct MaintenanceNeeded {
    /// Whether log compaction is needed
    pub compact_log: bool,
    /// Whether a snapshot should be taken
    pub take_snapshot: bool,
}

impl MaintenanceNeeded {
    /// Check if any maintenance is needed
    pub fn any_needed(&self) -> bool {
        self.compact_log || self.take_snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::Health;
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_health_checker() {
        let checker = HealthChecker::new();
        
        // Test timestamp freshness
        let now = SystemTime::now();
        assert!(checker.is_timestamp_fresh(now, Duration::from_secs(60)));
        
        let old_time = now - Duration::from_secs(120);
        assert!(!checker.is_timestamp_fresh(old_time, Duration::from_secs(60)));
    }

    #[test]
    fn test_health_aggregation() {
        let checker = HealthChecker::new();
        
        // All healthy
        let components = vec![
            ("consensus".to_string(), Health::Healthy),
            ("storage".to_string(), Health::Healthy),
        ];
        assert!(matches!(checker.aggregate_health(&components), Health::Healthy));
        
        // One degraded
        let components = vec![
            ("consensus".to_string(), Health::Healthy),
            ("storage".to_string(), Health::Degraded),
        ];
        assert!(matches!(checker.aggregate_health(&components), Health::Degraded));
        
        // One unhealthy
        let components = vec![
            ("consensus".to_string(), Health::Healthy),
            ("storage".to_string(), Health::Unhealthy),
        ];
        assert!(matches!(checker.aggregate_health(&components), Health::Unhealthy));
        
        // Empty components
        let components = vec![];
        assert!(matches!(checker.aggregate_health(&components), Health::Unhealthy));
    }

    #[test]
    fn test_maintenance_ops() {
        let maintenance = MaintenanceOps::new();
        
        // Test compaction thresholds
        assert!(!maintenance.should_compact(5000, 50 * 1024 * 1024));
        assert!(maintenance.should_compact(15000, 50 * 1024 * 1024));
        assert!(maintenance.should_compact(5000, 150 * 1024 * 1024));
        
        // Test snapshot thresholds
        assert!(!maintenance.should_snapshot(25000));
        assert!(maintenance.should_snapshot(75000));
    }

    #[test]
    fn test_cluster_monitor() {
        let monitor = ClusterMonitor::new();
        
        let maintenance_needed = monitor.needs_maintenance(5000, 50 * 1024 * 1024, 25000);
        assert!(!maintenance_needed.any_needed());
        
        let maintenance_needed = monitor.needs_maintenance(15000, 150 * 1024 * 1024, 75000);
        assert!(maintenance_needed.any_needed());
        assert!(maintenance_needed.compact_log);
        assert!(maintenance_needed.take_snapshot);
    }

    #[test]
    fn test_maintenance_ops_custom() {
        let maintenance = MaintenanceOps::with_thresholds(1000, 10 * 1024 * 1024, 5000)
            .disable_auto_maintenance();
        
        assert!(!maintenance.auto_compact);
        assert!(!maintenance.auto_snapshot);
        assert!(!maintenance.should_compact(2000, 20 * 1024 * 1024));
        assert!(!maintenance.should_snapshot(10000));
    }
}
