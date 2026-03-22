//! Integration tests for the `GET /api/realtime` SSE endpoint.
//!
//! These tests verify:
//! - SSE connection establishment and correct Content-Type
//! - PB_CONNECT event with a unique client ID
//! - Keep-alive comments (`: ping`)
//! - Broadcast event delivery to connected clients
//! - Clean disconnect handling (client count drops on close)
//! - Multiple concurrent connections receive independent client IDs

mod common;

use std::time::Duration;

use reqwest::StatusCode;
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpListener;
use tokio::time::timeout;

use zerobase_api::{RealtimeHub, RealtimeHubConfig, RealtimeEvent, AuthInfo};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Spawn a test server that only has the realtime route mounted.
/// Returns `(base_url, hub)` so tests can broadcast events.
async fn spawn_realtime_server() -> (String, RealtimeHub) {
    spawn_realtime_server_with_config(RealtimeHubConfig::default()).await
}

/// Spawn a realtime server with custom hub configuration.
async fn spawn_realtime_server_with_config(
    config: RealtimeHubConfig,
) -> (String, RealtimeHub) {
    let hub = RealtimeHub::with_config(config);
    let router = zerobase_api::api_router()
        .merge(zerobase_api::realtime_routes(hub.clone()));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("server error");
    });

    (base_url, hub)
}

/// Read one SSE data line from a text/event-stream response body.
/// Returns (event_name, data_json) parsed from the SSE frame.
///
/// SSE frames look like:
/// ```text
/// event: PB_CONNECT
/// data: {"clientId":"abc"}
///
/// ```
async fn read_sse_event(reader: &mut tokio::io::BufReader<impl tokio::io::AsyncRead + Unpin>) -> (String, serde_json::Value) {
    let mut event_name = String::new();
    let mut data_line = String::new();

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await.expect("read error");
        if n == 0 {
            panic!("SSE stream closed unexpectedly");
        }

        let trimmed = line.trim_end();

        if trimmed.is_empty() {
            // End of SSE frame.
            if !event_name.is_empty() || !data_line.is_empty() {
                break;
            }
            continue;
        }

        // Skip SSE comments (keep-alive).
        if trimmed.starts_with(':') {
            continue;
        }

        if let Some(name) = trimmed.strip_prefix("event: ").or_else(|| trimmed.strip_prefix("event:")) {
            event_name = name.trim().to_string();
        } else if let Some(data) = trimmed.strip_prefix("data: ").or_else(|| trimmed.strip_prefix("data:")) {
            data_line = data.trim().to_string();
        }
    }

    let json: serde_json::Value = serde_json::from_str(&data_line)
        .unwrap_or_else(|e| panic!("invalid JSON in SSE data: {e}\nraw: {data_line}"));

    (event_name, json)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sse_endpoint_returns_event_stream_content_type() {
    let (base_url, _hub) = spawn_realtime_server().await;

    let resp = reqwest::Client::new()
        .get(format!("{base_url}/api/realtime"))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .expect("missing content-type")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/event-stream"),
        "expected text/event-stream, got: {content_type}"
    );
}

#[tokio::test]
async fn sse_connect_sends_pb_connect_event_with_client_id() {
    let (base_url, _hub) = spawn_realtime_server().await;

    // Use a raw TCP connection to read the SSE stream.
    let stream = tokio::net::TcpStream::connect(
        base_url.strip_prefix("http://").unwrap(),
    )
    .await
    .expect("tcp connect failed");

    // Send HTTP GET manually.
    use tokio::io::AsyncWriteExt;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
        .await
        .unwrap();

    let mut buf_reader = tokio::io::BufReader::new(reader);

    // Skip HTTP response headers (read until empty line).
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await.unwrap();
        if line.trim().is_empty() {
            break;
        }
    }

    // Read the first SSE event within 5 seconds.
    let result = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader)).await;
    let (event_name, data) = result.expect("timed out waiting for PB_CONNECT event");

    assert_eq!(event_name, "PB_CONNECT");
    assert!(
        data.get("clientId").is_some(),
        "PB_CONNECT data should contain clientId"
    );
    let client_id = data["clientId"].as_str().unwrap();
    assert!(!client_id.is_empty(), "clientId should not be empty");
}

