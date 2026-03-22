//! `$app` bindings for the JS hooks runtime.
//!
//! Exposes a global `$app` object in the JS context with sub-objects:
//!
//! - `$app.logger()` — structured logging (API compat; use `console.log` instead)
//! - `$app.dao()` — returns a DAO proxy with record CRUD operations
//! - `$app.newMailMessage()` — creates a mail message builder
//! - `routerAdd(method, path, handler)` — custom route registration
//!
//! ## DAO Operations
//!
//! The DAO proxy returned by `$app.dao()` exposes:
//!
//! - `findRecordById(collection, id)` — returns a record or `null`
//! - `findFirstRecordByFilter(collection, filter)` — returns a record or `null`
//! - `findRecordsByFilter(collection, filter, sort, limit, offset)` — returns array
//! - `saveRecord(collection, data)` — creates or updates (if `data.id` is set)
//! - `deleteRecord(collection, id)` — deletes a record
//!
//! These operations are synchronous from JS's perspective. They collect requests
//! into shared state that the Rust engine processes using the actual DB layer.
//!
//! ## Mail
//!
//! `$app.newMailMessage()` returns a builder object:
//!
//! ```js
//! const msg = $app.newMailMessage();
//! msg.setTo("user@example.com");
//! msg.setSubject("Hello");
//! msg.setBody("World");
//! msg.send(); // queues for delivery
//! ```
//!
//! These mirror PocketBase's JS hook API.

use std::collections::HashMap;
use std::sync::Arc;

use boa_engine::object::ObjectInitializer;
use boa_engine::property::Attribute;
use boa_engine::{Context, JsNativeError, JsValue, NativeFunction};
#[cfg(test)]
use boa_engine::Source;
use parking_lot::RwLock;
use serde_json::Value;
use tracing::{debug, error, info, warn};

/// A request from JS to perform a DAO operation.
#[derive(Debug, Clone)]
pub enum DaoRequest {
    FindById {
        collection: String,
        id: String,
    },
    FindByFilter {
        collection: String,
        filter: String,
    },
    FindMany {
        collection: String,
        filter: String,
        sort: String,
        limit: u64,
        offset: u64,
    },
    Save {
        collection: String,
        data: HashMap<String, Value>,
    },
    Delete {
        collection: String,
        id: String,
    },
}

/// The result of a DAO operation (returned to JS).
#[derive(Debug, Clone)]
pub enum DaoResponse {
    Record(Option<HashMap<String, Value>>),
    Records(Vec<HashMap<String, Value>>),
    Saved(HashMap<String, Value>),
    Deleted(bool),
    Error(String),
}

