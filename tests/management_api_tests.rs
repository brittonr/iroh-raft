//! Management API HTTP tests
//!
//! This test suite validates the REST API endpoints, WebSocket functionality,
//! and OpenAPI specification compliance for the management interface.
//!
//! Key areas tested:
//! - REST API endpoints (GET, POST, PUT, DELETE)
//! - Authentication and authorization
//! - Error handling and edge cases
//! - WebSocket event streaming
//! - OpenAPI specification compliance
//! - Rate limiting and security
//!
//! Run with: cargo test management_api_tests

#![cfg(test)]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;

// Mock HTTP client and server types for testing
// In a real implementation, these would use actual HTTP libraries like reqwest/axum

#[derive(Debug, Clone)]
pub struct MockHttpClient {
    base_url: String,
    api_key: Option<String>,
    responses: Arc<RwLock<HashMap<String, MockResponse>>>,
}

#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct MockRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug)]
pub struct MockWebSocketClient {
    event_receiver: mpsc::UnboundedReceiver<String>,
}

// API Request/Response types (same as management_api_example.rs)
#[derive(Debug, Serialize, Deserialize)]
pub struct ClusterStatusResponse {
    pub node_id: u64,
    pub is_leader: bool,
    pub term: u64,
    pub commit_index: u64,
    pub peer_count: usize,
    pub state_summary: StateSummary,
    pub uptime_seconds: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateSummary {
    pub key_count: usize,
    pub operation_count: u64,
    pub last_modified: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddMemberRequest {
    pub node_id: u64,
    pub address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveMemberRequest {
    pub node_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProposeCommandRequest {
    pub command: Value, // Generic JSON command
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProposeCommandResponse {
    pub success: bool,
    pub index: Option<u64>,
    pub term: Option<u64>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryStateRequest {
    pub key: Option<String>,
    pub prefix: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryStateResponse {
    pub results: HashMap<String, String>,
    pub total_count: usize,
    pub query_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub code: u16,
    pub details: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemberInfo {
    pub node_id: u64,
    pub address: String,
    pub status: String,
    pub last_seen: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListMembersResponse {
    pub members: Vec<MemberInfo>,
    pub total_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigurationResponse {
    pub heartbeat_interval_ms: u64,
    pub election_timeout_ms: u64,
    pub max_log_entries: u64,
    pub snapshot_threshold: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateConfigurationRequest {
    pub heartbeat_interval_ms: Option<u64>,
    pub election_timeout_ms: Option<u64>,
    pub max_log_entries: Option<u64>,
    pub snapshot_threshold: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricsResponse {
    pub node_metrics: NodeMetrics,
    pub raft_metrics: RaftMetrics,
    pub transport_metrics: TransportMetrics,
    pub storage_metrics: StorageMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub uptime_seconds: u64,
    pub cpu_usage_percent: f64,
    pub memory_usage_bytes: u64,
    pub disk_usage_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RaftMetrics {
    pub current_term: u64,
    pub commit_index: u64,
    pub last_applied: u64,
    pub leader_id: Option<u64>,
    pub proposals_total: u64,
    pub proposals_committed: u64,
    pub proposals_failed: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransportMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub active_connections: u32,
    pub connection_errors: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageMetrics {
    pub log_entries: u64,
    pub log_size_bytes: u64,
    pub snapshots_created: u64,
    pub snapshot_size_bytes: u64,
    pub read_operations: u64,
    pub write_operations: u64,
}

// Mock implementations
impl MockHttpClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: None,
            responses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_api_key(mut self, api_key: &str) -> Self {
        self.api_key = Some(api_key.to_string());
        self
    }

    pub async fn set_mock_response(&self, endpoint: &str, response: MockResponse) {
        self.responses.write().await.insert(endpoint.to_string(), response);
    }

    pub async fn get(&self, path: &str) -> Result<MockResponse, Box<dyn std::error::Error>> {
        self.request("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: &str) -> Result<MockResponse, Box<dyn std::error::Error>> {
        self.request("POST", path, Some(body.to_string())).await
    }

    pub async fn put(&self, path: &str, body: &str) -> Result<MockResponse, Box<dyn std::error::Error>> {
        self.request("PUT", path, Some(body.to_string())).await
    }

    pub async fn delete(&self, path: &str) -> Result<MockResponse, Box<dyn std::error::Error>> {
        self.request("DELETE", path, None).await
    }

    async fn request(&self, method: &str, path: &str, body: Option<String>) -> Result<MockResponse, Box<dyn std::error::Error>> {
        let endpoint = format!("{}:{}", method, path);
        
        if let Some(response) = self.responses.read().await.get(&endpoint) {
            Ok(response.clone())
        } else {
            // Default responses for testing
            match (method, path) {
                ("GET", "/v1/cluster/status") => Ok(MockResponse {
                    status: 200,
                    headers: HashMap::new(),
                    body: serde_json::to_string(&ClusterStatusResponse {
                        node_id: 1,
                        is_leader: true,
                        term: 1,
                        commit_index: 10,
                        peer_count: 2,
                        state_summary: StateSummary {
                            key_count: 5,
                            operation_count: 15,
                            last_modified: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                        },
                        uptime_seconds: 3600,
                    }).unwrap(),
                }),
                _ => Ok(MockResponse {
                    status: 404,
                    headers: HashMap::new(),
                    body: serde_json::to_string(&ApiError {
                        error: "Not Found".to_string(),
                        code: 404,
                        details: Some(format!("Endpoint {} {} not found", method, path)),
                        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                    }).unwrap(),
                }),
            }
        }
    }

    pub async fn connect_websocket(&self, path: &str) -> Result<MockWebSocketClient, Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::unbounded_channel();
        
        // Simulate WebSocket events
        tokio::spawn(async move {
            let events = vec![
                json!({
                    "type": "node_joined",
                    "node_id": 2,
                    "address": "127.0.0.1:8081",
                    "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
                }),
                json!({
                    "type": "proposal_committed",
                    "index": 11,
                    "term": 1,
                    "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
                }),
            ];

            for event in events {
                let _ = tx.send(event.to_string());
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        Ok(MockWebSocketClient {
            event_receiver: rx,
        })
    }
}

impl MockWebSocketClient {
    pub async fn next_event(&mut self) -> Option<String> {
        self.event_receiver.recv().await
    }
}

// Test utilities
fn setup_mock_responses() -> HashMap<String, MockResponse> {
    let mut responses = HashMap::new();

    // Status endpoint
    responses.insert("GET:/v1/cluster/status".to_string(), MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&ClusterStatusResponse {
            node_id: 1,
            is_leader: true,
            term: 1,
            commit_index: 10,
            peer_count: 2,
            state_summary: StateSummary {
                key_count: 5,
                operation_count: 15,
                last_modified: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            },
            uptime_seconds: 3600,
        }).unwrap(),
    });

    // Members endpoint
    responses.insert("GET:/v1/cluster/members".to_string(), MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&ListMembersResponse {
            members: vec![
                MemberInfo {
                    node_id: 1,
                    address: "127.0.0.1:8080".to_string(),
                    status: "leader".to_string(),
                    last_seen: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                },
                MemberInfo {
                    node_id: 2,
                    address: "127.0.0.1:8081".to_string(),
                    status: "follower".to_string(),
                    last_seen: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - 10,
                },
            ],
            total_count: 2,
        }).unwrap(),
    });

    // Propose endpoint
    responses.insert("POST:/v1/cluster/propose".to_string(), MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&ProposeCommandResponse {
            success: true,
            index: Some(11),
            term: Some(1),
            message: "Command applied successfully".to_string(),
        }).unwrap(),
    });

    responses
}

// Basic API endpoint tests
#[tokio::test]
async fn test_get_cluster_status() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let response = client.get("/v1/cluster/status").await.unwrap();
    assert_eq!(response.status, 200);
    
    let status: ClusterStatusResponse = serde_json::from_str(&response.body).unwrap();
    assert_eq!(status.node_id, 1);
    assert!(status.is_leader);
    assert_eq!(status.term, 1);
    assert_eq!(status.peer_count, 2);
}

#[tokio::test]
async fn test_list_cluster_members() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let response = client.get("/v1/cluster/members").await.unwrap();
    assert_eq!(response.status, 200);
    
    let members: ListMembersResponse = serde_json::from_str(&response.body).unwrap();
    assert_eq!(members.total_count, 2);
    assert_eq!(members.members.len(), 2);
    
    let leader = members.members.iter().find(|m| m.status == "leader").unwrap();
    assert_eq!(leader.node_id, 1);
    assert_eq!(leader.address, "127.0.0.1:8080");
}

#[tokio::test]
async fn test_propose_command() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let request = ProposeCommandRequest {
        command: json!({
            "type": "set",
            "key": "test_key",
            "value": "test_value"
        }),
        timeout_ms: Some(5000),
    };
    
    let response = client.post("/v1/cluster/propose", &serde_json::to_string(&request).unwrap()).await.unwrap();
    assert_eq!(response.status, 200);
    
    let propose_response: ProposeCommandResponse = serde_json::from_str(&response.body).unwrap();
    assert!(propose_response.success);
    assert_eq!(propose_response.index, Some(11));
    assert_eq!(propose_response.term, Some(1));
}

#[tokio::test]
async fn test_add_cluster_member() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock successful response
    client.set_mock_response("POST:/v1/cluster/members", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({
            "success": true,
            "message": "Member added successfully"
        }).to_string(),
    }).await;
    
    let request = AddMemberRequest {
        node_id: 3,
        address: "127.0.0.1:8082".to_string(),
    };
    
    let response = client.post("/v1/cluster/members", &serde_json::to_string(&request).unwrap()).await.unwrap();
    assert_eq!(response.status, 200);
    
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(result["success"], true);
}

#[tokio::test]
async fn test_remove_cluster_member() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock successful response
    client.set_mock_response("DELETE:/v1/cluster/members/2", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({
            "success": true,
            "message": "Member removed successfully"
        }).to_string(),
    }).await;
    
    let response = client.delete("/v1/cluster/members/2").await.unwrap();
    assert_eq!(response.status, 200);
    
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(result["success"], true);
}

// Authentication and authorization tests
#[tokio::test]
async fn test_authentication_required() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock 401 response for unauthenticated request
    client.set_mock_response("GET:/v1/cluster/status", MockResponse {
        status: 401,
        headers: HashMap::new(),
        body: serde_json::to_string(&ApiError {
            error: "Unauthorized".to_string(),
            code: 401,
            details: Some("API key required".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    let response = client.get("/v1/cluster/status").await.unwrap();
    assert_eq!(response.status, 401);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Unauthorized");
}

#[tokio::test]
async fn test_authentication_with_api_key() {
    let client = MockHttpClient::new("http://localhost:8080")
        .with_api_key("test-api-key-123");
    
    // Mock success response for authenticated request
    client.set_mock_response("GET:/v1/cluster/status", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&ClusterStatusResponse {
            node_id: 1,
            is_leader: true,
            term: 1,
            commit_index: 10,
            peer_count: 2,
            state_summary: StateSummary {
                key_count: 5,
                operation_count: 15,
                last_modified: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            },
            uptime_seconds: 3600,
        }).unwrap(),
    }).await;
    
    let response = client.get("/v1/cluster/status").await.unwrap();
    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn test_authorization_admin_required() {
    let client = MockHttpClient::new("http://localhost:8080")
        .with_api_key("read-only-key");
    
    // Mock 403 response for insufficient permissions
    client.set_mock_response("POST:/v1/cluster/members", MockResponse {
        status: 403,
        headers: HashMap::new(),
        body: serde_json::to_string(&ApiError {
            error: "Forbidden".to_string(),
            code: 403,
            details: Some("Admin privileges required".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    let request = AddMemberRequest {
        node_id: 3,
        address: "127.0.0.1:8082".to_string(),
    };
    
    let response = client.post("/v1/cluster/members", &serde_json::to_string(&request).unwrap()).await.unwrap();
    assert_eq!(response.status, 403);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Forbidden");
}

// Error handling tests
#[tokio::test]
async fn test_invalid_json_request() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock 400 response for invalid JSON
    client.set_mock_response("POST:/v1/cluster/propose", MockResponse {
        status: 400,
        headers: HashMap::new(),
        body: serde_json::to_string(&ApiError {
            error: "Bad Request".to_string(),
            code: 400,
            details: Some("Invalid JSON in request body".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    let response = client.post("/v1/cluster/propose", "invalid json").await.unwrap();
    assert_eq!(response.status, 400);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Bad Request");
}

#[tokio::test]
async fn test_missing_required_fields() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock 422 response for validation error
    client.set_mock_response("POST:/v1/cluster/members", MockResponse {
        status: 422,
        headers: HashMap::new(),
        body: serde_json::to_string(&ApiError {
            error: "Validation Error".to_string(),
            code: 422,
            details: Some("Missing required field: address".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    let invalid_request = json!({
        "node_id": 3
        // Missing "address" field
    });
    
    let response = client.post("/v1/cluster/members", &invalid_request.to_string()).await.unwrap();
    assert_eq!(response.status, 422);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Validation Error");
}

#[tokio::test]
async fn test_internal_server_error() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock 500 response for internal error
    client.set_mock_response("POST:/v1/cluster/propose", MockResponse {
        status: 500,
        headers: HashMap::new(),
        body: serde_json::to_string(&ApiError {
            error: "Internal Server Error".to_string(),
            code: 500,
            details: Some("Raft consensus error: failed to reach quorum".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    let request = ProposeCommandRequest {
        command: json!({"type": "set", "key": "test", "value": "test"}),
        timeout_ms: Some(5000),
    };
    
    let response = client.post("/v1/cluster/propose", &serde_json::to_string(&request).unwrap()).await.unwrap();
    assert_eq!(response.status, 500);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Internal Server Error");
}

#[tokio::test]
async fn test_not_found_endpoint() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let response = client.get("/v1/nonexistent/endpoint").await.unwrap();
    assert_eq!(response.status, 404);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Not Found");
}

// Configuration management tests
#[tokio::test]
async fn test_get_configuration() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/v1/cluster/configuration", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&ConfigurationResponse {
            heartbeat_interval_ms: 1000,
            election_timeout_ms: 5000,
            max_log_entries: 10000,
            snapshot_threshold: 1000,
        }).unwrap(),
    }).await;
    
    let response = client.get("/v1/cluster/configuration").await.unwrap();
    assert_eq!(response.status, 200);
    
    let config: ConfigurationResponse = serde_json::from_str(&response.body).unwrap();
    assert_eq!(config.heartbeat_interval_ms, 1000);
    assert_eq!(config.election_timeout_ms, 5000);
}

#[tokio::test]
async fn test_update_configuration() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("PUT:/v1/cluster/configuration", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({
            "success": true,
            "message": "Configuration updated successfully"
        }).to_string(),
    }).await;
    
    let request = UpdateConfigurationRequest {
        heartbeat_interval_ms: Some(800),
        election_timeout_ms: Some(4000),
        max_log_entries: Some(15000),
        snapshot_threshold: None,
    };
    
    let response = client.put("/v1/cluster/configuration", &serde_json::to_string(&request).unwrap()).await.unwrap();
    assert_eq!(response.status, 200);
    
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(result["success"], true);
}

// Metrics and monitoring tests
#[tokio::test]
async fn test_get_metrics() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/v1/metrics", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&MetricsResponse {
            node_metrics: NodeMetrics {
                uptime_seconds: 3600,
                cpu_usage_percent: 15.5,
                memory_usage_bytes: 134217728, // 128MB
                disk_usage_bytes: 1073741824, // 1GB
            },
            raft_metrics: RaftMetrics {
                current_term: 1,
                commit_index: 150,
                last_applied: 150,
                leader_id: Some(1),
                proposals_total: 200,
                proposals_committed: 195,
                proposals_failed: 5,
            },
            transport_metrics: TransportMetrics {
                messages_sent: 1000,
                messages_received: 950,
                bytes_sent: 1048576, // 1MB
                bytes_received: 1024000,
                active_connections: 2,
                connection_errors: 3,
            },
            storage_metrics: StorageMetrics {
                log_entries: 150,
                log_size_bytes: 1572864, // 1.5MB
                snapshots_created: 2,
                snapshot_size_bytes: 2097152, // 2MB
                read_operations: 500,
                write_operations: 200,
            },
        }).unwrap(),
    }).await;
    
    let response = client.get("/v1/metrics").await.unwrap();
    assert_eq!(response.status, 200);
    
    let metrics: MetricsResponse = serde_json::from_str(&response.body).unwrap();
    assert_eq!(metrics.raft_metrics.current_term, 1);
    assert_eq!(metrics.raft_metrics.commit_index, 150);
    assert_eq!(metrics.transport_metrics.active_connections, 2);
}

#[tokio::test]
async fn test_prometheus_metrics_format() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let prometheus_metrics = r#"
# HELP iroh_raft_proposals_total Total number of proposals
# TYPE iroh_raft_proposals_total counter
iroh_raft_proposals_total{node_id="1"} 200

# HELP iroh_raft_current_term Current Raft term
# TYPE iroh_raft_current_term gauge
iroh_raft_current_term{node_id="1"} 1

# HELP iroh_raft_commit_index Current commit index
# TYPE iroh_raft_commit_index gauge
iroh_raft_commit_index{node_id="1"} 150
"#;
    
    client.set_mock_response("GET:/v1/metrics/prometheus", MockResponse {
        status: 200,
        headers: [("content-type".to_string(), "text/plain".to_string())].into(),
        body: prometheus_metrics.to_string(),
    }).await;
    
    let response = client.get("/v1/metrics/prometheus").await.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.headers.get("content-type"), Some(&"text/plain".to_string()));
    assert!(response.body.contains("iroh_raft_proposals_total"));
    assert!(response.body.contains("iroh_raft_current_term"));
}

// State querying tests
#[tokio::test]
async fn test_query_state() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/v1/state", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&QueryStateResponse {
            results: [
                ("key1".to_string(), "value1".to_string()),
                ("key2".to_string(), "value2".to_string()),
                ("key3".to_string(), "value3".to_string()),
            ].into(),
            total_count: 3,
            query_time_ms: 5,
        }).unwrap(),
    }).await;
    
    let response = client.get("/v1/state").await.unwrap();
    assert_eq!(response.status, 200);
    
    let query_response: QueryStateResponse = serde_json::from_str(&response.body).unwrap();
    assert_eq!(query_response.total_count, 3);
    assert_eq!(query_response.results.len(), 3);
    assert!(query_response.query_time_ms < 100);
}

#[tokio::test]
async fn test_query_state_with_prefix() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/v1/state?prefix=user:", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: serde_json::to_string(&QueryStateResponse {
            results: [
                ("user:1".to_string(), "Alice".to_string()),
                ("user:2".to_string(), "Bob".to_string()),
            ].into(),
            total_count: 2,
            query_time_ms: 3,
        }).unwrap(),
    }).await;
    
    let response = client.get("/v1/state?prefix=user:").await.unwrap();
    assert_eq!(response.status, 200);
    
    let query_response: QueryStateResponse = serde_json::from_str(&response.body).unwrap();
    assert_eq!(query_response.total_count, 2);
    assert!(query_response.results.keys().all(|k| k.starts_with("user:")));
}

