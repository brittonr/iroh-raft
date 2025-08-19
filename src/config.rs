//! Configuration management for iroh-raft
//!
//! This module provides a comprehensive configuration system for the iroh-raft library,
//! including support for builder pattern construction, environment variable overrides,
//! and validation.
//!
//! # Examples
//!
//! ```rust,no_run
//! use iroh_raft::config::{Config, ConfigBuilder};
//! use std::time::Duration;
//!
//! // Using builder pattern
//! let config = ConfigBuilder::new()
//!     .node_id(1)
//!     .data_dir("./data")
//!     .bind_address("127.0.0.1:8080")
//!     .add_peer("127.0.0.1:8081")
//!     .election_timeout(Duration::from_millis(5000))
//!     .heartbeat_interval(Duration::from_millis(1000))
//!     .build()
//!     .expect("Failed to build config");
//!
//! // Loading from environment (requires IROH_RAFT_NODE_ID to be set)
//! // let config = Config::from_env().expect("Failed to load config from environment");
//!
//! // Loading from file (requires config.toml to exist)
//! // let config = Config::from_file("config.toml").expect("Failed to load config from file");
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

/// Result type for configuration operations
pub type ConfigResult<T> = Result<T, ConfigError>;

/// Configuration error types
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Invalid configuration field value
    #[error("Invalid configuration: {field}: {message}")]
    Invalid { 
        /// The field that has invalid configuration
        field: String, 
        /// Description of what makes it invalid
        message: String 
    },
    
    /// Required configuration field is missing
    #[error("Missing required field: {field}")]
    Missing { 
        /// The name of the missing field
        field: String 
    },
    
    /// IO error occurred while reading/writing configuration
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Configuration serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::de::Error),
    
    /// Environment variable processing error
    #[error("Environment variable error: {0}")]
    Environment(String),
    
    /// Network address parsing error
    #[error("Address parse error: {0}")]
    AddressParse(#[from] std::net::AddrParseError),
}

/// Complete configuration for an iroh-raft node
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Node-specific configuration
    pub node: NodeConfig,
    
    /// Raft consensus algorithm settings
    pub raft: RaftConfig,
    
    /// Transport layer configuration
    pub transport: TransportConfig,
    
    /// Storage configuration
    pub storage: StorageConfig,
    
    /// Logging and observability
    pub observability: ObservabilityConfig,
}

/// Node-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeConfig {
    /// Unique node identifier (required)
    pub id: Option<u64>,
    
    /// Data directory for persistent storage
    pub data_dir: PathBuf,
    
    /// Enable debug mode
    pub debug: bool,
    
    /// Node metadata (optional)
    pub metadata: NodeMetadata,
}

/// Node metadata for identification and discovery
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NodeMetadata {
    /// Human-readable node name
    pub name: Option<String>,
    
    /// Node region/zone
    pub zone: Option<String>,
    
    /// Custom tags for the node
    pub tags: std::collections::HashMap<String, String>,
}

/// Raft consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RaftConfig {
    /// Election timeout range (randomized between election_timeout_min and election_timeout_max)
    #[serde(with = "humantime_serde")]
    pub election_timeout_min: Duration,
    
    /// Maximum election timeout (upper bound for randomization)
    #[serde(with = "humantime_serde")]
    pub election_timeout_max: Duration,
    
    /// Heartbeat interval (should be much smaller than election timeout)
    #[serde(with = "humantime_serde")]
    pub heartbeat_interval: Duration,
    
    /// Maximum number of uncommitted log entries
    pub max_uncommitted_entries: u64,
    
    /// Maximum number of inflight append entries messages
    pub max_inflight_msgs: usize,
    
    /// Log compaction configuration
    pub compaction: CompactionConfig,
    
    /// Snapshot configuration
    pub snapshot: SnapshotConfig,
    
    /// Configuration change timeout
    #[serde(with = "humantime_serde")]
    pub config_change_timeout: Duration,
    
    /// Whether to enable pre-vote to prevent disruptions
    pub pre_vote: bool,
    
    /// Whether to check quorum for read operations
    pub read_only_option: ReadOnlyOption,
}