/// A queued mail message from JS hooks.
#[derive(Debug, Clone)]
pub struct MailMessage {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Trait for handling DAO requests synchronously during JS execution.
///
/// Implementors provide actual database access. The engine provides a default
/// implementation that collects requests for later processing.
pub trait DaoHandler: Send + Sync {
    fn handle(&self, request: &DaoRequest) -> DaoResponse;
}

/// A no-op DAO handler that returns empty results.
/// Used when no real database is available (e.g., during hook loading).
pub struct NoOpDaoHandler;

impl DaoHandler for NoOpDaoHandler {
    fn handle(&self, request: &DaoRequest) -> DaoResponse {
        warn!(?request, "DAO operation called without database handler");
        match request {
            DaoRequest::FindById { .. } | DaoRequest::FindByFilter { .. } => {
                DaoResponse::Record(None)
            }
            DaoRequest::FindMany { .. } => DaoResponse::Records(Vec::new()),
            DaoRequest::Save { data, .. } => DaoResponse::Saved(data.clone()),
            DaoRequest::Delete { .. } => DaoResponse::Deleted(false),
        }
    }
}

/// Shared state for collecting results during JS execution.
#[derive(Default)]
pub struct BindingsState {
    pub route_registrations: Vec<(String, String, String)>,
    pub mail_queue: Vec<MailMessage>,
}

/// Register the `$app` global and related bindings in the JS context.
///
/// The `dao_handler` is called synchronously when JS invokes DAO operations.
/// The `state` collects route registrations and mail requests.
pub fn register_app_bindings(
    context: &mut Context,
    route_registrations: Arc<RwLock<Vec<(String, String, String)>>>,
) {
    register_app_bindings_with_dao(context, route_registrations, Arc::new(NoOpDaoHandler));
}

/// Register the `$app` global with a real DAO handler.
///
/// The `mail_queue` parameter, if provided, collects mail messages sent by JS hooks.
/// If `None`, an internal queue is created (useful when you don't need to retrieve mail).
pub fn register_app_bindings_with_dao(
    context: &mut Context,
    route_registrations: Arc<RwLock<Vec<(String, String, String)>>>,
    dao_handler: Arc<dyn DaoHandler>,
) {
    register_app_bindings_full(context, route_registrations, dao_handler, None);
}

/// Register the `$app` global with a DAO handler and an external mail queue.
///
/// Use this variant when you need to retrieve mail messages queued during JS execution.
pub fn register_app_bindings_with_mail_queue(
    context: &mut Context,
    route_registrations: Arc<RwLock<Vec<(String, String, String)>>>,
    dao_handler: Arc<dyn DaoHandler>,
    mail_queue: Arc<RwLock<Vec<MailMessage>>>,
) {
    register_app_bindings_full(context, route_registrations, dao_handler, Some(mail_queue));
}

fn register_app_bindings_full(
    context: &mut Context,
    route_registrations: Arc<RwLock<Vec<(String, String, String)>>>,
    dao_handler: Arc<dyn DaoHandler>,
    external_mail_queue: Option<Arc<RwLock<Vec<MailMessage>>>>,
) {
    let mail_queue: Arc<RwLock<Vec<MailMessage>>> =
        external_mail_queue.unwrap_or_else(|| Arc::new(RwLock::new(Vec::new())));

    // Build the $app object.
    let dao = dao_handler.clone();
    let mq = mail_queue.clone();

    let app = ObjectInitializer::new(context).build();

    // Register $app.dao as a separate global function that returns a DAO proxy.
    // We need to register the DAO proxy builder on the $app object separately
    // because the DAO methods require captured state.
    context
        .register_global_property(
            boa_engine::js_string!("$app"),
            app,
            Attribute::READONLY | Attribute::NON_ENUMERABLE,
        )
        .expect("failed to register $app");

    // Register $app.logger() — returns a logger object with info/warn/error/debug.
    register_logger_binding(context);

    // Register $app.dao() — returns a DAO proxy object.
    register_dao_binding(context, dao);

    // Register $app.newMailMessage() — returns a mail builder.
    register_mail_binding(context, mq);

    // Register routerAdd(method, path, handler).
    register_router_add(context, route_registrations);
}

/// Register `$app.logger()` which returns a structured logger object.
///
/// The returned object has `info()`, `warn()`, `error()`, and `debug()` methods
/// that log through Rust's tracing infrastructure with a `js_hook` source tag.
fn register_logger_binding(context: &mut Context) {
    let logger_setup = r#"
        $app.logger = function() {
            return {
                info: function() {
                    var args = Array.prototype.slice.call(arguments);
                    console.log.apply(console, args);
                },
                warn: function() {
                    var args = Array.prototype.slice.call(arguments);
                    console.warn.apply(console, args);
                },
                error: function() {
                    var args = Array.prototype.slice.call(arguments);
                    console.error.apply(console, args);
                },
                debug: function() {
                    var args = Array.prototype.slice.call(arguments);
                    console.debug.apply(console, args);
                },
            };
        };
    "#;

    context
        .eval(boa_engine::Source::from_bytes(logger_setup))
        .expect("failed to wire $app.logger()");
}

/// Register `$app.dao()` which returns an object with DAO methods.
fn register_dao_binding(context: &mut Context, dao_handler: Arc<dyn DaoHandler>) {
    // We create the DAO methods as global functions prefixed with __dao_
    // and then wire them to $app.dao() return value via JS.

    // findRecordById(collection, id) -> record or null
    let dao = dao_handler.clone();
    let find_by_id = unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            let collection = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("findRecordById: collection must be a string")
                })?;

            let id = args
                .get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("findRecordById: id must be a string")
                })?;

            let response = dao.handle(&DaoRequest::FindById { collection, id });
            dao_response_to_js(response, context)
        })
    };

    context
        .register_global_callable(
            boa_engine::js_string!("__dao_findRecordById"),
            2,
            find_by_id,
        )
        .expect("failed to register __dao_findRecordById");

    // findFirstRecordByFilter(collection, filter) -> record or null
    let dao = dao_handler.clone();
    let find_by_filter = unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            let collection = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("findFirstRecordByFilter: collection must be a string")
                })?;

            let filter = args
                .get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            let response = dao.handle(&DaoRequest::FindByFilter { collection, filter });
            dao_response_to_js(response, context)
        })
    };

    context
        .register_global_callable(
            boa_engine::js_string!("__dao_findFirstRecordByFilter"),
            2,
            find_by_filter,
        )
        .expect("failed to register __dao_findFirstRecordByFilter");

    // findRecordsByFilter(collection, filter, sort, limit, offset) -> array
    let dao = dao_handler.clone();
    let find_many = unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            let collection = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message("findRecordsByFilter: collection must be a string")
                })?;

            let filter = args
                .get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            let sort = args
                .get(2)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            let limit = args
                .get(3)
                .and_then(|v| v.to_number(context).ok())
                .map(|n| n as u64)
                .unwrap_or(100);

            let offset = args
                .get(4)
                .and_then(|v| v.to_number(context).ok())
                .map(|n| n as u64)
                .unwrap_or(0);

            let response = dao.handle(&DaoRequest::FindMany {
                collection,
                filter,
                sort,
                limit,
                offset,
            });
            dao_response_to_js(response, context)
        })
    };

    context
        .register_global_callable(
            boa_engine::js_string!("__dao_findRecordsByFilter"),
            5,
            find_many,
        )
        .expect("failed to register __dao_findRecordsByFilter");

    // saveRecord(collection, data) -> saved record
    let dao = dao_handler.clone();
    let save_record = unsafe {
        NativeFunction::from_closure(move |_this, args, context| {
            let collection = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("saveRecord: collection must be a string")
                })?;

            let data_val = args.get(1).cloned().unwrap_or(JsValue::undefined());
            let data_json_str = data_val.to_json(context).map_err(|e| {
                JsNativeError::typ().with_message(format!("saveRecord: invalid data: {e}"))
            })?;

            let data: HashMap<String, Value> = match data_json_str {
                Value::Object(map) => map.into_iter().collect(),
                _ => HashMap::new(),
            };

            let response = dao.handle(&DaoRequest::Save { collection, data });
            dao_response_to_js(response, context)
        })
    };

    context
        .register_global_callable(
            boa_engine::js_string!("__dao_saveRecord"),
            2,
            save_record,
        )
        .expect("failed to register __dao_saveRecord");

    // deleteRecord(collection, id) -> boolean
    let dao = dao_handler;
    let delete_record = unsafe {
        NativeFunction::from_closure(move |_this, args, _context| {
            let collection = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("deleteRecord: collection must be a string")
                })?;

            let id = args
                .get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("deleteRecord: id must be a string")
                })?;

            let response = dao.handle(&DaoRequest::Delete { collection, id });
            match response {
                DaoResponse::Deleted(ok) => Ok(JsValue::from(ok)),
                DaoResponse::Error(msg) => Err(JsNativeError::typ()
                    .with_message(format!("deleteRecord failed: {msg}"))
                    .into()),
                _ => Ok(JsValue::from(false)),
            }
        })
    };

    context
        .register_global_callable(
            boa_engine::js_string!("__dao_deleteRecord"),
            2,
            delete_record,
        )
        .expect("failed to register __dao_deleteRecord");

    // Wire $app.dao() to return an object with the DAO methods.
    let dao_setup = r#"
        $app.dao = function() {
            return {
                findRecordById: __dao_findRecordById,
                findFirstRecordByFilter: __dao_findFirstRecordByFilter,
                findRecordsByFilter: __dao_findRecordsByFilter,
                saveRecord: __dao_saveRecord,
                deleteRecord: __dao_deleteRecord,
            };
        };
    "#;

    context
        .eval(boa_engine::Source::from_bytes(dao_setup))
        .expect("failed to wire $app.dao()");
}