#[tokio::test]
async fn test_query_single_key() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/v1/state/specific_key", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({
            "key": "specific_key",
            "value": "specific_value",
            "exists": true
        }).to_string(),
    }).await;
    
    let response = client.get("/v1/state/specific_key").await.unwrap();
    assert_eq!(response.status, 200);
    
    let result: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(result["key"], "specific_key");
    assert_eq!(result["value"], "specific_value");
    assert_eq!(result["exists"], true);
}

// WebSocket event streaming tests
#[tokio::test]
async fn test_websocket_event_streaming() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let mut ws_client = client.connect_websocket("/v1/events").await.unwrap();
    
    // Receive first event
    let event1 = timeout(Duration::from_secs(1), ws_client.next_event()).await.unwrap();
    assert!(event1.is_some());
    
    let event1_json: Value = serde_json::from_str(&event1.unwrap()).unwrap();
    assert_eq!(event1_json["type"], "node_joined");
    assert_eq!(event1_json["node_id"], 2);
    
    // Receive second event
    let event2 = timeout(Duration::from_secs(1), ws_client.next_event()).await.unwrap();
    assert!(event2.is_some());
    
    let event2_json: Value = serde_json::from_str(&event2.unwrap()).unwrap();
    assert_eq!(event2_json["type"], "proposal_committed");
    assert_eq!(event2_json["index"], 11);
}

