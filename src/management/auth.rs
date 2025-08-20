//! Authentication and authorization for iroh-raft management
//!
//! This module provides P2P authentication and role-based access control
//! for management operations, leveraging Iroh's built-in security features.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use iroh::NodeId;

use crate::error::{RaftError, Result};
use super::{ManagementOperation, ManagementRole};

/// Authentication token for management operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    /// Node ID of the authenticated entity
    pub node_id: u64,
    /// Iroh peer ID for P2P authentication
    pub peer_id: NodeId,
    /// Role assigned to this token
    pub role: ManagementRole,
    /// Token expiration time
    pub expires_at: SystemTime,
    /// Token issuer
    pub issued_by: u64,
    /// Token signature (simplified for demo)
    pub signature: String,
}

/// Permission set for a specific role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    /// Operations allowed for this permission
    pub allowed_operations: Vec<String>,
    /// Time-based restrictions (optional)
    pub time_restrictions: Option<TimeRestriction>,
    /// Resource-based restrictions (optional)
    pub resource_restrictions: Option<ResourceRestriction>,
}

/// Time-based access restrictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRestriction {
    /// Valid from time
    pub valid_from: SystemTime,
    /// Valid until time
    pub valid_until: SystemTime,
    /// Maximum session duration
    pub max_session_duration: Duration,
}

/// Resource-based access restrictions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRestriction {
    /// Allowed node IDs (empty means all)
    pub allowed_nodes: Vec<u64>,
    /// Denied node IDs
    pub denied_nodes: Vec<u64>,
    /// Allowed operations by resource type
    pub operation_limits: HashMap<String, u32>,
}

/// Authentication and authorization manager
#[derive(Debug)]
pub struct AuthManager {
    /// Role definitions and permissions
    role_permissions: HashMap<ManagementRole, Permission>,
    /// Active tokens by node ID
    active_tokens: HashMap<u64, AuthToken>,
    /// Trusted issuers for token validation
    trusted_issuers: Vec<u64>,
    /// Default token lifetime
    default_token_lifetime: Duration,
}

impl AuthManager {
    /// Create a new authentication manager
    pub fn new() -> Self {
        let mut manager = Self {
            role_permissions: HashMap::new(),
            active_tokens: HashMap::new(),
            trusted_issuers: Vec::new(),
            default_token_lifetime: Duration::from_secs(8 * 60 * 60), // 8 hours
        };
        
        // Set up default role permissions
        manager.setup_default_permissions();
        manager
    }

    /// Set up default role permissions
    fn setup_default_permissions(&mut self) {
        // Admin role - full access
        let admin_permission = Permission {
            allowed_operations: vec!["*".to_string()], // Wildcard for all operations
            time_restrictions: None,
            resource_restrictions: None,
        };
        self.role_permissions.insert(ManagementRole::Admin, admin_permission);

        // Observer role - read-only access
        let observer_permission = Permission {
            allowed_operations: vec![
                "get_cluster_status".to_string(),
                "get_configuration".to_string(),
                "get_metrics".to_string(),
                "get_health_status".to_string(),
                "get_log_info".to_string(),
                "get_log_entries".to_string(),
                "get_snapshot_info".to_string(),
                "list_backups".to_string(),
            ],
            time_restrictions: None,
            resource_restrictions: None,
        };
        self.role_permissions.insert(ManagementRole::Observer, observer_permission);

        // Member manager role - member operations only
        let member_manager_permission = Permission {
            allowed_operations: vec![
                // Read operations
                "get_cluster_status".to_string(),
                "get_configuration".to_string(),
                "get_health_status".to_string(),
                // Member management operations
                "add_member".to_string(),
                "remove_member".to_string(),
                "update_member_role".to_string(),
            ],
            time_restrictions: None,
            resource_restrictions: None,
        };
        self.role_permissions.insert(ManagementRole::MemberManager, member_manager_permission);

        // Monitor role - metrics and health only
        let monitor_permission = Permission {
            allowed_operations: vec![
                "get_cluster_status".to_string(),
                "get_metrics".to_string(),
                "get_health_status".to_string(),
            ],
            time_restrictions: None,
            resource_restrictions: None,
        };
        self.role_permissions.insert(ManagementRole::Monitor, monitor_permission);
    }