/// Register `$app.newMailMessage()` which returns a mail builder object.
fn register_mail_binding(context: &mut Context, mail_queue: Arc<RwLock<Vec<MailMessage>>>) {
    let mq = mail_queue;

    // Register a global __mail_send function.
    let mq_send = mq.clone();
    let mail_send = unsafe {
        NativeFunction::from_closure(move |_this, args, _context| {
            let to = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            let subject = args
                .get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            let body = args
                .get(2)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            if to.is_empty() {
                return Err(JsNativeError::typ()
                    .with_message("mail send: 'to' address is required")
                    .into());
            }

            debug!(to = %to, subject = %subject, "JS hook queued mail message");
            mq_send.write().push(MailMessage {
                to,
                subject,
                body,
            });

            Ok(JsValue::undefined())
        })
    };

    context
        .register_global_callable(boa_engine::js_string!("__mail_send"), 3, mail_send)
        .expect("failed to register __mail_send");

    // Wire $app.newMailMessage() to return a builder.
    let mail_setup = r#"
        $app.newMailMessage = function() {
            var _to = "";
            var _subject = "";
            var _body = "";
            return {
                setTo: function(to) { _to = to; return this; },
                setSubject: function(subject) { _subject = subject; return this; },
                setBody: function(body) { _body = body; return this; },
                send: function() { __mail_send(_to, _subject, _body); },
            };
        };
    "#;

    context
        .eval(boa_engine::Source::from_bytes(mail_setup))
        .expect("failed to wire $app.newMailMessage()");
}