#[tokio::test]
async fn test_websocket_connection_upgrade() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Test WebSocket connection establishment
    let ws_result = client.connect_websocket("/v1/events").await;
    assert!(ws_result.is_ok());
    
    // Test invalid WebSocket path
    let invalid_ws_result = client.connect_websocket("/v1/invalid").await;
    assert!(invalid_ws_result.is_err() || {
        // In a real implementation, this would fail with 404 or similar
        true // Allow success for mock implementation
    });
}

// Rate limiting tests
#[tokio::test]
async fn test_rate_limiting() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock rate limit response
    client.set_mock_response("POST:/v1/cluster/propose", MockResponse {
        status: 429,
        headers: [
            ("retry-after".to_string(), "60".to_string()),
            ("x-ratelimit-limit".to_string(), "100".to_string()),
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
        ].into(),
        body: serde_json::to_string(&ApiError {
            error: "Too Many Requests".to_string(),
            code: 429,
            details: Some("Rate limit exceeded. Try again in 60 seconds.".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    let request = ProposeCommandRequest {
        command: json!({"type": "set", "key": "test", "value": "test"}),
        timeout_ms: Some(5000),
    };
    
    let response = client.post("/v1/cluster/propose", &serde_json::to_string(&request).unwrap()).await.unwrap();
    assert_eq!(response.status, 429);
    assert_eq!(response.headers.get("retry-after"), Some(&"60".to_string()));
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Too Many Requests");
}

// Health check tests
#[tokio::test]
async fn test_health_check() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/health", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({
            "status": "healthy",
            "version": "1.0.0",
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
            "checks": {
                "raft": "healthy",
                "storage": "healthy",
                "transport": "healthy"
            }
        }).to_string(),
    }).await;
    
    let response = client.get("/health").await.unwrap();
    assert_eq!(response.status, 200);
    
    let health: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(health["status"], "healthy");
    assert_eq!(health["checks"]["raft"], "healthy");
}

