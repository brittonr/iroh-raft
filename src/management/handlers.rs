//! REST endpoint handlers for the management API
//!
//! This module implements the HTTP handlers for all management API endpoints,
//! including cluster status, member management, leadership operations, and health checks.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tracing::{debug, error, info, warn};
use utoipa::IntoParams;

use super::schemas::{
    AddMemberRequest, ClusterConfiguration, ClusterHealth, ClusterMember, ClusterStatus,
    ErrorResponse, HealthCheck, HealthResponse, HealthStatus, MemberOperationResponse,
    MetricsResponse, NodeRole, NodeState, ReadinessCheck, ReadinessResponse,
    RemoveMemberRequest, TransferLeadershipRequest,
};

/// Placeholder for cluster handle - in real implementation this would be the actual cluster interface
#[derive(Debug, Clone)]
pub struct ClusterHandle {
    // This would contain the actual Raft cluster state and operations
    _phantom: std::marker::PhantomData<()>,
}

impl ClusterHandle {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Application state shared across handlers
#[derive(Debug, Clone)]
pub struct AppState {
    /// Handle to the Raft cluster
    pub cluster: Arc<ClusterHandle>,
    
    /// Service start time for uptime calculation
    pub start_time: SystemTime,
    
    /// Optional event bus for notifications
    pub event_bus: Option<Arc<super::events::EventBus>>,
}

/// Query parameters for cluster status
#[derive(Debug, Deserialize, IntoParams)]
pub struct ClusterStatusQuery {
    /// Include detailed member information
    #[serde(default)]
    pub detailed: bool,
    
    /// Include health check details
    #[serde(default)]
    pub include_health: bool,
}

/// Get cluster status
#[utoipa::path(
    get,
    path = "/v1/cluster/status",
    params(ClusterStatusQuery),
    responses(
        (status = 200, description = "Cluster status retrieved successfully", body = ClusterStatus),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "cluster"
)]
pub async fn get_cluster_status(
    State(state): State<AppState>,
    Query(query): Query<ClusterStatusQuery>,
) -> Result<Json<ClusterStatus>, ApiError> {
    debug!("Getting cluster status with query: {:?}", query);
    
    // TODO: Replace with actual cluster state retrieval
    let members = vec![
        ClusterMember::new(1, "127.0.0.1:8080".to_string(), NodeRole::Voter),
        ClusterMember::new(2, "127.0.0.1:8081".to_string(), NodeRole::Voter),
        ClusterMember::new(3, "127.0.0.1:8082".to_string(), NodeRole::Voter),
    ];
    
    let mut status = ClusterStatus::new(
        Some(1), // leader_id
        42,      // term
        members,
        NodeState::Leader,
    );
    
    // Add health information if requested
    if query.include_health {
        status.health = ClusterHealth::Healthy;
    }
    
    // TODO: Fill in actual values from cluster state
    status.last_log_index = 1000;
    status.commit_index = 1000;
    status.applied_index = 1000;
    
    Ok(Json(status))
}

/// Add a new member to the cluster
#[utoipa::path(
    post,
    path = "/v1/cluster/members",
    request_body = AddMemberRequest,
    responses(
        (status = 200, description = "Member added successfully", body = MemberOperationResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 409, description = "Member already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "cluster"
)]
pub async fn add_member(
    State(state): State<AppState>,
    Json(request): Json<AddMemberRequest>,
) -> Result<Json<MemberOperationResponse>, ApiError> {
    info!("Adding member: node_id={}, address={}", request.node_id, request.address);
    
    // Validate request
    if request.node_id == 0 {
        return Err(ApiError::ValidationError(
            "Node ID must be greater than 0".to_string()
        ));
    }
    
    if request.address.is_empty() {
        return Err(ApiError::ValidationError(
            "Address cannot be empty".to_string()
        ));
    }
    
    // TODO: Implement actual member addition logic
    // This would involve:
    // 1. Checking if member already exists
    // 2. Adding to Raft configuration
    // 3. Waiting for configuration change to be committed
    
    let response = MemberOperationResponse {
        success: true,
        message: format!("Member {} added successfully", request.node_id),
        cluster_config: None, // TODO: Return updated config
    };
    
    // Notify via event bus if available
    if let Some(event_bus) = &state.event_bus {
        let member = ClusterMember::new(request.node_id, request.address, request.role);
        let event = super::schemas::ClusterEvent::MemberAdded {
            member,
            timestamp: SystemTime::now(),
        };
        event_bus.publish(event).await;
    }
    
    Ok(Json(response))
}