/// Log compaction configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionConfig {
    /// Enable automatic log compaction
    pub enabled: bool,
    
    /// Number of applied entries after which to trigger compaction
    pub threshold_entries: u64,
    
    /// Size in bytes after which to trigger compaction
    pub threshold_bytes: u64,
    
    /// Interval between compaction checks
    #[serde(with = "humantime_serde")]
    pub check_interval: Duration,
    
    /// Number of entries to keep after compaction (for debugging)
    pub retain_entries: u64,
}

/// Snapshot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotConfig {
    /// Enable automatic snapshot creation
    pub enabled: bool,
    
    /// Number of applied entries after which to create a snapshot
    pub threshold_entries: u64,
    
    /// Size in bytes after which to create a snapshot
    pub threshold_bytes: u64,
    
    /// Compression level for snapshots (0-9, 0=disabled)
    pub compression_level: u32,
    
    /// Maximum time to wait for snapshot creation
    #[serde(with = "humantime_serde")]
    pub creation_timeout: Duration,
}

/// Read-only operation mode
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum ReadOnlyOption {
    /// No read-only operations (always go through Raft)
    #[default]
    Disabled,
    
    /// Allow read from leader without quorum check (fast but may return stale data)
    LeaderLease,
    
    /// Require quorum confirmation for all reads (slow but consistent)
    ReadIndex,
}

/// Transport layer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TransportConfig {
    /// Address to bind the Iroh endpoint
    pub bind_address: SocketAddr,
    
    /// List of initial peer addresses for cluster formation
    pub peers: Vec<SocketAddr>,
    
    /// Connection settings
    pub connection: ConnectionConfig,
    
    /// Iroh-specific settings
    pub iroh: IrohConfig,
}

/// Connection configuration for transport layer optimization
/// 
/// This configuration controls how the transport layer manages connections to peer nodes,
/// including connection pooling, timeouts, and resource limits.
/// 
/// # Connection Pooling
/// 
/// When `enable_pooling` is true, the transport layer will maintain a pool of connections
/// to each peer and reuse them for multiple messages. This reduces connection overhead
/// and improves throughput, especially for high-frequency communications.
/// 
/// # Resource Limits
/// 
/// The configuration provides several knobs to control resource usage:
/// - `max_connections_per_peer`: Limits memory and file descriptor usage
/// - `max_message_size`: Prevents DoS attacks with oversized messages
/// - `idle_timeout`: Automatically closes unused connections to free resources
/// 
/// # Example
/// 
/// ```rust
/// use iroh_raft::config::ConnectionConfig;
/// use std::time::Duration;
/// 
/// let connection_config = ConnectionConfig {
///     enable_pooling: true,
///     max_connections_per_peer: 5,
///     connect_timeout: Duration::from_secs(10),
///     keep_alive_interval: Duration::from_secs(30),
///     idle_timeout: Duration::from_secs(300),
///     max_message_size: 1024 * 1024, // 1MB
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionConfig {
    /// Timeout for establishing new connections to peers
    /// 
    /// If a connection attempt takes longer than this duration, it will be aborted
    /// and retried later. Lower values detect failed peers faster but may cause
    /// false positives on slow networks.
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,
    
    /// Interval for sending keep-alive messages on idle connections
    /// 
    /// Helps detect broken connections and maintain NAT traversal state.
    /// Should be significantly shorter than `idle_timeout`.
    #[serde(with = "humantime_serde")]
    pub keep_alive_interval: Duration,
    
    /// Maximum number of concurrent connections maintained per peer
    /// 
    /// When connection pooling is enabled, this limits the pool size per peer.
    /// Higher values allow better parallelism but consume more resources.
    /// Typical values: 1-10 depending on message volume.
    pub max_connections_per_peer: usize,
    
    /// Timeout for automatically closing idle connections
    /// 
    /// Connections with no activity for this duration will be closed to free
    /// resources. Set to a high value if connections are expensive to establish.
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,
    
    /// Maximum allowed size for incoming messages in bytes
    /// 
    /// Protects against DoS attacks with oversized messages. Should be large
    /// enough for legitimate Raft messages including snapshots.
    pub max_message_size: usize,
    
    /// Enable connection pooling for improved performance
    /// 
    /// When enabled, multiple messages to the same peer can reuse existing
    /// connections, reducing connection setup overhead. Recommended for
    /// production deployments with frequent message exchange.
    pub enable_pooling: bool,
}

