//! Converts JS `routerAdd()` registrations into live axum routes.
//!
//! Each JS route handler is executed by re-evaluating the original source
//! file in a fresh Boa context on a blocking thread (Boa contexts are
//! `!Send`). During re-evaluation, `routerAdd()` is replaced with a
//! version that captures and invokes the handler function at the target
//! registration index.
//!
//! ```js
//! routerAdd("GET", "/api/custom/hello", (c) => {
//!     return c.json(200, { message: "hello" });
//! });
//! ```

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use tracing::{debug, error, warn};

use zerobase_hooks::bindings::{DaoHandler, NoOpDaoHandler};
use zerobase_hooks::JsRouteRegistration;

/// Build an axum [`Router`] from JS `routerAdd()` registrations.
///
/// Each registration becomes a live route. The handler re-evaluates the
/// original JS file source in a fresh Boa context for each request,
/// extracting and invoking the registered handler function.
pub fn build_js_routes(
    routes: Vec<JsRouteRegistration>,
    file_sources: Vec<String>,
    dao_handler: Option<Arc<dyn DaoHandler>>,
) -> Router {
    let dao: Arc<dyn DaoHandler> = dao_handler.unwrap_or_else(|| Arc::new(NoOpDaoHandler));
    let file_sources = Arc::new(file_sources);
    let mut router = Router::new();

    for reg in routes {
        let method = reg.method.to_uppercase();
        let path = reg.path.clone();
        let source_file = reg.source_file.clone();
        let dao = dao.clone();
        let file_sources = file_sources.clone();
        let reg = Arc::new(reg);

        let handler = move |req: Request<Body>| {
            let dao = dao.clone();
            let file_sources = file_sources.clone();
            let reg = reg.clone();
            async move { execute_js_route_handler(req, &reg, &file_sources, dao).await }
        };
        debug!(method = %method, path = %path, file = %source_file, "registering JS custom route");

        router = match method.as_str() {
            "GET" => router.route(&path, get(handler)),
            "POST" => router.route(&path, post(handler)),
            "PUT" => router.route(&path, put(handler)),
            "PATCH" => router.route(&path, patch(handler)),
            "DELETE" => router.route(&path, delete(handler)),
            other => {
                warn!(method = %other, path = %path, "unsupported HTTP method in JS route, skipping");
                continue;
            }
        };
    }

    router
}

/// Execute a JS route handler for a single request.
async fn execute_js_route_handler(
    req: Request<Body>,
    reg: &JsRouteRegistration,
    file_sources: &[String],
    dao_handler: Arc<dyn DaoHandler>,
) -> Response {
    // Extract request data before moving into the blocking closure.
    let uri = req.uri().to_string();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let query_string = req.uri().query().unwrap_or("").to_string();

    // Extract auth info if available (set by auth middleware).
    let auth_info = req
        .extensions()
        .get::<zerobase_api::AuthInfo>()
        .cloned();

    // Extract headers.
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str().ok().map(|val| (k.to_string(), val.to_string()))
        })
        .collect();

    // Read the body.
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = %e, "failed to read request body for JS route");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read request body").into_response();
        }
    };
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    let file_source = file_sources
        .get(reg.file_source_index)
        .cloned()
        .unwrap_or_default();
    let target_index = reg.registration_index;
    let source_file = reg.source_file.clone();

    // Execute in a blocking task (Boa is !Send).
    let result = tokio::task::spawn_blocking(move || {
        execute_js_handler_blocking(
            &file_source,
            target_index,
            &source_file,
            &method,
            &uri,
            &path,
            &query_string,
            &headers,
            &body_str,
            auth_info.as_ref(),
            dao_handler,
        )
    })
    .await;

    match result {
        Ok(Ok(js_response)) => js_response.into_response(),
        Ok(Err(e)) => {
            error!(error = %e, "JS route handler error");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("JS handler error: {e}")).into_response()
        }
        Err(e) => {
            error!(error = %e, "JS route handler task panicked");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        }
    }
}

/// The response from a JS route handler.
struct JsRouteResponse {
    status: StatusCode,
    content_type: String,
    body: String,
}

