//! Integration tests for realtime subscription scenarios.
//!
//! These tests go beyond basic SSE connectivity (covered in `realtime_endpoints.rs`)
//! and verify end-to-end subscription behaviour at the HTTP level:
//!
//! - Events triggered by record create / update / delete broadcasts
//! - Access rule filtering of events (open, locked, expression-based)
//! - Record-level vs collection-level subscription filtering
//! - Multiple concurrent subscriptions on different collections
//! - Reconnection behaviour (disconnect + re-connect, client count management)
//! - Subscription replacement semantics
//! - Duplicate subscription handling
//! - High-frequency broadcast delivery
//! - System event bypass of subscription filter

mod common;

use std::collections::HashSet;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

use zerobase_api::{RealtimeEvent, RealtimeHub, RealtimeHubConfig};
use zerobase_core::schema::ApiRules;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Rules where only authenticated users can view.
fn auth_required_rules() -> ApiRules {
    ApiRules {
        view_rule: Some(r#"@request.auth.id != """#.to_string()),
        list_rule: Some(r#"@request.auth.id != """#.to_string()),
        create_rule: Some(r#"@request.auth.id != """#.to_string()),
        update_rule: Some(r#"@request.auth.id != """#.to_string()),
        delete_rule: Some(r#"@request.auth.id != """#.to_string()),
        manage_rule: None,
    }
}

/// Rules where view is locked but manage_rule is open (authenticated bypass).
fn manage_open_view_locked() -> ApiRules {
    ApiRules {
        view_rule: None,
        list_rule: None,
        create_rule: None,
        update_rule: None,
        delete_rule: None,
        manage_rule: Some(String::new()), // open to any authenticated user
    }
}

fn make_record(
    id: &str,
    fields: &[(&str, serde_json::Value)],
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut record = std::collections::HashMap::new();
    record.insert("id".to_string(), serde_json::json!(id));
    for (key, val) in fields {
        record.insert(key.to_string(), val.clone());
    }
    record
}

/// Spawn a test server with only realtime routes.
async fn spawn_realtime_server() -> (String, RealtimeHub) {
    let hub = RealtimeHub::new();
    let router =
        zerobase_api::api_router().merge(zerobase_api::realtime_routes(hub.clone()));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server error");
    });

    (base_url, hub)
}

/// Spawn a realtime server with a very short keep-alive so disconnects are
/// detected quickly (useful for reconnection tests).
async fn spawn_fast_keepalive_server() -> (String, RealtimeHub) {
    let hub = RealtimeHub::with_config(RealtimeHubConfig {
        channel_capacity: 256,
        keep_alive_interval: Duration::from_millis(100),
    });
    let router =
        zerobase_api::api_router().merge(zerobase_api::realtime_routes(hub.clone()));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind");
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{port}");

    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server error");
    });

    (base_url, hub)
}

/// Read one SSE event from a buffered reader.
async fn read_sse_event(
    reader: &mut tokio::io::BufReader<impl tokio::io::AsyncRead + Unpin>,
) -> (String, serde_json::Value) {
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
            if !event_name.is_empty() || !data_line.is_empty() {
                break;
            }
            continue;
        }

        if trimmed.starts_with(':') {
            continue;
        }

        if let Some(name) = trimmed
            .strip_prefix("event: ")
            .or_else(|| trimmed.strip_prefix("event:"))
        {
            event_name = name.trim().to_string();
        } else if let Some(data) = trimmed
            .strip_prefix("data: ")
            .or_else(|| trimmed.strip_prefix("data:"))
        {
            data_line = data.trim().to_string();
        }
    }

    let json: serde_json::Value = serde_json::from_str(&data_line)
        .unwrap_or_else(|e| panic!("invalid JSON in SSE data: {e}\nraw: {data_line}"));

    (event_name, json)
}

/// Try to read an SSE event with a short timeout.
async fn try_read_sse_event(
    reader: &mut tokio::io::BufReader<impl tokio::io::AsyncRead + Unpin>,
    millis: u64,
) -> Option<(String, serde_json::Value)> {
    timeout(Duration::from_millis(millis), read_sse_event(reader))
        .await
        .ok()
}

