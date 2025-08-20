//! Configuration for the management HTTP server
//!
//! This module provides configuration types for the management API server,
//! including network settings, security options, and feature toggles.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

/// Configuration for the management HTTP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementConfig {
    /// Address to bind the HTTP server to
    pub bind_address: SocketAddr,
    
    /// Whether to enable Swagger UI at /swagger-ui/
    pub enable_swagger_ui: bool,
    
    /// Whether to enable CORS headers
    pub enable_cors: bool,
    
    /// Maximum request body size in bytes
    pub max_request_size: usize,
    
    /// Request timeout duration
    pub request_timeout: Duration,
    
    /// Whether to enable detailed error responses
    pub detailed_errors: bool,
    
    /// API rate limiting configuration
    pub rate_limit: RateLimitConfig,
    
    /// Security configuration
    pub security: SecurityConfig,
    
    /// WebSocket configuration for event streaming
    pub websocket: WebSocketConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled
    pub enabled: bool,
    
    /// Maximum requests per minute per IP
    pub requests_per_minute: u32,
    
    /// Maximum concurrent connections per IP
    pub max_connections_per_ip: u32,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Whether to require authentication
    pub require_auth: bool,
    
    /// API key for authentication (if required)
    pub api_key: Option<String>,
    
    /// Whether to enable TLS
    pub enable_tls: bool,
    
    /// TLS certificate file path
    pub tls_cert_path: Option<String>,
    
    /// TLS private key file path
    pub tls_key_path: Option<String>,
}

/// WebSocket configuration for event streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// Whether WebSocket endpoints are enabled
    pub enabled: bool,
    
    /// Maximum WebSocket connections
    pub max_connections: u32,
    
    /// WebSocket message size limit
    pub max_message_size: usize,
    
    /// Ping interval for WebSocket connections
    pub ping_interval: Duration,
    
    /// Connection timeout for WebSocket
    pub connection_timeout: Duration,
}

impl Default for ManagementConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".parse().unwrap(),
            enable_swagger_ui: true,
            enable_cors: true,
            max_request_size: 1024 * 1024, // 1MB
            request_timeout: Duration::from_secs(30),
            detailed_errors: true,
            rate_limit: RateLimitConfig::default(),
            security: SecurityConfig::default(),
            websocket: WebSocketConfig::default(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 60,
            max_connections_per_ip: 10,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_auth: false,
            api_key: None,
            enable_tls: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_connections: 100,
            max_message_size: 64 * 1024, // 64KB
            ping_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(60),
        }
    }
}

/// Builder for ManagementConfig
#[derive(Debug, Default)]
pub struct ManagementConfigBuilder {
    config: ManagementConfig,
}

impl ManagementConfigBuilder {
    /// Create a new builder with default values
    pub fn new() -> Self {
        Self {
            config: ManagementConfig::default(),
        }
    }
    
    /// Set the bind address
    pub fn bind_address(mut self, addr: SocketAddr) -> Self {
        self.config.bind_address = addr;
        self
    }
    
    /// Enable or disable Swagger UI
    pub fn enable_swagger_ui(mut self, enable: bool) -> Self {
        self.config.enable_swagger_ui = enable;
        self
    }
    
    /// Enable or disable CORS
    pub fn enable_cors(mut self, enable: bool) -> Self {
        self.config.enable_cors = enable;
        self
    }
    
    /// Set maximum request size
    pub fn max_request_size(mut self, size: usize) -> Self {
        self.config.max_request_size = size;
        self
    }
    
    /// Set request timeout
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.config.request_timeout = timeout;
        self
    }
    
    /// Enable or disable detailed error responses
    pub fn detailed_errors(mut self, enable: bool) -> Self {
        self.config.detailed_errors = enable;
        self
    }
    
    /// Configure rate limiting
    pub fn rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.config.rate_limit = config;
        self
    }
    
    /// Configure security settings
    pub fn security(mut self, config: SecurityConfig) -> Self {
        self.config.security = config;
        self
    }
    
    /// Configure WebSocket settings
    pub fn websocket(mut self, config: WebSocketConfig) -> Self {
        self.config.websocket = config;
        self
    }
    
    /// Enable rate limiting with default settings
    pub fn enable_rate_limiting(mut self) -> Self {
        self.config.rate_limit.enabled = true;
        self
    }
    
    /// Set API key for authentication
    pub fn api_key(mut self, key: String) -> Self {
        self.config.security.api_key = Some(key);
        self.config.security.require_auth = true;
        self
    }
    
    /// Enable TLS with certificate paths
    pub fn enable_tls(mut self, cert_path: String, key_path: String) -> Self {
        self.config.security.enable_tls = true;
        self.config.security.tls_cert_path = Some(cert_path);
        self.config.security.tls_key_path = Some(key_path);
        self
    }
    
    /// Build the configuration
    pub fn build(self) -> ManagementConfig {
        self.config
    }
}

impl ManagementConfig {
    /// Create a new builder
    pub fn builder() -> ManagementConfigBuilder {
        ManagementConfigBuilder::new()
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), crate::error::RaftError> {
        // Validate TLS configuration
        if self.security.enable_tls {
            if self.security.tls_cert_path.is_none() {
                return Err(crate::error::RaftError::InvalidConfiguration {
                    component: "management".to_string(),
                    message: "TLS enabled but no certificate path provided".to_string(),
                    backtrace: snafu::Backtrace::new(),
                });
            }
            
            if self.security.tls_key_path.is_none() {
                return Err(crate::error::RaftError::InvalidConfiguration {
                    component: "management".to_string(),
                    message: "TLS enabled but no private key path provided".to_string(),
                    backtrace: snafu::Backtrace::new(),
                });
            }
        }
        
        // Validate rate limiting
        if self.rate_limit.enabled && self.rate_limit.requests_per_minute == 0 {
            return Err(crate::error::RaftError::InvalidConfiguration {
                component: "management".to_string(),
                message: "Rate limiting enabled but requests_per_minute is 0".to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }
        
        // Validate WebSocket configuration
        if self.websocket.enabled && self.websocket.max_connections == 0 {
            return Err(crate::error::RaftError::InvalidConfiguration {
                component: "management".to_string(),
                message: "WebSocket enabled but max_connections is 0".to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = ManagementConfig::default();
        assert!(config.enable_swagger_ui);
        assert!(config.enable_cors);
        assert!(!config.rate_limit.enabled);
        assert!(!config.security.require_auth);
        assert!(config.websocket.enabled);
    }
    
    #[test]
    fn test_builder() {
        let config = ManagementConfig::builder()
            .bind_address("0.0.0.0:9090".parse().unwrap())
            .enable_swagger_ui(false)
            .enable_cors(false)
            .api_key("test-key".to_string())
            .enable_rate_limiting()
            .build();
        
        assert_eq!(config.bind_address.port(), 9090);
        assert!(!config.enable_swagger_ui);
        assert!(!config.enable_cors);
        assert!(config.security.require_auth);
        assert_eq!(config.security.api_key, Some("test-key".to_string()));
        assert!(config.rate_limit.enabled);
    }
    
    #[test]
    fn test_validation() {
        // Valid config should pass
        let config = ManagementConfig::default();
        assert!(config.validate().is_ok());
        
        // Invalid TLS config should fail
        let mut config = ManagementConfig::default();
        config.security.enable_tls = true;
        assert!(config.validate().is_err());
        
        // Invalid rate limiting should fail
        let mut config = ManagementConfig::default();
        config.rate_limit.enabled = true;
        config.rate_limit.requests_per_minute = 0;
        assert!(config.validate().is_err());
    }
}