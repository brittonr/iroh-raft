//! HTTP server implementation for the management API
//!
//! This module provides the main ManagementServer implementation with OpenAPI
//! integration, middleware setup, and graceful shutdown handling.

use axum::{
    extract::DefaultBodyLimit,
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Method, StatusCode,
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    net::TcpListener,
    select,
    signal,
    sync::{broadcast, RwLock},
    time::timeout,
};
use tower::{
    layer::util::Stack,
    timeout::TimeoutLayer,
    ServiceBuilder,
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{debug, error, info, warn, Level};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use super::{
    config::ManagementConfig,
    events::{EventBus, start_connection_cleanup, websocket_handler, sse_handler, list_connections, event_bus_stats},
    handlers::{
        AppState, ClusterHandle, get_cluster_status, add_member, remove_member,
        transfer_leadership, health_check, readiness_check, get_metrics,
    },
    schemas::*,
};

/// OpenAPI documentation structure
#[derive(OpenApi)]
#[openapi(
    paths(
        super::handlers::get_cluster_status,
        super::handlers::add_member,
        super::handlers::remove_member,
        super::handlers::transfer_leadership,
        super::handlers::health_check,
        super::handlers::readiness_check,
        super::handlers::get_metrics,
    ),
    components(
        schemas(
            ClusterStatus,
            ClusterMember,
            NodeState,
            NodeRole,
            ClusterHealth,
            AddMemberRequest,
            RemoveMemberRequest,
            TransferLeadershipRequest,
            MemberOperationResponse,
            ClusterConfiguration,
            HealthResponse,
            HealthStatus,
            HealthCheck,
            ReadinessResponse,
            ReadinessCheck,
            MetricsResponse,
            ErrorResponse,
            ClusterEvent,
            SubscriptionRequest,
        )
    ),
    tags(
        (name = "cluster", description = "Cluster management operations"),
        (name = "leadership", description = "Leadership operations"),
        (name = "health", description = "Health and readiness checks"),
        (name = "metrics", description = "Metrics and monitoring"),
        (name = "events", description = "Real-time event streaming")
    ),
    info(
        title = "iroh-raft Management API",
        version = "0.1.0",
        description = "REST API for managing and monitoring iroh-raft clusters",
        contact(
            name = "iroh-raft",
            url = "https://github.com/blixard/iroh-raft"
        ),
        license(
            name = "MIT OR Apache-2.0"
        )
    ),
    servers(
        (url = "/", description = "Local server")
    ),
    external_docs(
        url = "https://github.com/blixard/iroh-raft/tree/main/docs",
        description = "Find more info here"
    )
)]
struct ApiDoc;

/// Management HTTP server
pub struct ManagementServer {
    /// Configuration for the server
    config: ManagementConfig,
    
    /// Handle to the Raft cluster
    cluster_handle: Arc<ClusterHandle>,
    
    /// Event bus for WebSocket streaming
    event_bus: Arc<EventBus>,
    
    /// Server start time
    start_time: SystemTime,
    
    /// Shutdown signal sender
    shutdown_tx: Option<broadcast::Sender<()>>,
    
    /// Server state
    state: Arc<RwLock<ServerState>>,
}

/// Server state tracking
#[derive(Debug, Default)]
struct ServerState {
    /// Whether the server is running
    running: bool,
    
    /// Number of active connections
    active_connections: usize,
    
    /// Total requests served
    total_requests: u64,
    
    /// Server statistics
    stats: ServerStats,
}

/// Server statistics
#[derive(Debug, Default)]
struct ServerStats {
    /// Requests by endpoint
    pub requests_by_endpoint: std::collections::HashMap<String, u64>,
    
    /// Request durations (for metrics)
    pub request_durations: Vec<Duration>,
}

impl ManagementServer {
    /// Create a new management server
    pub fn new(cluster_handle: Arc<ClusterHandle>, config: ManagementConfig) -> Self {
        let event_bus = Arc::new(EventBus::new(1000));
        
        Self {
            config,
            cluster_handle,
            event_bus,
            start_time: SystemTime::now(),
            shutdown_tx: None,
            state: Arc::new(RwLock::new(ServerState::default())),
        }
    }
    
    /// Start the HTTP server
    pub async fn serve(mut self) -> Result<(), crate::error::RaftError> {
        info!("Starting management HTTP server on {}", self.config.bind_address);
        
        // Validate configuration
        self.config.validate()?;
        
        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);
        
        // Create application state
        let app_state = AppState {
            cluster: self.cluster_handle.clone(),
            start_time: self.start_time,
            event_bus: Some(self.event_bus.clone()),
        };
        