/// Iroh-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IrohConfig {
    /// Relay server URL for NAT traversal
    pub relay_url: Option<String>,
    
    /// STUN servers for NAT traversal
    pub stun_servers: Vec<String>,
    
    /// Discovery configuration
    pub discovery: DiscoveryConfig,
    
    /// Custom ALPN protocols
    pub alpn_protocols: Vec<String>,
    
    /// Enable IPv6
    pub enable_ipv6: bool,
    
    /// Port mapping configuration for NAT traversal  
    pub port_mapping: PortMappingConfig,
}

/// Node discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscoveryConfig {
    /// Enable mDNS discovery
    pub enable_mdns: bool,
    
    /// Enable DHT-based discovery
    pub enable_dht: bool,
    
    /// Bootstrap nodes for DHT
    pub bootstrap_nodes: Vec<String>,
    
    /// Discovery interval
    #[serde(with = "humantime_serde")]
    pub discovery_interval: Duration,
}

/// Port mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PortMappingConfig {
    /// Enable UPnP port mapping
    pub enable_upnp: bool,
    
    /// Enable PCP (Port Control Protocol)
    pub enable_pcp: bool,
    
    /// Timeout for port mapping attempts
    #[serde(with = "humantime_serde")]
    pub mapping_timeout: Duration,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Storage backend type
    pub backend: StorageBackend,
    
    /// Sync strategy for durability
    pub sync_strategy: SyncStrategy,
    
    /// Maximum cache size in MB
    pub cache_size_mb: u64,
    
    /// Enable WAL (Write-Ahead Logging) for crash recovery
    pub enable_wal: bool,
    
    /// WAL sync interval
    #[serde(with = "humantime_serde")]
    pub wal_sync_interval: Duration,
    
    /// Database file permissions (octal)
    pub file_permissions: u32,
}

/// Storage backend options
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum StorageBackend {
    /// Embedded redb database (default)
    #[default]
    Redb,
    
    /// In-memory storage (for testing)
    Memory,
    
    /// Custom backend (with connection string)
    Custom { 
        /// Connection string for the custom storage backend
        connection_string: String 
    },
}

/// Sync strategy for durability vs performance tradeoff
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum SyncStrategy {
    /// No explicit sync (fastest, least durable)
    None,
    
    /// Sync on every write (slowest, most durable)
    Always,
    
    /// Periodic sync (balanced, default)
    #[default]
    Periodic,
    
    /// Sync on Raft commit only
    OnCommit,
}

/// Observability configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObservabilityConfig {
    /// Logging configuration
    pub logging: LoggingConfig,
    
    /// Metrics configuration
    pub metrics: MetricsConfig,
    
    /// Tracing configuration
    pub tracing: TracingConfig,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    
    /// Log format (json, pretty, compact)
    pub format: String,
    
    /// Enable structured logging
    pub structured: bool,
    
    /// Log output file (None for stdout)
    pub file: Option<PathBuf>,
    
    /// Maximum log file size in MB
    pub max_file_size_mb: u64,
    
    /// Number of log files to keep
    pub max_files: u32,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,
    
    /// Metrics server bind address
    pub bind_address: Option<SocketAddr>,
    
    /// Metrics collection interval
    #[serde(with = "humantime_serde")]
    pub collection_interval: Duration,
    
    /// Enable Prometheus metrics format
    pub prometheus: bool,
    
    /// Custom metrics prefix
    pub prefix: String,
}

/// Distributed tracing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TracingConfig {
    /// Enable distributed tracing
    pub enabled: bool,
    
    /// OTLP endpoint for trace export
    pub otlp_endpoint: Option<String>,
    
    /// Tracing sample rate (0.0 - 1.0)
    pub sample_rate: f64,
    
    /// Maximum number of spans per trace
    pub max_spans_per_trace: u32,
}