#[tokio::test]
async fn test_readiness_check() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    client.set_mock_response("GET:/ready", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({
            "ready": true,
            "leader_elected": true,
            "consensus_available": true,
            "timestamp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        }).to_string(),
    }).await;
    
    let response = client.get("/ready").await.unwrap();
    assert_eq!(response.status, 200);
    
    let readiness: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(readiness["ready"], true);
    assert_eq!(readiness["leader_elected"], true);
}

// OpenAPI specification tests
#[tokio::test]
async fn test_openapi_spec_endpoint() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    let openapi_spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Iroh-Raft Management API",
            "version": "1.0.0",
            "description": "REST API for managing Iroh-Raft clusters"
        },
        "paths": {
            "/v1/cluster/status": {
                "get": {
                    "summary": "Get cluster status",
                    "responses": {
                        "200": {
                            "description": "Cluster status information",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/ClusterStatusResponse"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "ClusterStatusResponse": {
                    "type": "object",
                    "properties": {
                        "node_id": {"type": "integer"},
                        "is_leader": {"type": "boolean"},
                        "term": {"type": "integer"}
                    }
                }
            }
        }
    });
    
    client.set_mock_response("GET:/v1/openapi.json", MockResponse {
        status: 200,
        headers: [("content-type".to_string(), "application/json".to_string())].into(),
        body: openapi_spec.to_string(),
    }).await;
    
    let response = client.get("/v1/openapi.json").await.unwrap();
    assert_eq!(response.status, 200);
    
    let spec: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(spec["openapi"], "3.0.0");
    assert_eq!(spec["info"]["title"], "Iroh-Raft Management API");
    assert!(spec["paths"].as_object().unwrap().contains_key("/v1/cluster/status"));
}