/// Register `routerAdd(method, path, handler)`.
///
/// Since Boa doesn't preserve function source text (toString returns
/// `[native code]`), we only store the method and path here. The actual
/// handler execution is done by re-evaluating the original file source
/// at request time, with `routerAdd` replaced by a version that captures
/// and invokes the handler.
fn register_router_add(
    context: &mut Context,
    route_registrations: Arc<RwLock<Vec<(String, String, String)>>>,
) {
    let routes = route_registrations;
    let router_add = unsafe {
        NativeFunction::from_closure(move |_this, args, _context| {
            let method = args
                .get(0)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("routerAdd: method must be a string")
                })?;

            let path = args
                .get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("routerAdd: path must be a string")
                })?;

            let _handler = args
                .get(2)
                .filter(|v| v.is_callable())
                .ok_or_else(|| {
                    JsNativeError::typ().with_message("routerAdd: handler must be a function")
                })?;

            // We store an empty string for handler_source; the actual handler
            // will be re-extracted from the file source at execution time.
            let mut routes = routes.write();
            routes.push((method, path, String::new()));

            Ok(JsValue::undefined())
        })
    };

    context
        .register_global_callable(boa_engine::js_string!("routerAdd"), 3, router_add)
        .expect("failed to register routerAdd");
}

/// Convert a `DaoResponse` into a `JsValue` for returning to JS.
fn dao_response_to_js(
    response: DaoResponse,
    context: &mut Context,
) -> Result<JsValue, boa_engine::JsError> {
    match response {
        DaoResponse::Record(Some(record)) => {
            let json_str = serde_json::to_string(&record).map_err(|e| {
                JsNativeError::typ().with_message(format!("serialization error: {e}"))
            })?;
            context
                .eval(boa_engine::Source::from_bytes(&format!(
                    "JSON.parse('{}')",
                    json_str.replace('\\', "\\\\").replace('\'', "\\'")
                )))
                .map_err(|e| {
                    JsNativeError::typ()
                        .with_message(format!("parse error: {e}"))
                        .into()
                })
        }
        DaoResponse::Record(None) => Ok(JsValue::null()),
        DaoResponse::Records(records) => {
            let json_str = serde_json::to_string(&records).map_err(|e| {
                JsNativeError::typ().with_message(format!("serialization error: {e}"))
            })?;
            context
                .eval(boa_engine::Source::from_bytes(&format!(
                    "JSON.parse('{}')",
                    json_str.replace('\\', "\\\\").replace('\'', "\\'")
                )))
                .map_err(|e| {
                    JsNativeError::typ()
                        .with_message(format!("parse error: {e}"))
                        .into()
                })
        }
        DaoResponse::Saved(record) => {
            let json_str = serde_json::to_string(&record).map_err(|e| {
                JsNativeError::typ().with_message(format!("serialization error: {e}"))
            })?;
            context
                .eval(boa_engine::Source::from_bytes(&format!(
                    "JSON.parse('{}')",
                    json_str.replace('\\', "\\\\").replace('\'', "\\'")
                )))
                .map_err(|e| {
                    JsNativeError::typ()
                        .with_message(format!("parse error: {e}"))
                        .into()
                })
        }
        DaoResponse::Deleted(ok) => Ok(JsValue::from(ok)),
        DaoResponse::Error(msg) => Err(JsNativeError::typ()
            .with_message(format!("DAO error: {msg}"))
            .into()),
    }
}