impl IntoResponse for JsRouteResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .status(self.status)
            .header("content-type", &self.content_type)
            .body(Body::from(self.body))
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Failed to build response"))
                    .unwrap()
            })
    }
}

/// Execute the JS handler by re-evaluating the source file in a fresh Boa context.
///
/// During re-evaluation, `routerAdd()` is replaced with a version that
/// captures the handler at `target_index` and invokes it with the request
/// context object `c`.
fn execute_js_handler_blocking(
    file_source: &str,
    target_index: usize,
    source_file: &str,
    method: &str,
    uri: &str,
    path: &str,
    query_string: &str,
    headers: &[(String, String)],
    body: &str,
    auth_info: Option<&zerobase_api::AuthInfo>,
    dao_handler: Arc<dyn DaoHandler>,
) -> Result<JsRouteResponse, String> {
    use boa_engine::{Context, JsNativeError, JsValue, NativeFunction, Source};
    use parking_lot::RwLock;

    let mut context = Context::default();

    // Register console for debugging.
    zerobase_hooks::bindings::register_console(&mut context);

    // Register $app bindings with DAO handler.
    let dummy_routes = Arc::new(RwLock::new(Vec::new()));
    zerobase_hooks::bindings::register_app_bindings_with_dao(
        &mut context,
        dummy_routes,
        dao_handler,
    );

    // Build the auth info JSON.
    let auth_json = if let Some(auth) = auth_info {
        serde_json::json!({
            "is_superuser": auth.is_superuser,
            "auth_record": auth.auth_record,
            "is_authenticated": auth.is_superuser || !auth.auth_record.is_empty(),
        })
    } else {
        serde_json::json!({
            "is_superuser": false,
            "auth_record": {},
            "is_authenticated": false,
        })
    };

    // Build the headers JSON.
    let headers_json: serde_json::Value = headers
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    // Parse query params.
    let query_params: serde_json::Value = url_query_to_json(query_string);

    // Escape strings for JS.
    let body_escaped = body
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    let auth_str = serde_json::to_string(&auth_json).unwrap_or_default();
    let auth_escaped = auth_str.replace('\\', "\\\\").replace('\'', "\\'");
    let headers_str = serde_json::to_string(&headers_json).unwrap_or_default();
    let headers_escaped = headers_str.replace('\\', "\\\\").replace('\'', "\\'");
    let params_str = serde_json::to_string(&query_params).unwrap_or_default();
    let params_escaped = params_str.replace('\\', "\\\\").replace('\'', "\\'");

    // Set up the request context object `c` and response storage.
    let setup_code = format!(
        r#"
        var __response = {{ status: 200, contentType: "application/json", body: "{{}}" }};
        var __body_str = '{body_escaped}';
        var __auth = JSON.parse('{auth_escaped}');
        var __headers = JSON.parse('{headers_escaped}');
        var __query_params = JSON.parse('{params_escaped}');
        var __route_call_counter = 0;
        var __target_route_index = {target_index};

        var c = {{
            method: '{method}',
            uri: '{uri}',
            path: '{path}',
            auth: __auth,
            queryParam: function(name) {{
                return __query_params[name] || '';
            }},
            header: function(name) {{
                return __headers[name.toLowerCase()] || '';
            }},
            body: function() {{
                return __body_str;
            }},
            json: function(status, data) {{
                __response.status = status || 200;
                __response.contentType = 'application/json';
                __response.body = JSON.stringify(data || {{}});
            }},
            string: function(status, text) {{
                __response.status = status || 200;
                __response.contentType = 'text/plain';
                __response.body = text || '';
            }},
            html: function(status, html) {{
                __response.status = status || 200;
                __response.contentType = 'text/html';
                __response.body = html || '';
            }},
        }};
        "#,
    );

    context
        .eval(Source::from_bytes(&setup_code))
        .map_err(|e| format!("JS route setup error: {e}"))?;

    // Replace `routerAdd` with a version that captures and invokes the
    // handler at the target registration index.
    let target_idx = target_index;
    let router_add_exec = unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            // Track which routerAdd call we're at.
            let current_count = context
                .eval(Source::from_bytes(
                    "__route_call_counter = __route_call_counter + 1; __route_call_counter",
                ))
                .unwrap_or(JsValue::from(1));

            let current_index =
                current_count.to_number(context).unwrap_or(1.0) as usize - 1;

            if current_index == target_idx {
                // This is the target route — invoke the handler with `c`.
                let handler = args.get(2).cloned().unwrap_or(JsValue::undefined());
                if handler.is_callable() {
                    let c = context
                        .eval(Source::from_bytes("c"))
                        .unwrap_or(JsValue::undefined());
                    handler
                        .as_callable()
                        .unwrap()
                        .call(&JsValue::undefined(), &[c], context)
                        .map_err(|e| {
                            JsNativeError::typ()
                                .with_message(format!("handler error: {e}"))
                        })?;
                }
            }
            // Non-target registrations are silently skipped.
            Ok(JsValue::undefined())
        })
    };

    context
        .register_global_callable(
            boa_engine::js_string!("routerAdd"),
            3,
            router_add_exec,
        )
        .expect("failed to register routerAdd execution variant");

    // Also register no-op event registration functions so the file
    // can be re-evaluated without errors.
    register_noop_event_globals(&mut context);

    // Re-evaluate the file source.
    context
        .eval(Source::from_bytes(file_source))
        .map_err(|e| {
            format!(
                "JS route handler '{}' evaluation error: {}",
                source_file, e
            )
        })?;

    // Read back the response.
    let response_json = context
        .eval(Source::from_bytes("JSON.stringify(__response)"))
        .map_err(|e| format!("Failed to read JS response: {e}"))?;

    let response_str = response_json
        .as_string()
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| {
            r#"{"status":500,"contentType":"text/plain","body":"No response"}"#.to_string()
        });

    let resp: serde_json::Value = serde_json::from_str(&response_str)
        .map_err(|e| format!("Failed to parse JS response: {e}"))?;

    let status_code = resp["status"].as_u64().unwrap_or(200) as u16;
    let content_type = resp["contentType"]
        .as_str()
        .unwrap_or("application/json")
        .to_string();
    let body = resp["body"].as_str().unwrap_or("{}").to_string();

    Ok(JsRouteResponse {
        status: StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        content_type,
        body,
    })
}