/// An SSE connection that keeps read/write halves alive.
/// Aborts the writer task on drop to ensure clean TCP teardown.
struct LiveSseConnection {
    client_id: String,
    reader: tokio::io::BufReader<tokio::io::ReadHalf<tokio::net::TcpStream>>,
    write_handle: tokio::task::JoinHandle<()>,
}

impl Drop for LiveSseConnection {
    fn drop(&mut self) {
        self.write_handle.abort();
    }
}

impl LiveSseConnection {
    async fn connect(base_url: &str) -> Self {
        let stream =
            tokio::net::TcpStream::connect(base_url.strip_prefix("http://").unwrap())
                .await
                .expect("tcp connect failed");

        let (reader, mut writer) = tokio::io::split(stream);
        writer
            .write_all(
                b"GET /api/realtime HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n",
            )
            .await
            .unwrap();

        let mut buf_reader = tokio::io::BufReader::new(reader);

        // Skip HTTP response headers.
        loop {
            let mut line = String::new();
            buf_reader.read_line(&mut line).await.unwrap();
            if line.trim().is_empty() {
                break;
            }
        }

        // Read PB_CONNECT.
        let (event_name, data) =
            timeout(Duration::from_secs(5), read_sse_event(&mut buf_reader))
                .await
                .expect("timed out waiting for PB_CONNECT");
        assert_eq!(event_name, "PB_CONNECT");
        let client_id = data["clientId"].as_str().unwrap().to_string();

        let write_handle = tokio::spawn(async move {
            let _writer = writer;
            tokio::time::sleep(Duration::from_secs(300)).await;
        });

        // Give the server a moment to register.
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            client_id,
            reader: buf_reader,
            write_handle,
        }
    }

    async fn next_event(&mut self) -> (String, serde_json::Value) {
        timeout(Duration::from_secs(5), read_sse_event(&mut self.reader))
            .await
            .expect("timed out waiting for SSE event")
    }

    async fn try_next_event(
        &mut self,
        millis: u64,
    ) -> Option<(String, serde_json::Value)> {
        try_read_sse_event(&mut self.reader, millis).await
    }
}

// ===========================================================================
// Test: Events on record create / update / delete
// ===========================================================================

#[tokio::test]
async fn record_create_event_contains_full_record_data() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["articles"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let record = make_record(
        "art_001",
        &[
            ("title", serde_json::json!("Test Article")),
            ("body", serde_json::json!("Some content")),
            ("published", serde_json::json!(true)),
        ],
    );
    hub.broadcast_record_event("articles", "art_001", "create", &record, &open_rules());

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "articles");
    assert_eq!(data["action"], "create");
    assert_eq!(data["record"]["id"], "art_001");
    assert_eq!(data["record"]["title"], "Test Article");
    assert_eq!(data["record"]["body"], "Some content");
    assert_eq!(data["record"]["published"], true);
}

#[tokio::test]
async fn record_update_event_contains_updated_data() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["articles"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let record = make_record(
        "art_002",
        &[
            ("title", serde_json::json!("Updated Title")),
            ("version", serde_json::json!(2)),
        ],
    );
    hub.broadcast_record_event("articles", "art_002", "update", &record, &open_rules());

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "articles");
    assert_eq!(data["action"], "update");
    assert_eq!(data["record"]["id"], "art_002");
    assert_eq!(data["record"]["title"], "Updated Title");
    assert_eq!(data["record"]["version"], 2);
}

#[tokio::test]
async fn record_delete_event_contains_pre_deletion_data() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["articles"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let record = make_record(
        "art_003",
        &[("title", serde_json::json!("About To Be Deleted"))],
    );
    hub.broadcast_record_event("articles", "art_003", "delete", &record, &open_rules());

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "articles");
    assert_eq!(data["action"], "delete");
    assert_eq!(data["record"]["id"], "art_003");
    assert_eq!(data["record"]["title"], "About To Be Deleted");
}