/// Remove a member from the cluster
#[utoipa::path(
    delete,
    path = "/v1/cluster/members/{node_id}",
    params(
        ("node_id" = u64, Path, description = "Node ID to remove")
    ),
    request_body = Option<RemoveMemberRequest>,
    responses(
        (status = 200, description = "Member removed successfully", body = MemberOperationResponse),
        (status = 404, description = "Member not found", body = ErrorResponse),
        (status = 400, description = "Cannot remove last member or leader", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "cluster"
)]
pub async fn remove_member(
    State(state): State<AppState>,
    Path(node_id): Path<u64>,
    Json(request): Json<Option<RemoveMemberRequest>>,
) -> Result<Json<MemberOperationResponse>, ApiError> {
    info!("Removing member: node_id={}", node_id);
    
    let force = request.map(|r| r.force).unwrap_or(false);
    
    // Validate removal
    if node_id == 0 {
        return Err(ApiError::ValidationError(
            "Invalid node ID".to_string()
        ));
    }
    
    // TODO: Implement actual member removal logic
    // This would involve:
    // 1. Checking if member exists
    // 2. Ensuring it's safe to remove (not the last member)
    // 3. Removing from Raft configuration
    // 4. Waiting for configuration change to be committed
    
    let response = MemberOperationResponse {
        success: true,
        message: format!("Member {} removed successfully", node_id),
        cluster_config: None, // TODO: Return updated config
    };
    
    // Notify via event bus if available
    if let Some(event_bus) = &state.event_bus {
        let event = super::schemas::ClusterEvent::MemberRemoved {
            node_id,
            timestamp: SystemTime::now(),
        };
        event_bus.publish(event).await;
    }
    
    Ok(Json(response))
}

/// Transfer leadership to another node
#[utoipa::path(
    post,
    path = "/v1/leadership/transfer",
    request_body = TransferLeadershipRequest,
    responses(
        (status = 200, description = "Leadership transfer initiated", body = MemberOperationResponse),
        (status = 400, description = "Invalid request or not leader", body = ErrorResponse),
        (status = 404, description = "Target node not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "leadership"
)]
pub async fn transfer_leadership(
    State(state): State<AppState>,
    Json(request): Json<TransferLeadershipRequest>,
) -> Result<Json<MemberOperationResponse>, ApiError> {
    info!("Transferring leadership to node {}", request.target_node_id);
    
    // Validate request
    if request.target_node_id == 0 {
        return Err(ApiError::ValidationError(
            "Target node ID must be greater than 0".to_string()
        ));
    }
    
    // TODO: Implement actual leadership transfer
    // This would involve:
    // 1. Checking if current node is leader
    // 2. Checking if target node exists and is eligible
    // 3. Initiating leadership transfer
    // 4. Waiting for transfer completion or timeout
    
    let response = MemberOperationResponse {
        success: true,
        message: format!("Leadership transfer to node {} initiated", request.target_node_id),
        cluster_config: None,
    };
    
    Ok(Json(response))
}

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/v1/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse),
        (status = 503, description = "Service is unhealthy", body = HealthResponse)
    ),
    tag = "health"
)]
pub async fn health_check(
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    debug!("Performing health check");
    
    let uptime = state.start_time.elapsed().unwrap_or(Duration::ZERO);
    
    let mut response = HealthResponse::healthy(uptime);
    
    // Add various health checks
    response = response.with_check(HealthCheck {
        name: "storage".to_string(),
        status: HealthStatus::Healthy, // TODO: Check actual storage health
        message: Some("Storage is accessible".to_string()),
        duration: Duration::from_millis(5),
    });
    
    response = response.with_check(HealthCheck {
        name: "raft".to_string(),
        status: HealthStatus::Healthy, // TODO: Check Raft health
        message: Some("Raft consensus is operational".to_string()),
        duration: Duration::from_millis(2),
    });
    
    response = response.with_check(HealthCheck {
        name: "transport".to_string(),
        status: HealthStatus::Healthy, // TODO: Check transport health
        message: Some("P2P transport is operational".to_string()),
        duration: Duration::from_millis(3),
    });
    
    // Determine HTTP status based on overall health
    let status_code = match response.status {
        HealthStatus::Healthy => StatusCode::OK,
        HealthStatus::Degraded => StatusCode::OK, // Still operational
        HealthStatus::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
    };
    
    Ok((status_code, Json(response)).into_response())
}