/// Register no-op versions of all event registration functions so that
/// re-evaluating a JS file for route execution doesn't fail on
/// `onRecordBeforeCreateRequest(...)` calls.
fn register_noop_event_globals(context: &mut boa_engine::Context) {
    use boa_engine::{JsValue, NativeFunction};

    let events = [
        "onRecordBeforeCreateRequest",
        "onRecordAfterCreateRequest",
        "onRecordBeforeUpdateRequest",
        "onRecordAfterUpdateRequest",
        "onRecordBeforeDeleteRequest",
        "onRecordAfterDeleteRequest",
        "onRecordBeforeViewRequest",
        "onRecordAfterViewRequest",
        "onRecordBeforeListRequest",
        "onRecordAfterListRequest",
    ];

    for event_name in events {
        let func = NativeFunction::from_copy_closure(|_this, _args, _ctx| {
            Ok(JsValue::undefined())
        });

        context
            .register_global_callable(
                boa_engine::JsString::from(event_name),
                2,
                func,
            )
            .expect("failed to register noop event function");
    }
}

/// Parse a URL query string into a JSON object.
fn url_query_to_json(query: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        if !key.is_empty() {
            map.insert(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }
    serde_json::Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tempfile::TempDir;
    use tower::ServiceExt;

    /// Helper: create a JS hooks dir, load hooks, and build routes.
    fn build_test_routes(js_code: &str) -> Router {
        build_test_routes_with_dao(js_code, None)
    }

    fn build_test_routes_with_dao(
        js_code: &str,
        dao: Option<Arc<dyn DaoHandler>>,
    ) -> Router {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("routes.pb.js"), js_code).unwrap();

        let engine = if let Some(dao) = dao.clone() {
            zerobase_hooks::JsHookEngine::with_dao_handler(dir.path(), dao)
        } else {
            zerobase_hooks::JsHookEngine::new(dir.path())
        };
        engine.load_hooks().unwrap();

        let routes = engine.custom_routes();
        let file_sources = engine.file_sources();
        build_js_routes(routes, file_sources, dao)
    }

    #[tokio::test]
    async fn js_route_returns_json_response() {
        let router = build_test_routes(
            r#"routerAdd("GET", "/api/custom/hello", function(c) {
                c.json(200, { message: "hello from JS" });
            });"#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/hello")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["message"], "hello from JS");
    }

    #[tokio::test]
    async fn js_route_post_reads_body() {
        let router = build_test_routes(
            r#"routerAdd("POST", "/api/custom/echo", function(c) {
                var body = c.body();
                c.json(200, { echo: body });
            });"#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/custom/echo")
                    .header("content-type", "text/plain")
                    .body(Body::from("test payload"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["echo"], "test payload");
    }

    #[tokio::test]
    async fn js_route_query_params() {
        let router = build_test_routes(
            r#"routerAdd("GET", "/api/custom/greet", function(c) {
                var name = c.queryParam('name');
                c.json(200, { greeting: "hello " + name });
            });"#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/greet?name=Alice")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["greeting"], "hello Alice");
    }

    #[tokio::test]
    async fn js_route_custom_status_code() {
        let router = build_test_routes(
            r#"routerAdd("GET", "/api/custom/not-found", function(c) {
                c.json(404, { error: "not found" });
            });"#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/not-found")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn js_route_string_response() {
        let router = build_test_routes(
            r#"routerAdd("GET", "/api/custom/text", function(c) {
                c.string(200, "plain text response");
            });"#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/text")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let ct = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "text/plain");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"plain text response");
    }

    #[tokio::test]
    async fn js_route_with_dao_handler() {
        use std::collections::HashMap;
        use zerobase_hooks::bindings::{DaoHandler, DaoRequest, DaoResponse};

        struct TestDao;
        impl DaoHandler for TestDao {
            fn handle(&self, request: &DaoRequest) -> DaoResponse {
                match request {
                    DaoRequest::FindById { id, .. } => {
                        let mut record = HashMap::new();
                        record.insert(
                            "id".to_string(),
                            serde_json::Value::String(id.clone()),
                        );
                        record.insert(
                            "title".to_string(),
                            serde_json::Value::String("Test Record".to_string()),
                        );
                        DaoResponse::Record(Some(record))
                    }
                    _ => DaoResponse::Record(None),
                }
            }
        }

        let router = build_test_routes_with_dao(
            r#"routerAdd("GET", "/api/custom/record", function(c) {
                var record = $app.dao().findRecordById("posts", "abc123");
                c.json(200, { title: record.title });
            });"#,
            Some(Arc::new(TestDao)),
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/record")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["title"], "Test Record");
    }

    #[tokio::test]
    async fn js_route_multiple_routes_in_one_file() {
        let router = build_test_routes(
            r#"
            routerAdd("GET", "/api/custom/items", function(c) {
                c.json(200, { action: "list" });
            });
            routerAdd("POST", "/api/custom/items", function(c) {
                c.json(201, { action: "create" });
            });
            "#,
        );

        // GET
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/custom/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["action"], "list");

        // POST
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/custom/items")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["action"], "create");
    }

    #[tokio::test]
    async fn js_route_auth_info_anonymous() {
        let router = build_test_routes(
            r#"routerAdd("GET", "/api/custom/whoami", function(c) {
                c.json(200, {
                    is_superuser: c.auth.is_superuser,
                    is_authenticated: c.auth.is_authenticated
                });
            });"#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/whoami")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["is_superuser"], false);
        assert_eq!(json["is_authenticated"], false);
    }

    #[tokio::test]
    async fn js_route_coexists_with_event_hooks() {
        // A file with both event hooks and routes.
        let router = build_test_routes(
            r#"
            onRecordBeforeCreateRequest(function(e) {
                e.record.set("touched", true);
            });
            routerAdd("GET", "/api/custom/mixed", function(c) {
                c.json(200, { mixed: true });
            });
            "#,
        );

        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/custom/mixed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["mixed"], true);
    }

    #[test]
    fn url_query_parsing() {
        let result = url_query_to_json("name=Alice&age=30");
        assert_eq!(result["name"], "Alice");
        assert_eq!(result["age"], "30");
    }

    #[test]
    fn empty_query_string() {
        let result = url_query_to_json("");
        assert!(result.as_object().unwrap().is_empty());
    }
}