// Default implementations
impl Default for Config {
    fn default() -> Self {
        Self {
            node: NodeConfig::default(),
            raft: RaftConfig::default(),
            transport: TransportConfig::default(),
            storage: StorageConfig::default(),
            observability: ObservabilityConfig::default(),
        }
    }
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: None,
            data_dir: PathBuf::from("./data"),
            debug: false,
            metadata: NodeMetadata::default(),
        }
    }
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            election_timeout_min: Duration::from_millis(3000),
            election_timeout_max: Duration::from_millis(5000),
            heartbeat_interval: Duration::from_millis(1000),
            max_uncommitted_entries: 10000,
            max_inflight_msgs: 256,
            compaction: CompactionConfig::default(),
            snapshot: SnapshotConfig::default(),
            config_change_timeout: Duration::from_secs(30),
            pre_vote: true,
            read_only_option: ReadOnlyOption::default(),
        }
    }
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_entries: 10000,
            threshold_bytes: 100 * 1024 * 1024, // 100MB
            check_interval: Duration::from_secs(300), // 5 minutes
            retain_entries: 1000,
        }
    }
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_entries: 50000,
            threshold_bytes: 500 * 1024 * 1024, // 500MB
            compression_level: 3,
            creation_timeout: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:0".parse().expect("Valid default bind address"), // Random port
            peers: Vec::new(),
            connection: ConnectionConfig::default(),
            iroh: IrohConfig::default(),
        }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            keep_alive_interval: Duration::from_secs(30),
            max_connections_per_peer: 10,
            idle_timeout: Duration::from_secs(300), // 5 minutes
            max_message_size: 16 * 1024 * 1024, // 16MB
            enable_pooling: true,
        }
    }
}

impl Default for IrohConfig {
    fn default() -> Self {
        Self {
            relay_url: Some("https://relay.iroh.network".to_string()),
            stun_servers: vec![
                "stun.l.google.com:19302".to_string(),
                "stun1.l.google.com:19302".to_string(),
            ],
            discovery: DiscoveryConfig::default(),
            alpn_protocols: vec!["iroh-raft/1.0".to_string()],
            enable_ipv6: true,
            port_mapping: PortMappingConfig::default(),
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enable_mdns: true,
            enable_dht: false, // Disabled by default for controlled environments
            bootstrap_nodes: Vec::new(),
            discovery_interval: Duration::from_secs(30),
        }
    }
}