#[tokio::test]
async fn sse_client_id_is_unique_across_connections() {
    let (base_url, _hub) = spawn_realtime_server().await;

    async fn get_client_id(base_url: &str) -> String {
        let stream = tokio::net::TcpStream::connect(
            base_url.strip_prefix("http://").unwrap(),
        )
        .await
        .unwrap();

        use tokio::io::AsyncWriteExt;
        let (reader, mut writer) = tokio::io::split(stream);
        writer
            .write_all(b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
            .await
            .unwrap();

        let mut buf_reader = tokio::io::BufReader::new(reader);
        loop {
            let mut line = String::new();
            buf_reader.read_line(&mut line).await.unwrap();
            if line.trim().is_empty() { break; }
        }

        let (_, data) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
            .await
            .expect("timed out");
        data["clientId"].as_str().unwrap().to_string()
    }

    let id1 = get_client_id(&base_url).await;
    let id2 = get_client_id(&base_url).await;
    assert_ne!(id1, id2, "each SSE connection should get a unique client ID");
}

#[tokio::test]
async fn sse_hub_tracks_connected_clients() {
    let (base_url, hub) = spawn_realtime_server().await;

    assert_eq!(hub.client_count().await, 0);

    // Connect a client.
    let _resp = reqwest::Client::new()
        .get(format!("{base_url}/api/realtime"))
        .send()
        .await
        .expect("request failed");

    // Give the server a moment to register.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(hub.client_count().await, 1);
}

#[tokio::test]
async fn sse_disconnect_removes_client_from_hub() {
    let (base_url, hub) = spawn_realtime_server().await;

    // Connect and immediately drop.
    {
        let _resp = reqwest::Client::new()
            .get(format!("{base_url}/api/realtime"))
            .send()
            .await
            .expect("request failed");
        // _resp dropped here, connection closes.
    }

    // Allow the drop guard to fire.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(hub.client_count().await, 0);
}

#[tokio::test]
async fn sse_broadcast_delivers_events_to_connected_client() {
    let (base_url, hub) = spawn_realtime_server().await;

    // Connect via raw TCP so we can read the stream.
    let stream = tokio::net::TcpStream::connect(
        base_url.strip_prefix("http://").unwrap(),
    )
    .await
    .unwrap();

    use tokio::io::AsyncWriteExt;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
        .await
        .unwrap();

    let mut buf_reader = tokio::io::BufReader::new(reader);

    // Skip HTTP headers.
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await.unwrap();
        if line.trim().is_empty() { break; }
    }

    // Read PB_CONNECT first.
    let (event_name, _) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
        .await
        .expect("timed out waiting for PB_CONNECT");
    assert_eq!(event_name, "PB_CONNECT");

    // Broadcast a custom event.
    hub.broadcast(RealtimeEvent {
        event: "collections/posts".to_string(),
        data: serde_json::json!({"action": "create", "record": {"id": "r1"}}),
        topic: String::new(),
        rules: None,
    });

    // Read the broadcast event.
    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
        .await
        .expect("timed out waiting for broadcast event");

    assert_eq!(event_name, "collections/posts");
    assert_eq!(data["action"], "create");
    assert_eq!(data["record"]["id"], "r1");
}

#[tokio::test]
async fn sse_keep_alive_sends_ping_comment() {
    // Use a very short keep-alive interval for testing.
    let config = RealtimeHubConfig {
        channel_capacity: 256,
        keep_alive_interval: Duration::from_millis(200),
    };
    let (base_url, _hub) = spawn_realtime_server_with_config(config).await;

    let stream = tokio::net::TcpStream::connect(
        base_url.strip_prefix("http://").unwrap(),
    )
    .await
    .unwrap();

    use tokio::io::AsyncWriteExt;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
        .await
        .unwrap();

    let mut buf_reader = tokio::io::BufReader::new(reader);

    // Skip HTTP headers.
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await.unwrap();
        if line.trim().is_empty() { break; }
    }

    // Read lines until we see a keep-alive comment (`: ping`).
    // We should see it within ~500ms given 200ms interval.
    let found_ping = timeout(Duration::from_secs(3), async {
        loop {
            let mut line = String::new();
            let n = buf_reader.read_line(&mut line).await.unwrap();
            if n == 0 { return false; }
            if line.trim() == ": ping" {
                return true;
            }
        }
    })
    .await
    .expect("timed out waiting for keep-alive ping");

    assert!(found_ping, "expected to receive a `: ping` keep-alive comment");
}

