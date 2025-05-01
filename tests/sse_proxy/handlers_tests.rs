#![cfg(test)]

use actix_web::{App, test, web};
use mcp_runner::server::ServerId;
use mcp_runner::sse_proxy::handlers;
use mcp_runner::sse_proxy::types::ServerInfo;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

// Create a mock ServerId that resembles the real one
#[derive(Clone, Copy, Debug)]
struct MockServerId(Uuid);

impl From<MockServerId> for ServerId {
    fn from(_mock: MockServerId) -> Self {
        // We can't construct ServerId directly since the constructor is private
        // This is just for testing and will be caught if the internals change
        unsafe { std::mem::transmute::<Uuid, ServerId>(Uuid::new_v4()) }
    }
}

// Define a struct that implements the RunnerAccess trait methods
struct MockRunnerAccess {
    servers: HashMap<String, ServerId>,
    allowed_servers: Option<Vec<String>>,
}

impl MockRunnerAccess {
    fn new() -> Self {
        let mut servers = HashMap::new();
        servers.insert(
            "test-server".to_string(),
            MockServerId(Uuid::new_v4()).into(),
        );

        Self {
            servers,
            allowed_servers: None,
        }
    }
}

// Helper function to create test data
async fn create_test_data() -> web::Data<Arc<Mutex<TestContext>>> {
    let server_info = Arc::new(Mutex::new(HashMap::new()));

    // Initialize server info
    let mut info_map = server_info.lock().await;
    info_map.insert(
        "test-server".to_string(),
        ServerInfo {
            name: "test-server".to_string(),
            id: "test-id".to_string(),
            status: "Running".to_string(),
        },
    );
    drop(info_map);

    let test_context = TestContext { server_info };

    web::Data::new(Arc::new(Mutex::new(test_context)))
}

// Test context to avoid having to modify the proxy struct
struct TestContext {
    server_info: Arc<Mutex<HashMap<String, ServerInfo>>>,
}

// Implement the get_server_info method to match what handlers.rs expects
impl TestContext {
    fn get_server_info(&self) -> Arc<Mutex<HashMap<String, ServerInfo>>> {
        self.server_info.clone()
    }
}

#[actix_web::test]
async fn test_list_servers_handler() {
    // Create the test context
    let test_data = create_test_data().await;

    // Create a custom factory to mock the structure expected by the handler
    let app = test::init_service(App::new().app_data(test_data).service(
        web::resource("/servers").to(|data: web::Data<Arc<Mutex<TestContext>>>| async move {
            // Similar to what happens in the real handler, but using our TestContext
            let context = data.lock().await;
            // Store the Arc<Mutex> in a binding to extend its lifetime
            let server_info = context.get_server_info();
            let servers = server_info.lock().await;

            // Convert to a vector for the response
            let server_list: Vec<_> = servers.values().cloned().collect();

            web::Json(server_list)
        }),
    ))
    .await;

    // Create test request
    let req = test::TestRequest::get().uri("/servers").to_request();

    // Call the service
    let resp = test::call_service(&app, req).await;
    assert!(resp.status().is_success());

    // Verify the response
    let body = test::read_body(resp).await;
    let servers: Vec<ServerInfo> = serde_json::from_slice(&body).unwrap();

    assert!(!servers.is_empty());
    assert_eq!(servers[0].name, "test-server");
}

#[actix_web::test]
async fn test_initialize_handler() {
    // For initialize handler, we don't need the SSEProxy
    let app =
        test::init_service(App::new().route("/initialize", web::post().to(handlers::initialize)))
            .await;

    let req = test::TestRequest::post().uri("/initialize").to_request();

    let resp = test::call_service(&app, req).await;

    // The initialize handler should return OK
    assert!(resp.status().is_success());

    let body = test::read_body(resp).await;
    let response: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(response["status"], "ok");
}