impl Default for PortMappingConfig {
    fn default() -> Self {
        Self {
            enable_upnp: true,
            enable_pcp: false,
            mapping_timeout: Duration::from_secs(30),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::default(),
            sync_strategy: SyncStrategy::default(),
            cache_size_mb: 128,
            enable_wal: true,
            wal_sync_interval: Duration::from_secs(1),
            file_permissions: 0o600, // Owner read/write only
        }
    }
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            logging: LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            tracing: TracingConfig::default(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "pretty".to_string(),
            structured: false,
            file: None,
            max_file_size_mb: 100,
            max_files: 10,
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_address: None, // Will use random port if enabled
            collection_interval: Duration::from_secs(15),
            prometheus: true,
            prefix: "iroh_raft".to_string(),
        }
    }
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            otlp_endpoint: None,
            sample_rate: 0.1, // 10% sampling
            max_spans_per_trace: 1000,
        }
    }
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;
        
        // Apply environment variable overrides
        config.apply_env_overrides()?;
        
        // Validate the configuration
        config.validate()?;
        
        Ok(config)
    }
    
    /// Load configuration from environment variables
    pub fn from_env() -> ConfigResult<Self> {
        let mut config = Config::default();
        config.apply_env_overrides()?;
        config.validate()?;
        Ok(config)
    }
    
    /// Apply environment variable overrides
    pub fn apply_env_overrides(&mut self) -> ConfigResult<()> {
        // Node configuration
        if let Ok(id) = std::env::var("IROH_RAFT_NODE_ID") {
            self.node.id = Some(id.parse().map_err(|e| ConfigError::Environment(
                format!("Invalid node ID: {}", e)
            ))?);
        }
        
        if let Ok(data_dir) = std::env::var("IROH_RAFT_DATA_DIR") {
            self.node.data_dir = PathBuf::from(data_dir);
        }
        
        if let Ok(debug) = std::env::var("IROH_RAFT_DEBUG") {
            self.node.debug = debug.parse().unwrap_or(false);
        }
        
        // Transport configuration
        if let Ok(bind_addr) = std::env::var("IROH_RAFT_BIND_ADDRESS") {
            self.transport.bind_address = bind_addr.parse()?;
        }
        
        if let Ok(peers) = std::env::var("IROH_RAFT_PEERS") {
            self.transport.peers = peers
                .split(',')
                .map(|peer| peer.trim().parse())
                .collect::<Result<Vec<_>, _>>()?;
        }
        
        // Raft configuration
        if let Ok(election_timeout) = std::env::var("IROH_RAFT_ELECTION_TIMEOUT_MS") {
            let timeout_ms: u64 = election_timeout.parse().map_err(|e| {
                ConfigError::Environment(format!("Invalid election timeout: {}", e))
            })?;
            self.raft.election_timeout_min = Duration::from_millis(timeout_ms);
            self.raft.election_timeout_max = Duration::from_millis(timeout_ms + 2000);
        }
        
        if let Ok(heartbeat) = std::env::var("IROH_RAFT_HEARTBEAT_INTERVAL_MS") {
            let heartbeat_ms: u64 = heartbeat.parse().map_err(|e| {
                ConfigError::Environment(format!("Invalid heartbeat interval: {}", e))
            })?;
            self.raft.heartbeat_interval = Duration::from_millis(heartbeat_ms);
        }
        
        // Observability configuration
        if let Ok(log_level) = std::env::var("IROH_RAFT_LOG_LEVEL") {
            self.observability.logging.level = log_level;
        }
        
        if let Ok(metrics_enabled) = std::env::var("IROH_RAFT_METRICS_ENABLED") {
            self.observability.metrics.enabled = metrics_enabled.parse().unwrap_or(true);
        }
        
        if let Ok(otlp_endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            self.observability.tracing.enabled = true;
            self.observability.tracing.otlp_endpoint = Some(otlp_endpoint);
        }
        
        Ok(())
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> ConfigResult<()> {
        // Validate node ID
        if self.node.id.is_none() {
            return Err(ConfigError::Missing {
                field: "node.id".to_string(),
            });
        }
        
        // Validate data directory
        if self.node.data_dir.as_os_str().is_empty() {
            return Err(ConfigError::Invalid {
                field: "node.data_dir".to_string(),
                message: "Data directory cannot be empty".to_string(),
            });
        }
        
        // Validate Raft timing constraints
        if self.raft.heartbeat_interval >= self.raft.election_timeout_min {
            return Err(ConfigError::Invalid {
                field: "raft.heartbeat_interval".to_string(),
                message: "Heartbeat interval must be less than election timeout".to_string(),
            });
        }
        
        if self.raft.election_timeout_min >= self.raft.election_timeout_max {
            return Err(ConfigError::Invalid {
                field: "raft.election_timeout".to_string(),
                message: "Election timeout min must be less than max".to_string(),
            });
        }
        
        // Validate peer addresses are unique
        let mut seen_peers = HashSet::new();
        for peer in &self.transport.peers {
            if !seen_peers.insert(peer) {
                return Err(ConfigError::Invalid {
                    field: "transport.peers".to_string(),
                    message: format!("Duplicate peer address: {}", peer),
                });
            }
        }
        
        // Validate log level
        match self.observability.logging.level.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            _ => {
                return Err(ConfigError::Invalid {
                    field: "observability.logging.level".to_string(),
                    message: format!("Invalid log level: {}", self.observability.logging.level),
                });
            }
        }
        
        // Validate tracing sample rate
        if !(0.0..=1.0).contains(&self.observability.tracing.sample_rate) {
            return Err(ConfigError::Invalid {
                field: "observability.tracing.sample_rate".to_string(),
                message: "Sample rate must be between 0.0 and 1.0".to_string(),
            });
        }
        
        Ok(())
    }
    
    /// Save configuration to a TOML file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> ConfigResult<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Environment(format!("Serialization failed: {}", e)))?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Configuration builder for fluent API construction
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Create a new configuration builder with default values
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }
    
    /// Set the node ID (required)
    pub fn node_id(mut self, id: u64) -> Self {
        self.config.node.id = Some(id);
        self
    }
    
    /// Set the data directory
    pub fn data_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.config.node.data_dir = dir.into();
        self
    }
    
    /// Enable debug mode
    pub fn debug(mut self, debug: bool) -> Self {
        self.config.node.debug = debug;
        self
    }
    
    /// Set node name (metadata)
    pub fn node_name<S: Into<String>>(mut self, name: S) -> Self {
        self.config.node.metadata.name = Some(name.into());
        self
    }
    
    /// Set node zone (metadata)
    pub fn node_zone<S: Into<String>>(mut self, zone: S) -> Self {
        self.config.node.metadata.zone = Some(zone.into());
        self
    }
    
    /// Add a node tag (metadata)
    pub fn add_node_tag<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.config.node.metadata.tags.insert(key.into(), value.into());
        self
    }
    
    /// Set bind address for transport
    pub fn bind_address<A: Into<String>>(mut self, addr: A) -> Self {
        let addr_str = addr.into();
        self.config.transport.bind_address = addr_str.parse().unwrap_or_else(|_| {
            "127.0.0.1:0".parse().expect("Valid default bind address")
        });
        self
    }
    
    /// Add a peer address
    pub fn add_peer<A: Into<String>>(mut self, addr: A) -> Self {
        let addr_str = addr.into();
        if let Ok(socket_addr) = addr_str.parse() {
            self.config.transport.peers.push(socket_addr);
        }
        self
    }
    
    /// Set multiple peer addresses
    pub fn peers(mut self, peers: Vec<String>) -> Self {
        self.config.transport.peers = peers
            .into_iter()
            .filter_map(|addr| addr.parse().ok())
            .collect();
        self
    }
    
    /// Set election timeout range
    pub fn election_timeout(mut self, min: Duration) -> Self {
        self.config.raft.election_timeout_min = min;
        self.config.raft.election_timeout_max = min + Duration::from_millis(2000);
        self
    }
    
    /// Set heartbeat interval
    pub fn heartbeat_interval(mut self, interval: Duration) -> Self {
        self.config.raft.heartbeat_interval = interval;
        self
    }
    
    /// Enable or disable pre-vote
    pub fn pre_vote(mut self, enabled: bool) -> Self {
        self.config.raft.pre_vote = enabled;
        self
    }
    
    /// Set read-only option
    pub fn read_only_option(mut self, option: ReadOnlyOption) -> Self {
        self.config.raft.read_only_option = option;
        self
    }
    
    /// Configure log compaction
    pub fn log_compaction(mut self, enabled: bool, threshold_entries: u64) -> Self {
        self.config.raft.compaction.enabled = enabled;
        self.config.raft.compaction.threshold_entries = threshold_entries;
        self
    }
    
    /// Configure snapshots
    pub fn snapshots(mut self, enabled: bool, threshold_entries: u64, compression_level: u32) -> Self {
        self.config.raft.snapshot.enabled = enabled;
        self.config.raft.snapshot.threshold_entries = threshold_entries;
        self.config.raft.snapshot.compression_level = compression_level;
        self
    }
    
    /// Set storage backend
    pub fn storage_backend(mut self, backend: StorageBackend) -> Self {
        self.config.storage.backend = backend;
        self
    }
    
    /// Set storage sync strategy
    pub fn sync_strategy(mut self, strategy: SyncStrategy) -> Self {
        self.config.storage.sync_strategy = strategy;
        self
    }
    
    /// Set cache size in MB
    pub fn cache_size_mb(mut self, size_mb: u64) -> Self {
        self.config.storage.cache_size_mb = size_mb;
        self
    }
    
    /// Set log level
    pub fn log_level<S: Into<String>>(mut self, level: S) -> Self {
        self.config.observability.logging.level = level.into();
        self
    }
    
    /// Enable or disable metrics
    pub fn metrics(mut self, enabled: bool) -> Self {
        self.config.observability.metrics.enabled = enabled;
        self
    }
    
    /// Set metrics bind address
    pub fn metrics_address<A: Into<String>>(mut self, addr: A) -> Self {
        if let Ok(socket_addr) = addr.into().parse() {
            self.config.observability.metrics.bind_address = Some(socket_addr);
        }
        self
    }
    
    /// Enable distributed tracing with OTLP endpoint
    pub fn tracing<S: Into<String>>(mut self, otlp_endpoint: S) -> Self {
        self.config.observability.tracing.enabled = true;
        self.config.observability.tracing.otlp_endpoint = Some(otlp_endpoint.into());
        self
    }
    
    /// Set tracing sample rate
    pub fn tracing_sample_rate(mut self, rate: f64) -> Self {
        self.config.observability.tracing.sample_rate = rate;
        self
    }
    
    /// Set relay server for NAT traversal
    pub fn relay_server<S: Into<String>>(mut self, url: S) -> Self {
        self.config.transport.iroh.relay_url = Some(url.into());
        self
    }
    
    /// Enable mDNS discovery
    pub fn enable_mdns_discovery(mut self, enabled: bool) -> Self {
        self.config.transport.iroh.discovery.enable_mdns = enabled;
        self
    }
    
    /// Build the final configuration
    pub fn build(self) -> ConfigResult<Config> {
        self.config.validate()?;
        Ok(self.config)
    }
    
    /// Build without validation (useful for testing)
    pub fn build_unchecked(self) -> Config {
        self.config
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FromStr for ReadOnlyOption {
    type Err = ConfigError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disabled" => Ok(ReadOnlyOption::Disabled),
            "leader_lease" => Ok(ReadOnlyOption::LeaderLease),
            "read_index" => Ok(ReadOnlyOption::ReadIndex),
            _ => Err(ConfigError::Invalid {
                field: "read_only_option".to_string(),
                message: format!("Invalid read-only option: {}", s),
            }),
        }
    }
}