#[tokio::test]
async fn sse_health_endpoint_still_works_with_realtime_routes() {
    let (base_url, _hub) = spawn_realtime_server().await;

    let resp = reqwest::Client::new()
        .get(format!("{base_url}/api/health"))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "healthy");
}

// ---------------------------------------------------------------------------
// POST /api/realtime — subscription management tests
// ---------------------------------------------------------------------------

/// Represents a connected SSE client. The underlying TCP connection is held
/// alive by background tasks.
struct SseConnection {
    client_id: String,
}

/// Helper: connect an SSE client and return a handle that keeps it alive.
async fn connect_sse(base_url: &str) -> SseConnection {
    let stream = tokio::net::TcpStream::connect(
        base_url.strip_prefix("http://").unwrap(),
    )
    .await
    .unwrap();

    use tokio::io::AsyncWriteExt;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
        .await
        .unwrap();

    let mut buf_reader = tokio::io::BufReader::new(reader);
    // Skip HTTP headers.
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await.unwrap();
        if line.trim().is_empty() { break; }
    }

    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
        .await
        .expect("timed out waiting for PB_CONNECT");
    assert_eq!(event_name, "PB_CONNECT");
    let client_id = data["clientId"].as_str().unwrap().to_string();

    // We need to keep the connection alive. Spawn a task that holds the
    // read half and prevents the connection from being dropped.
    let read_half = buf_reader.into_inner();
    tokio::spawn(async move {
        // Hold the read half alive until the task is cancelled.
        let _reader = read_half;
        tokio::time::sleep(Duration::from_secs(300)).await;
    });

    // Leak the write half into a spawned task as well.
    tokio::spawn(async move {
        let _writer = writer;
        tokio::time::sleep(Duration::from_secs(300)).await;
    });

    // Give the server a moment to register the client.
    tokio::time::sleep(Duration::from_millis(50)).await;

    SseConnection {
        client_id,
    }
}

#[tokio::test]
async fn post_realtime_sets_subscriptions_successfully() {
    let (base_url, hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": ["posts", "comments", "posts/rec_123"]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(body["code"], 200);
    assert_eq!(body["message"], "Subscriptions updated.");
    assert_eq!(body["data"]["clientId"], client_id.as_str());

    let subs = body["data"]["subscriptions"].as_array().unwrap();
    assert_eq!(subs.len(), 3);

    // Verify via hub API.
    let hub_subs = hub.get_subscriptions(&client_id).await.unwrap();
    assert_eq!(hub_subs.len(), 3);
    assert!(hub_subs.contains("posts"));
    assert!(hub_subs.contains("comments"));
    assert!(hub_subs.contains("posts/rec_123"));
}

#[tokio::test]
async fn post_realtime_replaces_subscriptions() {
    let (base_url, hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let client = reqwest::Client::new();

    // First subscription set.
    client
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": ["posts", "comments"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(hub.get_subscriptions(&client_id).await.unwrap().len(), 2);

    // Replace with different set.
    client
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": ["tags"]
        }))
        .send()
        .await
        .unwrap();

    let subs = hub.get_subscriptions(&client_id).await.unwrap();
    assert_eq!(subs.len(), 1);
    assert!(subs.contains("tags"));
    assert!(!subs.contains("posts"));
}

#[tokio::test]
async fn post_realtime_empty_subscriptions_clears_all() {
    let (base_url, hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let client = reqwest::Client::new();

    // Subscribe first.
    client
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": ["posts"]
        }))
        .send()
        .await
        .unwrap();

    // Unsubscribe from everything.
    let resp = client
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(hub.get_subscriptions(&client_id).await.unwrap().is_empty());
}

#[tokio::test]
async fn post_realtime_missing_client_id_returns_400() {
    let (base_url, _hub) = spawn_realtime_server().await;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "subscriptions": ["posts"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 400);
    assert_eq!(body["message"], "clientId is required");
}