#[tokio::test]
async fn create_update_delete_sequence_delivers_all_events() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["tasks"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let r1 = make_record("t1", &[("status", serde_json::json!("open"))]);
    hub.broadcast_record_event("tasks", "t1", "create", &r1, &open_rules());

    let r2 = make_record("t1", &[("status", serde_json::json!("in_progress"))]);
    hub.broadcast_record_event("tasks", "t1", "update", &r2, &open_rules());

    let r3 = make_record("t1", &[("status", serde_json::json!("in_progress"))]);
    hub.broadcast_record_event("tasks", "t1", "delete", &r3, &open_rules());

    let (_, d1) = conn.next_event().await;
    assert_eq!(d1["action"], "create");
    assert_eq!(d1["record"]["status"], "open");

    let (_, d2) = conn.next_event().await;
    assert_eq!(d2["action"], "update");
    assert_eq!(d2["record"]["status"], "in_progress");

    let (_, d3) = conn.next_event().await;
    assert_eq!(d3["action"], "delete");
}

// ===========================================================================
// Test: Access rule filtering via SSE (anonymous client)
// ===========================================================================

#[tokio::test]
async fn locked_rules_block_anonymous_sse_client() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["items"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // Locked event — should be filtered for anonymous.
    let locked_record = make_record("locked_001", &[]);
    hub.broadcast_record_event(
        "items",
        "locked_001",
        "create",
        &locked_record,
        &locked_rules(),
    );

    // Open event — should pass through.
    let open_record = make_record("open_001", &[]);
    hub.broadcast_record_event(
        "items",
        "open_001",
        "update",
        &open_record,
        &open_rules(),
    );

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "items");
    assert_eq!(data["action"], "update");
    assert_eq!(data["record"]["id"], "open_001");
}

#[tokio::test]
async fn auth_required_rules_block_anonymous_sse_client() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // Auth-required event — should be filtered for anonymous.
    let r1 = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r1, &auth_required_rules());

    // Open event — should arrive.
    let r2 = make_record("p2", &[]);
    hub.broadcast_record_event("posts", "p2", "create", &r2, &open_rules());

    let (_, data) = conn.next_event().await;
    assert_eq!(data["record"]["id"], "p2");
}

#[tokio::test]
async fn manage_open_view_locked_blocks_anonymous_sse_client() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["secrets"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // manage_rule open + view_rule locked — anonymous should be blocked.
    let r1 = make_record("s1", &[]);
    hub.broadcast_record_event(
        "secrets",
        "s1",
        "create",
        &r1,
        &manage_open_view_locked(),
    );

    // Open event — should arrive.
    let r2 = make_record("s2", &[]);
    hub.broadcast_record_event("secrets", "s2", "create", &r2, &open_rules());

    let (_, data) = conn.next_event().await;
    assert_eq!(data["record"]["id"], "s2");
}

#[tokio::test]
async fn mixed_rules_filter_correctly_in_sequence() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["mixed"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // 1. Open — should arrive.
    let r1 = make_record("m1", &[]);
    hub.broadcast_record_event("mixed", "m1", "create", &r1, &open_rules());

    // 2. Locked — filtered (anonymous).
    let r2 = make_record("m2", &[]);
    hub.broadcast_record_event("mixed", "m2", "create", &r2, &locked_rules());

    // 3. Open — should arrive.
    let r3 = make_record("m3", &[]);
    hub.broadcast_record_event("mixed", "m3", "update", &r3, &open_rules());

    // 4. Auth-required — filtered (anonymous).
    let r4 = make_record("m4", &[]);
    hub.broadcast_record_event("mixed", "m4", "create", &r4, &auth_required_rules());

    // 5. Open — should arrive.
    let r5 = make_record("m5", &[]);
    hub.broadcast_record_event("mixed", "m5", "delete", &r5, &open_rules());

    let (_, d1) = conn.next_event().await;
    assert_eq!(d1["record"]["id"], "m1");
    assert_eq!(d1["action"], "create");

    let (_, d3) = conn.next_event().await;
    assert_eq!(d3["record"]["id"], "m3");
    assert_eq!(d3["action"], "update");

    let (_, d5) = conn.next_event().await;
    assert_eq!(d5["record"]["id"], "m5");
    assert_eq!(d5["action"], "delete");
}

// ===========================================================================
// Test: Multi-client scenarios
// ===========================================================================