/// Readiness check endpoint
#[utoipa::path(
    get,
    path = "/v1/ready",
    responses(
        (status = 200, description = "Service is ready", body = ReadinessResponse),
        (status = 503, description = "Service is not ready", body = ReadinessResponse)
    ),
    tag = "health"
)]
pub async fn readiness_check(
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    debug!("Performing readiness check");
    
    let checks = vec![
        ReadinessCheck {
            name: "raft_initialized".to_string(),
            ready: true, // TODO: Check if Raft is initialized
            message: Some("Raft node is initialized and ready".to_string()),
        },
        ReadinessCheck {
            name: "storage_ready".to_string(),
            ready: true, // TODO: Check storage readiness
            message: Some("Storage backend is ready".to_string()),
        },
        ReadinessCheck {
            name: "transport_ready".to_string(),
            ready: true, // TODO: Check transport readiness
            message: Some("Transport layer is ready".to_string()),
        },
    ];
    
    let response = ReadinessResponse::new(checks);
    
    let status_code = if response.ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    
    Ok((status_code, Json(response)).into_response())
}

/// Get metrics in Prometheus format
#[utoipa::path(
    get,
    path = "/v1/metrics",
    responses(
        (status = 200, description = "Metrics retrieved successfully", body = MetricsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "metrics"
)]
pub async fn get_metrics(
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    debug!("Getting metrics");
    
    // TODO: Implement actual metrics collection
    // This would integrate with the metrics module to collect:
    // - Raft metrics (term, log index, etc.)
    // - Transport metrics (connections, messages)
    // - Storage metrics (operations, disk usage)
    // - System metrics (memory, CPU)
    
    let metrics_text = format!(
        "# HELP raft_term Current Raft term\n\
         # TYPE raft_term gauge\n\
         raft_term 42\n\
         \n\
         # HELP raft_log_index Current log index\n\
         # TYPE raft_log_index gauge\n\
         raft_log_index 1000\n\
         \n\
         # HELP raft_commit_index Current commit index\n\
         # TYPE raft_commit_index gauge\n\
         raft_commit_index 1000\n\
         \n\
         # HELP cluster_members Number of cluster members\n\
         # TYPE cluster_members gauge\n\
         cluster_members 3\n\
         \n\
         # HELP management_api_requests_total Total number of API requests\n\
         # TYPE management_api_requests_total counter\n\
         management_api_requests_total{{method=\"GET\",path=\"/v1/metrics\"}} 1\n"
    );
    
    let response = MetricsResponse {
        metrics: metrics_text,
        content_type: "text/plain; version=0.0.4".to_string(),
        timestamp: SystemTime::now(),
    };
    
    // Return with Prometheus content type
    Ok((
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        response.metrics,
    ).into_response())
}

/// API error type for handlers
#[derive(Debug)]
pub enum ApiError {
    /// Input validation error
    ValidationError(String),
    
    /// Resource not found
    NotFound(String),
    
    /// Operation conflict
    Conflict(String),
    
    /// Internal server error
    Internal(String),
    
    /// Service unavailable
    ServiceUnavailable(String),
    
    /// Underlying Raft error
    RaftError(crate::error::RaftError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match self {
            ApiError::ValidationError(msg) => (StatusCode::BAD_REQUEST, "validation_error", msg),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", msg),
            ApiError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable", msg),
            ApiError::RaftError(err) => {
                let category = err.category();
                let status = match category {
                    "validation" => StatusCode::BAD_REQUEST,
                    "raft" if err.to_string().contains("not leader") => StatusCode::CONFLICT,
                    "transport" | "storage" => StatusCode::SERVICE_UNAVAILABLE,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, category, err.to_string())
            }
        };
        
        let error_response = ErrorResponse::new(error_type, &message);
        (status, Json(error_response)).into_response()
    }
}

impl From<crate::error::RaftError> for ApiError {
    fn from(err: crate::error::RaftError) -> Self {
        ApiError::RaftError(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use tower::ServiceExt;
    
    fn create_test_app() -> Router {
        let state = AppState {
            cluster: Arc::new(ClusterHandle::new()),
            start_time: SystemTime::now(),
            event_bus: None,
        };
        
        Router::new()
            .route("/v1/cluster/status", get(get_cluster_status))
            .route("/v1/health", get(health_check))
            .route("/v1/ready", get(readiness_check))
            .with_state(state)
    }
    
    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_test_app();
        
        let response = app
            .oneshot(Request::builder().uri("/v1/health").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
    
    #[tokio::test]
    async fn test_readiness_endpoint() {
        let app = create_test_app();
        
        let response = app
            .oneshot(Request::builder().uri("/v1/ready").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
    
    #[tokio::test]
    async fn test_cluster_status_endpoint() {
        let app = create_test_app();
        
        let response = app
            .oneshot(Request::builder().uri("/v1/cluster/status").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
}