#[tokio::test]
async fn post_realtime_empty_client_id_returns_400() {
    let (base_url, _hub) = spawn_realtime_server().await;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": "",
            "subscriptions": ["posts"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["message"], "clientId is required");
}

#[tokio::test]
async fn post_realtime_unknown_client_id_returns_404() {
    let (base_url, _hub) = spawn_realtime_server().await;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": "nonexistent_id_xx",
            "subscriptions": ["posts"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], 404);
    assert_eq!(body["message"], "client connection not found");
}

#[tokio::test]
async fn post_realtime_invalid_topic_returns_400() {
    let (base_url, _hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": ["valid_topic", "invalid topic!"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["message"].as_str().unwrap().contains("invalid subscription topic"));
}

#[tokio::test]
async fn post_realtime_missing_subscriptions_returns_400() {
    let (base_url, _hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["message"], "subscriptions is required");
}

#[tokio::test]
async fn post_realtime_non_array_subscriptions_returns_400() {
    let (base_url, _hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": "not_an_array"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["message"], "subscriptions must be an array");
}

#[tokio::test]
async fn post_realtime_non_string_subscription_items_returns_400() {
    let (base_url, _hub) = spawn_realtime_server().await;
    let conn = connect_sse(&base_url).await;
    let client_id = &conn.client_id;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": client_id,
            "subscriptions": ["posts", 123]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["message"], "subscriptions must be an array of strings");
}

// ---------------------------------------------------------------------------
// End-to-end: broadcast_record_event + SSE delivery with subscription filtering
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use zerobase_core::schema::ApiRules;

fn open_rules() -> ApiRules {
    ApiRules {
        list_rule: Some(String::new()),
        view_rule: Some(String::new()),
        create_rule: Some(String::new()),
        update_rule: Some(String::new()),
        delete_rule: Some(String::new()),
        manage_rule: None,
    }
}

fn locked_rules() -> ApiRules {
    ApiRules {
        list_rule: None,
        view_rule: None,
        create_rule: None,
        update_rule: None,
        delete_rule: None,
        manage_rule: None,
    }
}

/// Spawn a realtime server and connect an SSE client. Returns the raw TCP
/// reader so we can read SSE events, plus the hub and client_id.
async fn spawn_and_connect_sse() -> (
    tokio::io::BufReader<tokio::io::ReadHalf<tokio::net::TcpStream>>,
    RealtimeHub,
    String,
) {
    let (base_url, hub) = spawn_realtime_server().await;

    let stream = tokio::net::TcpStream::connect(
        base_url.strip_prefix("http://").unwrap(),
    )
    .await
    .unwrap();

    use tokio::io::AsyncWriteExt;
    let (reader, mut writer) = tokio::io::split(stream);
    writer
        .write_all(b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n")
        .await
        .unwrap();

    let mut buf_reader = tokio::io::BufReader::new(reader);

    // Skip HTTP headers.
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await.unwrap();
        if line.trim().is_empty() { break; }
    }

    // Read PB_CONNECT.
    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
        .await
        .expect("timed out waiting for PB_CONNECT");
    assert_eq!(event_name, "PB_CONNECT");
    let client_id = data["clientId"].as_str().unwrap().to_string();

    // Keep writer alive.
    tokio::spawn(async move {
        let _writer = writer;
        tokio::time::sleep(Duration::from_secs(300)).await;
    });

    (buf_reader, hub, client_id)
}

#[tokio::test]
async fn broadcast_record_event_delivers_to_subscribed_sse_client() {
    let (mut reader, hub, client_id) = spawn_and_connect_sse().await;

    // Subscribe to "posts".
    let subs = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&client_id, subs).await.unwrap();

    // Broadcast a record create event.
    let mut record = HashMap::new();
    record.insert("id".to_string(), serde_json::json!("rec_001"));
    record.insert("title".to_string(), serde_json::json!("Hello World"));
    hub.broadcast_record_event("posts", "rec_001", "create", &record, &open_rules());

    // Read the SSE event.
    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut reader))
        .await
        .expect("timed out waiting for broadcast record event");

    assert_eq!(event_name, "posts");
    assert_eq!(data["action"], "create");
    assert_eq!(data["record"]["id"], "rec_001");
    assert_eq!(data["record"]["title"], "Hello World");
}