#[tokio::test]
async fn multi_client_both_receive_open_events() {
    let (base_url, hub) = spawn_realtime_server().await;

    let mut conn1 = LiveSseConnection::connect(&base_url).await;
    let mut conn2 = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["items"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn1.client_id, subs.clone())
        .await
        .unwrap();
    hub.set_subscriptions(&conn2.client_id, subs)
        .await
        .unwrap();

    assert_eq!(hub.client_count().await, 2);

    let record = make_record("i1", &[("name", serde_json::json!("Public Item"))]);
    hub.broadcast_record_event("items", "i1", "create", &record, &open_rules());

    let (_, d1) = conn1.next_event().await;
    assert_eq!(d1["record"]["id"], "i1");

    let (_, d2) = conn2.next_event().await;
    assert_eq!(d2["record"]["id"], "i1");
}

#[tokio::test]
async fn concurrent_clients_with_distinct_subscriptions_receive_correct_events() {
    let (base_url, hub) = spawn_realtime_server().await;

    let mut posts_conn = LiveSseConnection::connect(&base_url).await;
    let mut comments_conn = LiveSseConnection::connect(&base_url).await;

    let posts_subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&posts_conn.client_id, posts_subs)
        .await
        .unwrap();

    let comments_subs: HashSet<String> =
        ["comments"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&comments_conn.client_id, comments_subs)
        .await
        .unwrap();

    let rp = make_record("p1", &[("title", serde_json::json!("Post!"))]);
    hub.broadcast_record_event("posts", "p1", "create", &rp, &open_rules());

    let rc = make_record("c1", &[("body", serde_json::json!("Comment!"))]);
    hub.broadcast_record_event("comments", "c1", "create", &rc, &open_rules());

    let (e1, d1) = posts_conn.next_event().await;
    assert_eq!(e1, "posts");
    assert_eq!(d1["record"]["id"], "p1");

    let (e2, d2) = comments_conn.next_event().await;
    assert_eq!(e2, "comments");
    assert_eq!(d2["record"]["id"], "c1");

    // Posts client should NOT get the comment.
    let extra = posts_conn.try_next_event(300).await;
    assert!(extra.is_none(), "posts client should not receive comment events");
}

// ===========================================================================
// Test: Record-level subscriptions
// ===========================================================================

#[tokio::test]
async fn record_level_subscription_only_receives_matching_record() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> =
        ["posts/rec_42"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // Different record — filtered.
    let r1 = make_record("rec_99", &[("title", serde_json::json!("Other"))]);
    hub.broadcast_record_event("posts", "rec_99", "create", &r1, &open_rules());

    // Matching record — should arrive.
    let r2 = make_record("rec_42", &[("title", serde_json::json!("Target"))]);
    hub.broadcast_record_event("posts", "rec_42", "update", &r2, &open_rules());

    let (_, data) = conn.next_event().await;
    assert_eq!(data["action"], "update");
    assert_eq!(data["record"]["id"], "rec_42");
    assert_eq!(data["record"]["title"], "Target");
}

#[tokio::test]
async fn collection_level_subscription_receives_any_record_in_collection() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let r1 = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r1, &open_rules());

    let r2 = make_record("p2", &[]);
    hub.broadcast_record_event("posts", "p2", "create", &r2, &open_rules());

    let r3 = make_record("p3", &[]);
    hub.broadcast_record_event("posts", "p3", "delete", &r3, &open_rules());

    let (_, d1) = conn.next_event().await;
    assert_eq!(d1["record"]["id"], "p1");

    let (_, d2) = conn.next_event().await;
    assert_eq!(d2["record"]["id"], "p2");

    let (_, d3) = conn.next_event().await;
    assert_eq!(d3["record"]["id"], "p3");
}

// ===========================================================================
// Test: Multiple concurrent subscriptions on different collections
// ===========================================================================