    /// Add a trusted issuer
    pub fn add_trusted_issuer(&mut self, issuer_node_id: u64) {
        if !self.trusted_issuers.contains(&issuer_node_id) {
            self.trusted_issuers.push(issuer_node_id);
        }
    }

    /// Remove a trusted issuer
    pub fn remove_trusted_issuer(&mut self, issuer_node_id: u64) {
        self.trusted_issuers.retain(|&id| id != issuer_node_id);
    }

    /// Issue a new authentication token
    pub fn issue_token(
        &mut self,
        node_id: u64,
        peer_id: NodeId,
        role: ManagementRole,
        issuer_node_id: u64,
        duration: Option<Duration>,
    ) -> Result<AuthToken> {
        // Verify the issuer is trusted
        if !self.trusted_issuers.contains(&issuer_node_id) {
            return Err(RaftError::AuthorizationDenied {
                operation: "issue_token".to_string(),
                peer_id: issuer_node_id.to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        let expires_at = SystemTime::now() + duration.unwrap_or(self.default_token_lifetime);
        
        // Generate signature (simplified - in production, use proper cryptographic signatures)
        let signature = format!("sig_{}_{}_{}_{}", 
            node_id, 
            role as u32,
            expires_at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            issuer_node_id
        );

        let token = AuthToken {
            node_id,
            peer_id,
            role,
            expires_at,
            issued_by: issuer_node_id,
            signature,
        };

        // Store the active token
        self.active_tokens.insert(node_id, token.clone());

        Ok(token)
    }

    /// Validate an authentication token
    pub fn validate_token(&self, token: &AuthToken) -> Result<()> {
        // Check if token is expired
        if SystemTime::now() > token.expires_at {
            return Err(RaftError::AuthorizationDenied {
                operation: "validate_token".to_string(),
                peer_id: token.node_id.to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        // Check if issuer is trusted
        if !self.trusted_issuers.contains(&token.issued_by) {
            return Err(RaftError::AuthorizationDenied {
                operation: "validate_token".to_string(),
                peer_id: token.issued_by.to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        // Verify signature (simplified verification)
        let expected_signature = format!("sig_{}_{}_{}_{}", 
            token.node_id, 
            token.role as u32,
            token.expires_at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            token.issued_by
        );

        if token.signature != expected_signature {
            return Err(RaftError::AuthorizationDenied {
                operation: "validate_token".to_string(),
                peer_id: token.node_id.to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        Ok(())
    }

    /// Check if a token has permission for a specific operation
    pub fn check_permission(&self, token: &AuthToken, operation: &ManagementOperation) -> Result<()> {
        // First validate the token
        self.validate_token(token)?;

        // Get the permission for this role
        let permission = self.role_permissions.get(&token.role)
            .ok_or_else(|| RaftError::AuthorizationDenied {
                operation: "check_permission".to_string(),
                peer_id: token.node_id.to_string(),
                backtrace: snafu::Backtrace::new(),
            })?;

        // Check if operation is allowed
        let operation_name = operation_to_string(operation);
        let is_allowed = permission.allowed_operations.contains(&"*".to_string()) ||
                        permission.allowed_operations.contains(&operation_name);

        if !is_allowed {
            return Err(RaftError::AuthorizationDenied {
                operation: operation_name,
                peer_id: token.node_id.to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        // Check time restrictions
        if let Some(time_restriction) = &permission.time_restrictions {
            let now = SystemTime::now();
            if now < time_restriction.valid_from || now > time_restriction.valid_until {
                return Err(RaftError::AuthorizationDenied {
                    operation: operation_name,
                    peer_id: token.node_id.to_string(),
                    backtrace: snafu::Backtrace::new(),
                });
            }
        }

        // Check resource restrictions
        if let Some(resource_restriction) = &permission.resource_restrictions {
            // Check node access restrictions
            if !resource_restriction.allowed_nodes.is_empty() {
                if let Some(target_node) = operation_target_node(operation) {
                    if !resource_restriction.allowed_nodes.contains(&target_node) {
                        return Err(RaftError::AuthorizationDenied {
                            operation: operation_name,
                            peer_id: token.node_id.to_string(),
                            backtrace: snafu::Backtrace::new(),
                        });
                    }
                }
            }

            if let Some(target_node) = operation_target_node(operation) {
                if resource_restriction.denied_nodes.contains(&target_node) {
                    return Err(RaftError::AuthorizationDenied {
                        operation: operation_name,
                        peer_id: token.node_id.to_string(),
                        backtrace: snafu::Backtrace::new(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Revoke a token
    pub fn revoke_token(&mut self, node_id: u64) {
        self.active_tokens.remove(&node_id);
    }

    /// Clean up expired tokens
    pub fn cleanup_expired_tokens(&mut self) {
        let now = SystemTime::now();
        self.active_tokens.retain(|_, token| token.expires_at > now);
    }

    /// Get active token for a node
    pub fn get_active_token(&self, node_id: u64) -> Option<&AuthToken> {
        self.active_tokens.get(&node_id)
    }

    /// List all active tokens
    pub fn list_active_tokens(&self) -> Vec<&AuthToken> {
        self.active_tokens.values().collect()
    }

    /// Update role permissions
    pub fn update_role_permission(&mut self, role: ManagementRole, permission: Permission) {
        self.role_permissions.insert(role, permission);
    }

    /// Get role permission
    pub fn get_role_permission(&self, role: &ManagementRole) -> Option<&Permission> {
        self.role_permissions.get(role)
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert management operation to string for permission checking
fn operation_to_string(operation: &ManagementOperation) -> String {
    match operation {
        ManagementOperation::GetClusterStatus => "get_cluster_status".to_string(),
        ManagementOperation::AddMember { .. } => "add_member".to_string(),
        ManagementOperation::RemoveMember { .. } => "remove_member".to_string(),
        ManagementOperation::UpdateMemberRole { .. } => "update_member_role".to_string(),
        ManagementOperation::TransferLeadership { .. } => "transfer_leadership".to_string(),
        ManagementOperation::StepDown => "step_down".to_string(),
        ManagementOperation::GetConfiguration => "get_configuration".to_string(),
        ManagementOperation::UpdateConfiguration { .. } => "update_configuration".to_string(),
        ManagementOperation::GetMetrics { .. } => "get_metrics".to_string(),
        ManagementOperation::GetHealthStatus => "get_health_status".to_string(),
        ManagementOperation::GetLogInfo => "get_log_info".to_string(),
        ManagementOperation::CompactLog { .. } => "compact_log".to_string(),
        ManagementOperation::GetLogEntries { .. } => "get_log_entries".to_string(),
        ManagementOperation::TriggerSnapshot => "trigger_snapshot".to_string(),
        ManagementOperation::GetSnapshotInfo => "get_snapshot_info".to_string(),
        ManagementOperation::CreateBackup { .. } => "create_backup".to_string(),
        ManagementOperation::RestoreBackup { .. } => "restore_backup".to_string(),
        ManagementOperation::ListBackups => "list_backups".to_string(),
    }
}

/// Extract target node ID from operation (if applicable)
fn operation_target_node(operation: &ManagementOperation) -> Option<u64> {
    match operation {
        ManagementOperation::AddMember { node_id, .. } => Some(*node_id),
        ManagementOperation::RemoveMember { node_id } => Some(*node_id),
        ManagementOperation::UpdateMemberRole { node_id, .. } => Some(*node_id),
        ManagementOperation::TransferLeadership { target_node } => *target_node,
        _ => None,
    }
}

/// Authentication result
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub token: AuthToken,
    pub permissions: Permission,
}

/// Simple authentication helper for Iroh P2P connections
pub struct IrohAuth {
    auth_manager: AuthManager,
    node_whitelist: Vec<NodeId>,
}

impl IrohAuth {
    /// Create a new Iroh authentication helper
    pub fn new(auth_manager: AuthManager) -> Self {
        Self {
            auth_manager,
            node_whitelist: Vec::new(),
        }
    }

    /// Add a node to the whitelist (allowed to connect)
    pub fn add_to_whitelist(&mut self, node_id: NodeId) {
        if !self.node_whitelist.contains(&node_id) {
            self.node_whitelist.push(node_id);
        }
    }

    /// Remove a node from the whitelist
    pub fn remove_from_whitelist(&mut self, node_id: &NodeId) {
        self.node_whitelist.retain(|id| id != node_id);
    }

    /// Check if a node is allowed to connect
    pub fn is_node_allowed(&self, node_id: &NodeId) -> bool {
        self.node_whitelist.is_empty() || self.node_whitelist.contains(node_id)
    }

    /// Authenticate a connection using Iroh's peer information
    pub fn authenticate_connection(
        &self,
        peer_id: NodeId,
        node_id: u64,
    ) -> Result<Option<AuthToken>> {
        // Check if the node is in whitelist
        if !self.is_node_allowed(&peer_id) {
            return Err(RaftError::AuthorizationDenied {
                operation: "authenticate_connection".to_string(),
                peer_id: peer_id.to_string(),
                backtrace: snafu::Backtrace::new(),
            });
        }

        // Get active token for the node
        if let Some(token) = self.auth_manager.get_active_token(node_id) {
            // Verify the peer ID matches
            if token.peer_id == peer_id {
                self.auth_manager.validate_token(token)?;
                return Ok(Some(token.clone()));
            }
        }

        // No valid token found
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_auth_manager_creation() {
        let auth_manager = AuthManager::new();
        
        // Check that default permissions are set up
        assert!(auth_manager.role_permissions.contains_key(&ManagementRole::Admin));
        assert!(auth_manager.role_permissions.contains_key(&ManagementRole::Observer));
        assert!(auth_manager.role_permissions.contains_key(&ManagementRole::MemberManager));
        assert!(auth_manager.role_permissions.contains_key(&ManagementRole::Monitor));
    }

    #[test]
    fn test_token_issuance_and_validation() {
        let mut auth_manager = AuthManager::new();
        auth_manager.add_trusted_issuer(1);

        let peer_id = NodeId::from_str("k51qzi5uqu5dk5k12mcq2c8nnfa4cvz5hv0xlwxsltwf8q1vz6q3zlj4e8b40n6n1")
            .unwrap();

        // Issue a token
        let token = auth_manager.issue_token(
            100,
            peer_id,
            ManagementRole::Admin,
            1,
            Some(Duration::from_secs(3600)),
        ).unwrap();

        // Validate the token
        assert!(auth_manager.validate_token(&token).is_ok());

        // Check permission for admin operation
        let operation = ManagementOperation::GetClusterStatus;
        assert!(auth_manager.check_permission(&token, &operation).is_ok());
    }

    #[test]
    fn test_permission_checking() {
        let mut auth_manager = AuthManager::new();
        auth_manager.add_trusted_issuer(1);

        let peer_id = NodeId::from_str("k51qzi5uqu5dk5k12mcq2c8nnfa4cvz5hv0xlwxsltwf8q1vz6q3zlj4e8b40n6n1")
            .unwrap();

        // Issue an observer token
        let observer_token = auth_manager.issue_token(
            200,
            peer_id,
            ManagementRole::Observer,
            1,
            Some(Duration::from_secs(3600)),
        ).unwrap();

        // Observer should be able to read cluster status
        let read_operation = ManagementOperation::GetClusterStatus;
        assert!(auth_manager.check_permission(&observer_token, &read_operation).is_ok());

        // Observer should NOT be able to add members
        let write_operation = ManagementOperation::AddMember {
            node_id: 999,
            address: "test".to_string(),
            role: super::super::MemberRole::Voter,
        };
        assert!(auth_manager.check_permission(&observer_token, &write_operation).is_err());
    }

    #[test]
    fn test_token_expiration() {
        let mut auth_manager = AuthManager::new();
        auth_manager.add_trusted_issuer(1);

        let peer_id = NodeId::from_str("k51qzi5uqu5dk5k12mcq2c8nnfa4cvz5hv0xlwxsltwf8q1vz6q3zlj4e8b40n6n1")
            .unwrap();

        // Issue a token that expires immediately
        let mut token = auth_manager.issue_token(
            300,
            peer_id,
            ManagementRole::Admin,
            1,
            Some(Duration::from_nanos(1)),
        ).unwrap();

        // Make sure it's expired
        std::thread::sleep(Duration::from_millis(10));
        token.expires_at = SystemTime::now() - Duration::from_secs(1);

        // Validation should fail
        assert!(auth_manager.validate_token(&token).is_err());
    }
}