#[tokio::test]
async fn broadcast_record_event_not_delivered_to_unsubscribed_client() {
    let (mut reader, hub, client_id) = spawn_and_connect_sse().await;

    // Subscribe to "comments", NOT "posts".
    let subs = ["comments"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&client_id, subs).await.unwrap();

    // Broadcast a "posts" event — should be filtered out.
    let mut record = HashMap::new();
    record.insert("id".to_string(), serde_json::json!("rec_001"));
    hub.broadcast_record_event("posts", "rec_001", "create", &record, &open_rules());

    // Then broadcast a "comments" event — should be received.
    let mut comment = HashMap::new();
    comment.insert("id".to_string(), serde_json::json!("cmt_001"));
    hub.broadcast_record_event("comments", "cmt_001", "create", &comment, &open_rules());

    // The client should receive the comments event, skipping the posts one.
    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut reader))
        .await
        .expect("timed out waiting for comments event");

    assert_eq!(event_name, "comments");
    assert_eq!(data["action"], "create");
    assert_eq!(data["record"]["id"], "cmt_001");
}

#[tokio::test]
async fn broadcast_record_event_locked_rules_blocks_anon_allows_after_open() {
    let (mut reader, hub, client_id) = spawn_and_connect_sse().await;

    let subs = ["items"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&client_id, subs).await.unwrap();

    // First: locked rules event — should be filtered out for anonymous.
    let mut locked_record = HashMap::new();
    locked_record.insert("id".to_string(), serde_json::json!("locked_001"));
    hub.broadcast_record_event("items", "locked_001", "create", &locked_record, &locked_rules());

    // Second: open rules event — should pass through.
    let mut open_record = HashMap::new();
    open_record.insert("id".to_string(), serde_json::json!("open_001"));
    hub.broadcast_record_event("items", "open_001", "update", &open_record, &open_rules());

    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut reader))
        .await
        .expect("timed out waiting for open record event");

    assert_eq!(event_name, "items");
    assert_eq!(data["action"], "update");
    assert_eq!(data["record"]["id"], "open_001");
}

#[tokio::test]
async fn broadcast_record_event_delete_action_format() {
    let (mut reader, hub, client_id) = spawn_and_connect_sse().await;

    let subs = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&client_id, subs).await.unwrap();

    let mut record = HashMap::new();
    record.insert("id".to_string(), serde_json::json!("rec_del"));
    record.insert("title".to_string(), serde_json::json!("Goodbye"));
    hub.broadcast_record_event("posts", "rec_del", "delete", &record, &open_rules());

    let (event_name, data) = timeout(Duration::from_secs(5), read_sse_event(&mut reader))
        .await
        .expect("timed out waiting for delete event");

    assert_eq!(event_name, "posts");
    assert_eq!(data["action"], "delete");
    assert_eq!(data["record"]["id"], "rec_del");
    assert_eq!(data["record"]["title"], "Goodbye");
}

#[tokio::test]
async fn broadcast_record_event_collection_subscription_matches_any_record() {
    let (mut reader, hub, client_id) = spawn_and_connect_sse().await;

    // Subscribe to collection "posts" — should match any record in posts.
    let subs = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&client_id, subs).await.unwrap();

    let mut r1 = HashMap::new();
    r1.insert("id".to_string(), serde_json::json!("r1"));
    hub.broadcast_record_event("posts", "r1", "create", &r1, &open_rules());

    let (_, data1) = timeout(Duration::from_secs(5), read_sse_event(&mut reader))
        .await
        .expect("timed out");
    assert_eq!(data1["record"]["id"], "r1");

    let mut r2 = HashMap::new();
    r2.insert("id".to_string(), serde_json::json!("r2"));
    hub.broadcast_record_event("posts", "r2", "update", &r2, &open_rules());

    let (_, data2) = timeout(Duration::from_secs(5), read_sse_event(&mut reader))
        .await
        .expect("timed out");
    assert_eq!(data2["record"]["id"], "r2");
}