#[tokio::test]
async fn client_subscribed_to_multiple_collections_receives_events_from_all() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["posts", "comments", "tags"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let r1 = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r1, &open_rules());

    let r2 = make_record("c1", &[]);
    hub.broadcast_record_event("comments", "c1", "create", &r2, &open_rules());

    let r3 = make_record("t1", &[]);
    hub.broadcast_record_event("tags", "t1", "create", &r3, &open_rules());

    let (e1, d1) = conn.next_event().await;
    assert_eq!(e1, "posts");
    assert_eq!(d1["record"]["id"], "p1");

    let (e2, d2) = conn.next_event().await;
    assert_eq!(e2, "comments");
    assert_eq!(d2["record"]["id"], "c1");

    let (e3, d3) = conn.next_event().await;
    assert_eq!(e3, "tags");
    assert_eq!(d3["record"]["id"], "t1");
}

#[tokio::test]
async fn events_from_non_subscribed_collections_are_filtered_out() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["comments"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // "posts" — filtered.
    let r1 = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r1, &open_rules());

    // "tags" — filtered.
    let r2 = make_record("t1", &[]);
    hub.broadcast_record_event("tags", "t1", "create", &r2, &open_rules());

    // "comments" — should arrive.
    let r3 = make_record("c1", &[("body", serde_json::json!("hello"))]);
    hub.broadcast_record_event("comments", "c1", "create", &r3, &open_rules());

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "comments");
    assert_eq!(data["record"]["id"], "c1");
}

// ===========================================================================
// Test: Reconnection behaviour
// ===========================================================================

#[tokio::test]
async fn reconnecting_client_gets_new_client_id() {
    let (base_url, hub) = spawn_fast_keepalive_server().await;

    let conn1 = LiveSseConnection::connect(&base_url).await;
    let id1 = conn1.client_id.clone();

    drop(conn1);
    // Wait for keep-alive to detect disconnection (100ms interval + margin).
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(hub.client_count().await, 0);

    let conn2 = LiveSseConnection::connect(&base_url).await;
    assert_ne!(id1, conn2.client_id, "reconnection should get a new client ID");
    assert_eq!(hub.client_count().await, 1);
}

#[tokio::test]
async fn reconnected_client_needs_to_resubscribe() {
    let (base_url, hub) = spawn_fast_keepalive_server().await;

    let conn1 = LiveSseConnection::connect(&base_url).await;
    let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn1.client_id, subs)
        .await
        .unwrap();
    assert!(hub
        .get_subscriptions(&conn1.client_id)
        .await
        .unwrap()
        .contains("posts"));

    let old_id = conn1.client_id.clone();
    drop(conn1);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Old client gone.
    assert!(hub.get_subscriptions(&old_id).await.is_none());

    // New client has empty subs.
    let conn2 = LiveSseConnection::connect(&base_url).await;
    let subs2 = hub.get_subscriptions(&conn2.client_id).await.unwrap();
    assert!(subs2.is_empty(), "new client should start with no subscriptions");
}

#[tokio::test]
async fn multiple_disconnect_reconnect_cycles_maintain_correct_count() {
    let (base_url, hub) = spawn_fast_keepalive_server().await;

    for i in 0..3 {
        let conn = LiveSseConnection::connect(&base_url).await;
        assert_eq!(
            hub.client_count().await,
            1,
            "cycle {i}: should have 1 client"
        );
        drop(conn);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            hub.client_count().await,
            0,
            "cycle {i}: should have 0 clients after drop"
        );
    }
}

// ===========================================================================
// Test: Subscription replacement semantics
// ===========================================================================

#[tokio::test]
async fn replacing_subscriptions_stops_events_from_old_collections() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs1: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs1)
        .await
        .unwrap();

    // Replace with "comments".
    let subs2: HashSet<String> = ["comments"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs2)
        .await
        .unwrap();

    // "posts" — filtered (no longer subscribed).
    let r1 = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r1, &open_rules());

    // "comments" — should arrive.
    let r2 = make_record("c1", &[]);
    hub.broadcast_record_event("comments", "c1", "create", &r2, &open_rules());

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "comments");
    assert_eq!(data["record"]["id"], "c1");
}

#[tokio::test]
async fn clearing_all_subscriptions_blocks_all_record_events() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["posts"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    // Clear.
    hub.set_subscriptions(&conn.client_id, HashSet::new())
        .await
        .unwrap();

    let r = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r, &open_rules());

    let maybe = conn.try_next_event(500).await;
    assert!(
        maybe.is_none(),
        "should not receive events after clearing subscriptions"
    );
}