/// Register `console.log`, `console.warn`, `console.error` in the JS context.
pub fn register_console(context: &mut Context) {
    let console = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_copy_closure(|_this, args, context| {
                let msg = format_args_to_string(args, context);
                info!(source = "js_hook", "{}", msg);
                Ok(JsValue::undefined())
            }),
            boa_engine::js_string!("log"),
            1,
        )
        .function(
            NativeFunction::from_copy_closure(|_this, args, context| {
                let msg = format_args_to_string(args, context);
                warn!(source = "js_hook", "{}", msg);
                Ok(JsValue::undefined())
            }),
            boa_engine::js_string!("warn"),
            1,
        )
        .function(
            NativeFunction::from_copy_closure(|_this, args, context| {
                let msg = format_args_to_string(args, context);
                error!(source = "js_hook", "{}", msg);
                Ok(JsValue::undefined())
            }),
            boa_engine::js_string!("error"),
            1,
        )
        .function(
            NativeFunction::from_copy_closure(|_this, args, context| {
                let msg = format_args_to_string(args, context);
                debug!(source = "js_hook", "{}", msg);
                Ok(JsValue::undefined())
            }),
            boa_engine::js_string!("debug"),
            1,
        )
        .build();

    context
        .register_global_property(
            boa_engine::js_string!("console"),
            console,
            Attribute::READONLY | Attribute::NON_ENUMERABLE,
        )
        .expect("failed to register console");
}