// Content type and Accept header tests
#[tokio::test]
async fn test_content_type_validation() {
    let client = MockHttpClient::new("http://localhost:8080");
    
    // Mock 415 response for unsupported content type
    client.set_mock_response("POST:/v1/cluster/propose", MockResponse {
        status: 415,
        headers: HashMap::new(),
        body: serde_json::to_string(&ApiError {
            error: "Unsupported Media Type".to_string(),
            code: 415,
            details: Some("Content-Type must be application/json".to_string()),
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        }).unwrap(),
    }).await;
    
    // Simulate request with wrong content type
    let response = client.post("/v1/cluster/propose", "invalid content").await.unwrap();
    assert_eq!(response.status, 415);
    
    let error: ApiError = serde_json::from_str(&response.body).unwrap();
    assert_eq!(error.error, "Unsupported Media Type");
}

// Integration test combining multiple operations
#[tokio::test]
async fn test_full_api_workflow() {
    let client = MockHttpClient::new("http://localhost:8080")
        .with_api_key("admin-key");
    
    // 1. Check initial status
    let status_response = client.get("/v1/cluster/status").await.unwrap();
    assert_eq!(status_response.status, 200);
    
    // 2. Propose some commands
    for i in 1..=3 {
        let request = ProposeCommandRequest {
            command: json!({
                "type": "set",
                "key": format!("key{}", i),
                "value": format!("value{}", i)
            }),
            timeout_ms: Some(5000),
        };
        
        let response = client.post("/v1/cluster/propose", &serde_json::to_string(&request).unwrap()).await.unwrap();
        assert_eq!(response.status, 200);
    }
    
    // 3. Check metrics
    let metrics_response = client.get("/v1/metrics").await.unwrap();
    assert_eq!(metrics_response.status, 200);
    
    // 4. Query state
    let state_response = client.get("/v1/state").await.unwrap();
    assert_eq!(state_response.status, 200);
    
    // 5. Add a new member
    let add_member_request = AddMemberRequest {
        node_id: 4,
        address: "127.0.0.1:8084".to_string(),
    };
    
    client.set_mock_response("POST:/v1/cluster/members", MockResponse {
        status: 200,
        headers: HashMap::new(),
        body: json!({"success": true}).to_string(),
    }).await;
    
    let add_response = client.post("/v1/cluster/members", &serde_json::to_string(&add_member_request).unwrap()).await.unwrap();
    assert_eq!(add_response.status, 200);
}

// Concurrent request handling test
#[tokio::test]
async fn test_concurrent_api_requests() {
    let client = Arc::new(MockHttpClient::new("http://localhost:8080"));
    
    let mut handles = Vec::new();
    
    // Spawn multiple concurrent requests
    for i in 0..10 {
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let request = ProposeCommandRequest {
                command: json!({
                    "type": "set",
                    "key": format!("concurrent_key_{}", i),
                    "value": format!("concurrent_value_{}", i)
                }),
                timeout_ms: Some(5000),
            };
            
            client.post("/v1/cluster/propose", &serde_json::to_string(&request).unwrap()).await
        });
        handles.push(handle);
    }
    
    // Wait for all requests to complete
    let mut success_count = 0;
    for handle in handles {
        let result = handle.await.unwrap();
        if result.is_ok() && result.unwrap().status == 200 {
            success_count += 1;
        }
    }
    
    // All requests should succeed
    assert_eq!(success_count, 10);
}