        // Build the router
        let app = self.build_router(app_state).await?;
        
        // Create TCP listener
        let listener = TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|e| crate::error::RaftError::Io {
                operation: "bind_address".to_string(),
                source: e,
                backtrace: snafu::Backtrace::new(),
            })?;
        
        info!("Management API server listening on {}", self.config.bind_address);
        
        // Start connection cleanup task
        if self.config.websocket.enabled {
            start_connection_cleanup(
                self.event_bus.clone(),
                Duration::from_secs(60), // cleanup interval
                self.config.websocket.connection_timeout,
            );
        }
        
        // Mark server as running
        {
            let mut state = self.state.write().await;
            state.running = true;
        }
        
        // Serve with graceful shutdown
        let server = axum::serve(listener, app);
        
        select! {
            result = server => {
                if let Err(e) = result {
                    error!("Server error: {}", e);
                    return Err(crate::error::RaftError::Internal {
                        message: format!("HTTP server error: {}", e),
                        backtrace: snafu::Backtrace::new(),
                    });
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Received shutdown signal");
            }
            _ = signal::ctrl_c() => {
                info!("Received Ctrl+C, shutting down");
            }
        }
        
        self.shutdown().await?;
        Ok(())
    }
    
    /// Gracefully shutdown the server
    pub async fn shutdown(self) -> Result<(), crate::error::RaftError> {
        info!("Shutting down management server");
        
        // Mark server as not running
        {
            let mut state = self.state.write().await;
            state.running = false;
        }
        
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx {
            let _ = tx.send(());
        }
        
        // TODO: Wait for active connections to finish
        // TODO: Close WebSocket connections gracefully
        
        info!("Management server shutdown complete");
        Ok(())
    }
    
    /// Build the router with all routes and middleware
    async fn build_router(&self, app_state: AppState) -> Result<Router, crate::error::RaftError> {
        let mut router = Router::new()
            // Health endpoints
            .route("/v1/health", get(health_check))
            .route("/v1/ready", get(readiness_check))
            
            // Cluster management
            .route("/v1/cluster/status", get(get_cluster_status))
            .route("/v1/cluster/members", post(add_member))
            .route("/v1/cluster/members/:node_id", delete(remove_member))
            
            // Leadership operations
            .route("/v1/leadership/transfer", post(transfer_leadership))
            
            // Metrics
            .route("/v1/metrics", get(get_metrics));
        
        // Add WebSocket and SSE endpoints if enabled
        // TODO: Re-enable once handler issues are resolved
        // if self.config.websocket.enabled {
        //     router = router
        //         .route("/v1/events/ws", get(websocket_handler))
        //         .route("/v1/events/sse", get(sse_handler))
        //         .route("/v1/events/connections", get(list_connections))
        //         .route("/v1/events/stats", get(event_bus_stats));
        // }
        
        // Add Swagger UI if enabled
        if self.config.enable_swagger_ui {
            router = router.merge(
                SwaggerUi::new("/swagger-ui")
                    .url("/api-docs/openapi.json", ApiDoc::openapi())
            );
        }
        
        // Apply middleware
        let router = router
            .layer(DefaultBodyLimit::max(self.config.max_request_size));
        
        let router = if self.config.enable_cors {
            router.layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
                    .allow_headers([AUTHORIZATION, CONTENT_TYPE])
            )
        } else {
            router
        };
        
        let router = router
            .layer(
                TraceLayer::new_for_http()
                    .on_request(DefaultOnRequest::new().level(Level::DEBUG))
                    .on_response(DefaultOnResponse::new().level(Level::DEBUG))
            )
            .layer(middleware::from_fn_with_state(
                app_state.clone(),
                request_metrics_middleware
            ));
        
        // Add authentication middleware if required
        let router = if self.config.security.require_auth {
            router.layer(middleware::from_fn_with_state(
                self.config.clone(),
                auth_middleware
            ))
        } else {
            router
        };
        
        Ok(router.with_state(app_state))
    }
    
    /// Get server statistics
    pub async fn stats(&self) -> ServerStats {
        let state = self.state.read().await;
        ServerStats {
            requests_by_endpoint: state.stats.requests_by_endpoint.clone(),
            request_durations: state.stats.request_durations.clone(),
        }
    }
    
    /// Check if server is running
    pub async fn is_running(&self) -> bool {
        let state = self.state.read().await;
        state.running
    }
}