// ===========================================================================
// Test: Duplicate subscriptions are deduplicated
// ===========================================================================

#[tokio::test]
async fn duplicate_subscriptions_via_post_endpoint_are_deduplicated() {
    let (base_url, hub) = spawn_realtime_server().await;
    let conn = LiveSseConnection::connect(&base_url).await;

    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": &conn.client_id,
            "subscriptions": ["posts", "posts", "posts", "comments"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let subs = hub.get_subscriptions(&conn.client_id).await.unwrap();
    assert_eq!(subs.len(), 2);
}

// ===========================================================================
// Test: System events bypass subscription filtering
// ===========================================================================

#[tokio::test]
async fn system_event_with_empty_topic_bypasses_subscription_filter() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    // No subscriptions at all.

    hub.broadcast(RealtimeEvent {
        event: "system_notice".to_string(),
        data: serde_json::json!({"message": "maintenance in 5 mins"}),
        topic: String::new(),
        rules: None,
    });

    let (event_name, data) = conn.next_event().await;
    assert_eq!(event_name, "system_notice");
    assert_eq!(data["message"], "maintenance in 5 mins");
}

// ===========================================================================
// Test: High-frequency broadcasts
// ===========================================================================

#[tokio::test]
async fn rapid_broadcasts_all_delivered_to_subscribed_client() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    let subs: HashSet<String> = ["events"].iter().map(|s| s.to_string()).collect();
    hub.set_subscriptions(&conn.client_id, subs)
        .await
        .unwrap();

    let count = 50;
    for i in 0..count {
        let record = make_record(&format!("e_{i}"), &[("seq", serde_json::json!(i))]);
        hub.broadcast_record_event(
            "events",
            &format!("e_{i}"),
            "create",
            &record,
            &open_rules(),
        );
    }

    for i in 0..count {
        let (_, data) = conn.next_event().await;
        assert_eq!(data["action"], "create");
        assert_eq!(data["record"]["seq"], i);
    }
}

// ===========================================================================
// Test: Empty subscriptions via POST endpoint
// ===========================================================================

#[tokio::test]
async fn post_empty_subscriptions_clears_and_returns_ok() {
    let (base_url, hub) = spawn_realtime_server().await;
    let conn = LiveSseConnection::connect(&base_url).await;

    // First subscribe.
    reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": &conn.client_id,
            "subscriptions": ["posts", "comments"]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        hub.get_subscriptions(&conn.client_id)
            .await
            .unwrap()
            .len(),
        2
    );

    // Clear with empty array.
    let resp = reqwest::Client::new()
        .post(format!("{base_url}/api/realtime"))
        .json(&serde_json::json!({
            "clientId": &conn.client_id,
            "subscriptions": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(hub
        .get_subscriptions(&conn.client_id)
        .await
        .unwrap()
        .is_empty());
}

// ===========================================================================
// Test: No events before subscribing
// ===========================================================================

#[tokio::test]
async fn no_events_before_subscribing() {
    let (base_url, hub) = spawn_realtime_server().await;
    let mut conn = LiveSseConnection::connect(&base_url).await;

    // Broadcast WITHOUT subscribing first.
    let r1 = make_record("p1", &[]);
    hub.broadcast_record_event("posts", "p1", "create", &r1, &open_rules());

    let maybe_event = conn.try_next_event(500).await;
    assert!(
        maybe_event.is_none(),
        "should not receive events without subscriptions"
    );
}

// ===========================================================================
// Test: Client count accuracy
// ===========================================================================

#[tokio::test]
async fn client_count_accurate_with_multiple_simultaneous_connections() {
    let (base_url, hub) = spawn_fast_keepalive_server().await;

    let mut connections = Vec::new();
    for _ in 0..5 {
        let conn = LiveSseConnection::connect(&base_url).await;
        connections.push(conn);
    }

    assert_eq!(hub.client_count().await, 5);

    // Drop two.
    connections.pop();
    connections.pop();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(hub.client_count().await, 3);

    // Drop all remaining.
    connections.clear();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(hub.client_count().await, 0);
}