impl FromStr for StorageBackend {
    type Err = ConfigError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "redb" => Ok(StorageBackend::Redb),
            "memory" => Ok(StorageBackend::Memory),
            _ => Ok(StorageBackend::Custom { 
                connection_string: s.to_string() 
            }),
        }
    }
}

impl FromStr for SyncStrategy {
    type Err = ConfigError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(SyncStrategy::None),
            "always" => Ok(SyncStrategy::Always),
            "periodic" => Ok(SyncStrategy::Periodic),
            "on_commit" => Ok(SyncStrategy::OnCommit),
            _ => Err(ConfigError::Invalid {
                field: "sync_strategy".to_string(),
                message: format!("Invalid sync strategy: {}", s),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    #[test]
    fn test_default_config() {
        let config = Config::default();
        
        // Basic validation should pass
        assert!(config.validate().is_err()); // Should fail due to missing node ID
        
        // Check default values
        assert_eq!(config.raft.heartbeat_interval, Duration::from_millis(1000));
        assert_eq!(config.raft.election_timeout_min, Duration::from_millis(3000));
        assert!(config.raft.pre_vote);
        assert!(config.raft.compaction.enabled);
        assert!(config.raft.snapshot.enabled);
        assert_eq!(config.observability.logging.level, "info");
        assert!(config.observability.metrics.enabled);
    }
    
    #[test]
    fn test_config_builder() {
        let config = ConfigBuilder::new()
            .node_id(1)
            .data_dir("/tmp/test")
            .bind_address("127.0.0.1:8080")
            .add_peer("127.0.0.1:8081")
            .election_timeout(Duration::from_millis(5000))
            .heartbeat_interval(Duration::from_millis(1000))
            .log_level("debug")
            .build()
            .expect("Failed to build config");
        
        assert_eq!(config.node.id, Some(1));
        assert_eq!(config.node.data_dir, PathBuf::from("/tmp/test"));
        assert_eq!(config.transport.bind_address, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.transport.peers.len(), 1);
        assert_eq!(config.raft.election_timeout_min, Duration::from_millis(5000));
        assert_eq!(config.raft.heartbeat_interval, Duration::from_millis(1000));
        assert_eq!(config.observability.logging.level, "debug");
    }
    
    #[test]
    fn test_validation() {
        // Test missing node ID
        let config = Config::default();
        assert!(config.validate().is_err());
        
        // Test invalid timing
        let mut config = Config::default();
        config.node.id = Some(1);
        config.raft.heartbeat_interval = Duration::from_millis(5000);
        config.raft.election_timeout_min = Duration::from_millis(3000);
        assert!(config.validate().is_err());
        
        // Test valid config
        let mut config = Config::default();
        config.node.id = Some(1);
        config.raft.heartbeat_interval = Duration::from_millis(1000);
        config.raft.election_timeout_min = Duration::from_millis(5000);
        config.raft.election_timeout_max = Duration::from_millis(7000);
        assert!(config.validate().is_ok());
    }
    
    #[test]
    fn test_env_overrides() {
        std::env::set_var("IROH_RAFT_NODE_ID", "42");
        std::env::set_var("IROH_RAFT_DATA_DIR", "/custom/data");
        std::env::set_var("IROH_RAFT_LOG_LEVEL", "trace");
        
        let config = Config::from_env().expect("Failed to load config from env");
        
        assert_eq!(config.node.id, Some(42));
        assert_eq!(config.node.data_dir, PathBuf::from("/custom/data"));
        assert_eq!(config.observability.logging.level, "trace");
        
        // Clean up
        std::env::remove_var("IROH_RAFT_NODE_ID");
        std::env::remove_var("IROH_RAFT_DATA_DIR");
        std::env::remove_var("IROH_RAFT_LOG_LEVEL");
    }
}