/// Authentication middleware
async fn auth_middleware(
    axum::extract::State(config): axum::extract::State<ManagementConfig>,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !config.security.require_auth {
        return Ok(next.run(request).await);
    }
    
    // Skip auth for health endpoints
    let path = request.uri().path();
    if path == "/v1/health" || path == "/v1/ready" {
        return Ok(next.run(request).await);
    }
    
    // Check for API key
    if let Some(expected_key) = &config.security.api_key {
        if let Some(auth_header) = request.headers().get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(key) = auth_str.strip_prefix("Bearer ") {
                    if key == expected_key {
                        return Ok(next.run(request).await);
                    }
                }
            }
        }
    }
    
    Err(StatusCode::UNAUTHORIZED)
}

/// Request metrics middleware
async fn request_metrics_middleware(
    axum::extract::State(app_state): axum::extract::State<AppState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let path = request.uri().path().to_string();
    let method = request.method().to_string();
    
    let response = next.run(request).await;
    
    let duration = start.elapsed();
    
    // TODO: Update metrics in a proper metrics system
    debug!("Request: {} {} - {}ms", method, path, duration.as_millis());
    
    response
}

/// Server information endpoint
#[derive(serde::Serialize, ToSchema)]
pub struct ServerInfo {
    /// Server version
    pub version: String,
    
    /// Server uptime
    pub uptime: Duration,
    
    /// Configuration summary
    pub config: ServerConfigSummary,
    
    /// Runtime statistics
    pub stats: RuntimeStats,
}

/// Server configuration summary
#[derive(serde::Serialize, ToSchema)]
pub struct ServerConfigSummary {
    /// Bind address
    pub bind_address: String,
    
    /// Swagger UI enabled
    pub swagger_ui_enabled: bool,
    
    /// CORS enabled
    pub cors_enabled: bool,
    
    /// WebSocket enabled
    pub websocket_enabled: bool,
    
    /// Authentication required
    pub auth_required: bool,
}

/// Runtime statistics
#[derive(serde::Serialize, ToSchema)]
pub struct RuntimeStats {
    /// Total requests served
    pub total_requests: u64,
    
    /// Active WebSocket connections
    pub active_websocket_connections: usize,
    
    /// Average request duration
    pub avg_request_duration_ms: f64,
}

/// Get server information
#[utoipa::path(
    get,
    path = "/v1/server/info",
    responses(
        (status = 200, description = "Server information", body = ServerInfo)
    ),
    tag = "server"
)]
pub async fn server_info(
    axum::extract::State(app_state): axum::extract::State<AppState>,
) -> Json<ServerInfo> {
    let uptime = app_state.start_time.elapsed().unwrap_or(Duration::ZERO);
    
    let info = ServerInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime,
        config: ServerConfigSummary {
            bind_address: "unknown".to_string(), // TODO: Get from actual config
            swagger_ui_enabled: true,
            cors_enabled: true,
            websocket_enabled: true,
            auth_required: false,
        },
        stats: RuntimeStats {
            total_requests: 0, // TODO: Get from actual stats
            active_websocket_connections: 0, // TODO: Get from event bus
            avg_request_duration_ms: 0.0,
        },
    };
    
    Json(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    
    #[tokio::test]
    async fn test_server_creation() {
        let config = ManagementConfig::default();
        let cluster_handle = Arc::new(ClusterHandle::new());
        let server = ManagementServer::new(cluster_handle, config);
        
        assert!(!server.is_running().await);
    }
    
    #[tokio::test]
    async fn test_router_creation() {
        let config = ManagementConfig::default();
        let cluster_handle = Arc::new(ClusterHandle::new());
        let server = ManagementServer::new(cluster_handle.clone(), config);
        
        let app_state = AppState {
            cluster: cluster_handle,
            start_time: SystemTime::now(),
            event_bus: Some(server.event_bus.clone()),
        };
        
        let router = server.build_router(app_state).await.unwrap();
        
        // Test that we can create the router without errors
        assert!(std::mem::size_of_val(&router) > 0);
    }
    
    #[tokio::test]
    async fn test_health_endpoint_integration() {
        let config = ManagementConfig::default();
        let cluster_handle = Arc::new(ClusterHandle::new());
        let server = ManagementServer::new(cluster_handle.clone(), config);
        
        let app_state = AppState {
            cluster: cluster_handle,
            start_time: SystemTime::now(),
            event_bus: Some(server.event_bus.clone()),
        };
        
        let app = server.build_router(app_state).await.unwrap();
        
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(axum::body::Body::empty())
                    .unwrap()
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
}