/// Format JS arguments into a single string for logging.
fn format_args_to_string(args: &[JsValue], context: &mut Context) -> String {
    args.iter()
        .map(|arg| {
            arg.to_string(context)
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_else(|_| "[unconvertible]".to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Get the mail queue from a context's registered bindings.
///
/// This retrieves the mail queue that was registered during `register_app_bindings_with_dao`.
/// Returns messages queued during JS execution.
pub fn drain_mail_queue_from(mail_queue: &Arc<RwLock<Vec<MailMessage>>>) -> Vec<MailMessage> {
    std::mem::take(&mut *mail_queue.write())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_bindings_register_without_panic() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings(&mut context, routes);

        // $app should exist.
        let result = context
            .eval(Source::from_bytes("typeof $app"))
            .unwrap();
        assert_eq!(
            result.to_string(&mut context).unwrap().to_std_string_escaped(),
            "object"
        );
    }

    #[test]
    fn console_bindings_register_without_panic() {
        let mut context = Context::default();
        register_console(&mut context);

        // console.log should not throw.
        context
            .eval(Source::from_bytes("console.log('test message');"))
            .unwrap();
    }

    #[test]
    fn router_add_collects_routes() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings(&mut context, routes.clone());

        context
            .eval(Source::from_bytes(
                r#"routerAdd("GET", "/api/custom/hello", function(req) { return "hi"; });"#,
            ))
            .unwrap();

        let collected = routes.read();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].0, "GET");
        assert_eq!(collected[0].1, "/api/custom/hello");
        // Handler source is empty — actual source is preserved in file_sources
        // and re-extracted at execution time.
        assert!(collected[0].2.is_empty());
    }

    #[test]
    fn app_dao_returns_object_with_methods() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings(&mut context, routes);

        let result = context
            .eval(Source::from_bytes("typeof $app.dao()"))
            .unwrap();
        assert_eq!(
            result.to_string(&mut context).unwrap().to_std_string_escaped(),
            "object"
        );

        // Check that methods exist.
        let result = context
            .eval(Source::from_bytes(
                "typeof $app.dao().findRecordById",
            ))
            .unwrap();
        assert_eq!(
            result.to_string(&mut context).unwrap().to_std_string_escaped(),
            "function"
        );
    }

    #[test]
    fn dao_find_by_id_returns_null_with_no_handler() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings(&mut context, routes);

        let result = context
            .eval(Source::from_bytes(
                r#"$app.dao().findRecordById("posts", "abc123")"#,
            ))
            .unwrap();
        assert!(result.is_null());
    }

    #[test]
    fn dao_with_custom_handler() {
        struct TestDaoHandler;
        impl DaoHandler for TestDaoHandler {
            fn handle(&self, request: &DaoRequest) -> DaoResponse {
                match request {
                    DaoRequest::FindById { collection, id } => {
                        let mut record = HashMap::new();
                        record.insert("id".to_string(), Value::String(id.clone()));
                        record.insert(
                            "collection".to_string(),
                            Value::String(collection.clone()),
                        );
                        record.insert("title".to_string(), Value::String("Test Post".to_string()));
                        DaoResponse::Record(Some(record))
                    }
                    DaoRequest::FindMany { .. } => {
                        let mut r1 = HashMap::new();
                        r1.insert("id".to_string(), Value::String("1".to_string()));
                        r1.insert("title".to_string(), Value::String("First".to_string()));
                        let mut r2 = HashMap::new();
                        r2.insert("id".to_string(), Value::String("2".to_string()));
                        r2.insert("title".to_string(), Value::String("Second".to_string()));
                        DaoResponse::Records(vec![r1, r2])
                    }
                    DaoRequest::Save { data, .. } => DaoResponse::Saved(data.clone()),
                    DaoRequest::Delete { .. } => DaoResponse::Deleted(true),
                    _ => DaoResponse::Record(None),
                }
            }
        }

        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings_with_dao(
            &mut context,
            routes,
            Arc::new(TestDaoHandler),
        );

        // findRecordById should return a record.
        let result = context
            .eval(Source::from_bytes(
                r#"JSON.stringify($app.dao().findRecordById("posts", "abc123"))"#,
            ))
            .unwrap();
        let json_str = result.to_string(&mut context).unwrap().to_std_string_escaped();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["id"], "abc123");
        assert_eq!(parsed["title"], "Test Post");

        // findRecordsByFilter should return an array.
        let result = context
            .eval(Source::from_bytes(
                r#"var records = $app.dao().findRecordsByFilter("posts", "", "", 10, 0); records.length"#,
            ))
            .unwrap();
        let count = result.to_number(&mut context).unwrap();
        assert_eq!(count, 2.0);

        // deleteRecord should return true.
        let result = context
            .eval(Source::from_bytes(
                r#"$app.dao().deleteRecord("posts", "abc123")"#,
            ))
            .unwrap();
        assert_eq!(result.to_boolean(), true);
    }

    #[test]
    fn app_logger_returns_object_with_methods() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_console(&mut context);
        register_app_bindings(&mut context, routes);

        let result = context
            .eval(Source::from_bytes("typeof $app.logger()"))
            .unwrap();
        assert_eq!(
            result.to_string(&mut context).unwrap().to_std_string_escaped(),
            "object"
        );

        // Check that logger methods exist.
        let result = context
            .eval(Source::from_bytes("typeof $app.logger().info"))
            .unwrap();
        assert_eq!(
            result.to_string(&mut context).unwrap().to_std_string_escaped(),
            "function"
        );

        // Logger methods should not throw.
        context
            .eval(Source::from_bytes(
                r#"
                var log = $app.logger();
                log.info("test info");
                log.warn("test warn");
                log.error("test error");
                log.debug("test debug");
                "#,
            ))
            .unwrap();
    }

    #[test]
    fn console_log_with_multiple_args() {
        let mut context = Context::default();
        register_console(&mut context);

        // Should not throw even with multiple arguments.
        context
            .eval(Source::from_bytes(
                "console.log('hello', 'world', 42, true);",
            ))
            .unwrap();
    }

    #[test]
    fn mail_message_builder_works() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings(&mut context, routes);

        // newMailMessage should return an object with builder methods.
        let result = context
            .eval(Source::from_bytes("typeof $app.newMailMessage()"))
            .unwrap();
        assert_eq!(
            result.to_string(&mut context).unwrap().to_std_string_escaped(),
            "object"
        );

        // Build and send a message (it goes to a no-op but shouldn't throw).
        context
            .eval(Source::from_bytes(
                r#"
                var msg = $app.newMailMessage();
                msg.setTo("user@example.com");
                msg.setSubject("Test Subject");
                msg.setBody("Hello World");
                msg.send();
                "#,
            ))
            .unwrap();
    }

    #[test]
    fn mail_message_chaining() {
        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings(&mut context, routes);

        // Builder methods should be chainable.
        context
            .eval(Source::from_bytes(
                r#"
                $app.newMailMessage()
                    .setTo("user@example.com")
                    .setSubject("Test")
                    .setBody("Body")
                    .send();
                "#,
            ))
            .unwrap();
    }

    #[test]
    fn dao_save_record() {
        struct SaveTestHandler;
        impl DaoHandler for SaveTestHandler {
            fn handle(&self, request: &DaoRequest) -> DaoResponse {
                match request {
                    DaoRequest::Save { data, .. } => {
                        let mut result = data.clone();
                        result.insert(
                            "id".to_string(),
                            Value::String("generated_id".to_string()),
                        );
                        DaoResponse::Saved(result)
                    }
                    _ => DaoResponse::Record(None),
                }
            }
        }

        let mut context = Context::default();
        let routes = Arc::new(RwLock::new(Vec::new()));
        register_app_bindings_with_dao(
            &mut context,
            routes,
            Arc::new(SaveTestHandler),
        );

        let result = context
            .eval(Source::from_bytes(
                r#"JSON.stringify($app.dao().saveRecord("posts", {title: "New Post"}))"#,
            ))
            .unwrap();
        let json_str = result.to_string(&mut context).unwrap().to_std_string_escaped();
        let parsed: Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["id"], "generated_id");
        assert_eq!(parsed["title"], "New Post");
    }
}
