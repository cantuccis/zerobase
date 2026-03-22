//! Core JS hook engine — loads, evaluates, and executes JavaScript hooks.
//!
//! The engine creates a Boa JS context, exposes `$app` bindings, and
//! evaluates all `*.pb.js` files from the hooks directory. Hook scripts
//! register callbacks via helpers like:
//!
//! ```js
//! onRecordBeforeCreateRequest((e) => {
//!     e.record.set("computed", e.record.get("a") + e.record.get("b"));
//! }, "my_collection");
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use boa_engine::{Context, JsArgs, JsNativeError, JsValue, Source};
use parking_lot::RwLock;
use serde_json::Value;
use tracing::{debug, error, info, warn};

use zerobase_core::error::ZerobaseError;
use zerobase_core::hooks::{Hook, HookContext, HookResult, RecordOperation};

use crate::bindings::{self, DaoHandler, MailMessage, NoOpDaoHandler};
use crate::error::JsHookError;

// ── JS route registration ───────────────────────────────────────────────────

/// A custom route registered via `routerAdd()` in a JS hook file.
///
/// Since Boa doesn't preserve function source text, we store the entire
/// file source and a registration index so the handler can be re-extracted
/// by re-evaluating the file at request time.
#[derive(Debug, Clone)]
pub struct JsRouteRegistration {
    /// HTTP method (GET, POST, etc.).
    pub method: String,
    /// Route path (e.g. `/api/custom/hello`).
    pub path: String,
    /// Index into `JsHookState::file_sources` for the source file.
    pub file_source_index: usize,
    /// Which `routerAdd()` call (0-based) within this file this route is.
    pub registration_index: usize,
    /// Source file name (for diagnostics).
    pub source_file: String,
}

// ── Registered JS callback ──────────────────────────────────────────────────

/// A single JS callback registered for a specific event + optional collection filter.
#[derive(Debug, Clone)]
struct JsCallback {
    /// The event this callback listens to (e.g. "onRecordBeforeCreateRequest").
    event: String,
    /// Optional collection name filter. If `None`, fires for all collections.
    collection: Option<String>,
    /// Index into `JsHookState::file_sources` for the source file.
    file_source_index: usize,
    /// Which registration call (0-based) within this file this callback is.
    /// Used to identify the correct callback when re-evaluating the file.
    registration_index: usize,
    /// Source file name (for diagnostics).
    source_file: String,
}

/// Maps event names to record operations and phases.
fn parse_event_name(event: &str) -> Option<(RecordOperation, HookPhase)> {
    match event {
        "onRecordBeforeCreateRequest" => Some((RecordOperation::Create, HookPhase::Before)),
        "onRecordAfterCreateRequest" => Some((RecordOperation::Create, HookPhase::After)),
        "onRecordBeforeUpdateRequest" => Some((RecordOperation::Update, HookPhase::Before)),
        "onRecordAfterUpdateRequest" => Some((RecordOperation::Update, HookPhase::After)),
        "onRecordBeforeDeleteRequest" => Some((RecordOperation::Delete, HookPhase::Before)),
        "onRecordAfterDeleteRequest" => Some((RecordOperation::Delete, HookPhase::After)),
        "onRecordBeforeViewRequest" => Some((RecordOperation::View, HookPhase::Before)),
        "onRecordAfterViewRequest" => Some((RecordOperation::View, HookPhase::After)),
        "onRecordBeforeListRequest" => Some((RecordOperation::List, HookPhase::Before)),
        "onRecordAfterListRequest" => Some((RecordOperation::List, HookPhase::After)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookPhase {
    Before,
    After,
}

// ── JsHookEngine ────────────────────────────────────────────────────────────

/// Shared state for registered JS callbacks and their functions.
///
/// This is wrapped in `Arc<RwLock<...>>` so it can be shared between the
/// engine (which writes during load) and `JsHook` instances (which read
/// during execution).
#[derive(Default)]
struct JsHookState {
    /// All registered callbacks from JS files.
    callbacks: Vec<JsCallback>,
    /// The original JS file sources, indexed by `JsCallback::file_source_index`.
    /// We re-evaluate the entire file in fresh contexts for each invocation to
    /// avoid Boa's `!Send` limitation.
    file_sources: Vec<String>,
    /// Custom route registrations from JS.
    custom_routes: Vec<JsRouteRegistration>,
    /// Mail send requests queued by hooks.
    mail_queue: Vec<MailMessage>,
}

/// The main JS hook engine.
///
/// Loads `*.pb.js` files from a directory, evaluates them to collect
/// hook registrations, and produces [`JsHook`] instances that implement
/// the Rust [`Hook`] trait.
pub struct JsHookEngine {
    hooks_dir: PathBuf,
    state: Arc<RwLock<JsHookState>>,
    dao_handler: Arc<dyn DaoHandler>,
}

impl JsHookEngine {
    /// Create a new engine that loads hooks from the given directory.
    pub fn new(hooks_dir: impl Into<PathBuf>) -> Self {
        Self {
            hooks_dir: hooks_dir.into(),
            state: Arc::new(RwLock::new(JsHookState::default())),
            dao_handler: Arc::new(NoOpDaoHandler),
        }
    }

    /// Create a new engine with a custom DAO handler for database access.
    pub fn with_dao_handler(hooks_dir: impl Into<PathBuf>, dao_handler: Arc<dyn DaoHandler>) -> Self {
        Self {
            hooks_dir: hooks_dir.into(),
            state: Arc::new(RwLock::new(JsHookState::default())),
            dao_handler,
        }
    }

    /// Set the DAO handler used during callback execution.
    pub fn set_dao_handler(&mut self, dao_handler: Arc<dyn DaoHandler>) {
        self.dao_handler = dao_handler;
    }

    /// The hooks directory path.
    pub fn hooks_dir(&self) -> &Path {
        &self.hooks_dir
    }

    /// Load (or reload) all `*.pb.js` files from the hooks directory.
    ///
    /// This clears any previously loaded hooks and re-evaluates all files.
    /// Returns the number of hooks registered.
    pub fn load_hooks(&self) -> Result<usize, JsHookError> {
        let mut state = self.state.write();
        state.callbacks.clear();
        state.file_sources.clear();
        state.custom_routes.clear();

        if !self.hooks_dir.exists() {
            debug!(dir = %self.hooks_dir.display(), "hooks directory does not exist, skipping");
            return Ok(0);
        }

        let mut files = self.discover_hook_files()?;
        files.sort(); // deterministic load order

        for file_path in &files {
            if let Err(e) = self.load_single_file(file_path, &mut state) {
                error!(file = %file_path.display(), error = %e, "failed to load hook file");
                return Err(e);
            }
        }

        let count = state.callbacks.len();
        info!(
            hooks_dir = %self.hooks_dir.display(),
            files = files.len(),
            hooks = count,
            "JS hooks loaded"
        );

        Ok(count)
    }

    /// Discover all `*.pb.js` files in the hooks directory.
    fn discover_hook_files(&self) -> Result<Vec<PathBuf>, JsHookError> {
        let mut files = Vec::new();
        let entries = std::fs::read_dir(&self.hooks_dir).map_err(|e| JsHookError::FileRead {
            path: self.hooks_dir.display().to_string(),
            source: e,
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| JsHookError::FileRead {
                path: self.hooks_dir.display().to_string(),
                source: e,
            })?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("js")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |n| n.ends_with(".pb.js"))
            {
                files.push(path);
            }
        }

        Ok(files)
    }

    /// Load and evaluate a single JS file, collecting its hook registrations.
    fn load_single_file(
        &self,
        path: &Path,
        state: &mut JsHookState,
    ) -> Result<(), JsHookError> {
        let source_code = std::fs::read_to_string(path).map_err(|e| JsHookError::FileRead {
            path: path.display().to_string(),
            source: e,
        })?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        debug!(file = %file_name, "loading JS hook file");

        // Create a temporary JS context to evaluate the registration code.
        let mut context = Context::default();

        // Set up the registration collector.
        let registrations: Arc<RwLock<Vec<(String, Option<String>, String)>>> =
            Arc::new(RwLock::new(Vec::new()));
        let route_registrations: Arc<RwLock<Vec<(String, String, String)>>> =
            Arc::new(RwLock::new(Vec::new()));

        // Register all the event registration functions.
        self.register_event_globals(
            &mut context,
            registrations.clone(),
            &file_name,
        );

        // Register $app bindings.
        bindings::register_app_bindings(
            &mut context,
            route_registrations.clone(),
        );

        // Evaluate the source.
        context
            .eval(Source::from_bytes(&source_code))
            .map_err(|e| JsHookError::JsEval {
                file: file_name.clone(),
                message: e.to_string(),
            })?;

        // Store the file source for later re-evaluation.
        let file_source_index = state.file_sources.len();
        state.file_sources.push(source_code);

        // Collect registrations into state.
        let regs = registrations.read();
        for (reg_index, (event, collection, _fn_source)) in regs.iter().enumerate() {
            state.callbacks.push(JsCallback {
                event: event.clone(),
                collection: collection.clone(),
                file_source_index,
                registration_index: reg_index,
                source_file: file_name.clone(),
            });
            debug!(
                event = %event,
                collection = collection.as_deref().unwrap_or("*"),
                file = %file_name,
                "registered JS hook"
            );
        }

        // Collect route registrations with file source info.
        let routes = route_registrations.read();
        for (reg_idx, (method, path, _)) in routes.iter().enumerate() {
            state.custom_routes.push(JsRouteRegistration {
                method: method.clone(),
                path: path.clone(),
                file_source_index,
                registration_index: reg_idx,
                source_file: file_name.clone(),
            });
            debug!(
                method = %method,
                path = %path,
                file = %file_name,
                "registered JS custom route"
            );
        }

        Ok(())
    }

    /// Register the global event functions (onRecordBeforeCreateRequest, etc.)
    /// that JS scripts call to register hooks.
    fn register_event_globals(
        &self,
        context: &mut Context,
        registrations: Arc<RwLock<Vec<(String, Option<String>, String)>>>,
        _file_name: &str,
    ) {
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
            let regs = registrations.clone();
            let event = event_name.to_string();

            // SAFETY: The captured values (Arc, String) are not Boa GC-traced
            // types. They are standard Rust heap types that don't participate
            // in Boa's garbage collection, so this is safe.
            let func = unsafe {
                boa_engine::NativeFunction::from_closure(
                    move |_this, args, context| {
                        let callback = args.get_or_undefined(0);
                        let collection_filter = args.get(1).and_then(|v| {
                            v.as_string().map(|s| s.to_std_string_escaped())
                        });

                        // Extract the function source to re-evaluate later.
                        let fn_source = if callback.is_callable() {
                            callback
                                .to_string(context)?
                                .to_std_string_escaped()
                        } else {
                            return Err(JsNativeError::typ()
                                .with_message(format!(
                                    "{}: first argument must be a function",
                                    event
                                ))
                                .into());
                        };

                        let mut regs = regs.write();
                        regs.push((event.clone(), collection_filter, fn_source));

                        Ok(JsValue::undefined())
                    },
                )
            };

            context
                .register_global_callable(
                    boa_engine::JsString::from(event_name),
                    2,
                    func,
                )
                .expect("failed to register global event function");
        }
    }

    /// Produce a [`JsHook`] that implements the Rust `Hook` trait.
    ///
    /// The returned hook executes all JS callbacks that match the given
    /// record operation and collection.
    pub fn into_hook(self) -> JsHook {
        JsHook {
            state: self.state.clone(),
            dao_handler: self.dao_handler.clone(),
        }
    }

    /// Create a [`JsHook`] without consuming the engine.
    ///
    /// The hook shares the same internal state as this engine, so
    /// subsequent calls to [`load_hooks`] will update the hook's
    /// registered callbacks automatically.
    pub fn create_hook(&self) -> JsHook {
        JsHook {
            state: self.state.clone(),
            dao_handler: self.dao_handler.clone(),
        }
    }

    /// Get all registered custom routes.
    pub fn custom_routes(&self) -> Vec<JsRouteRegistration> {
        self.state.read().custom_routes.clone()
    }

    /// Get the file sources (for use by route handlers that need to
    /// re-evaluate JS files at request time).
    pub fn file_sources(&self) -> Vec<String> {
        self.state.read().file_sources.clone()
    }

    /// Drain the mail queue and return all pending mail messages.
    pub fn drain_mail_queue(&self) -> Vec<MailMessage> {
        let mut state = self.state.write();
        std::mem::take(&mut state.mail_queue)
    }

    /// Get the number of registered hooks.
    pub fn hook_count(&self) -> usize {
        self.state.read().callbacks.len()
    }

    /// List registered hook descriptions for debugging.
    pub fn registered_hooks(&self) -> Vec<String> {
        self.state
            .read()
            .callbacks
            .iter()
            .map(|cb| {
                format!(
                    "{} [{}] (from {})",
                    cb.event,
                    cb.collection.as_deref().unwrap_or("*"),
                    cb.source_file
                )
            })
            .collect()
    }
}

// ── JsHook ──────────────────────────────────────────────────────────────────

/// A [`Hook`] implementation that executes JavaScript callbacks.
///
/// Created by [`JsHookEngine::into_hook`]. This struct is `Send + Sync`
/// and can be registered in the [`HookRegistry`].
pub struct JsHook {
    state: Arc<RwLock<JsHookState>>,
    dao_handler: Arc<dyn DaoHandler>,
}

impl JsHook {
    /// Execute matching JS callbacks for a hook context.
    ///
    /// For each matching callback, we re-evaluate its entire source file in a
    /// fresh JS context. The event registration functions are replaced with
    /// versions that invoke the callback at the target `registration_index`
    /// with the event object, while skipping all other registrations.
    fn execute_callbacks(
        &self,
        ctx: &HookContext,
        phase: HookPhase,
    ) -> HookResult<Option<HashMap<String, Value>>> {
        let state = self.state.read();

        // Find matching callbacks.
        let matching: Vec<&JsCallback> = state
            .callbacks
            .iter()
            .filter(|cb| {
                let Some((op, cb_phase)) = parse_event_name(&cb.event) else {
                    return false;
                };

                if op != ctx.operation || cb_phase != phase {
                    return false;
                }

                match &cb.collection {
                    Some(coll) => coll == &ctx.collection,
                    None => true,
                }
            })
            .collect();

        if matching.is_empty() {
            return Ok(None);
        }

        let mut current_record = ctx.record.clone();
        let mut modified_record: Option<HashMap<String, Value>> = None;
        let mut accumulated_mail: Vec<MailMessage> = Vec::new();

        for callback in matching {
            let file_source = &state.file_sources[callback.file_source_index];
            let target_index = callback.registration_index;
            let target_event = callback.event.clone();

            // Build event JSON with current record data (may have been modified by prior callbacks).
            let event_json = serde_json::json!({
                "collection": ctx.collection,
                "record_id": ctx.record_id,
                "record": current_record,
                "operation": ctx.operation.to_string(),
                "auth": {
                    "is_superuser": ctx.auth.is_superuser,
                    "auth_record": ctx.auth.auth_record,
                },
                "metadata": ctx.metadata,
            });

            let event_js_str = serde_json::to_string(&event_json)
                .map_err(|e| ZerobaseError::internal(format!("JSON serialization error: {e}")))?;

            // Create a fresh JS context for each callback execution.
            let mut js_ctx = Context::default();

            // Register console for debugging.
            bindings::register_console(&mut js_ctx);

            // Set up the event object as a global before re-evaluating the file.
            let escaped_json = event_js_str.replace('\\', "\\\\").replace('\'', "\\'");
            let setup_code = format!(
                r#"
                var __event_data = JSON.parse('{escaped_json}');
                var __modified = null;
                var __http_error = null;
                var __target_event = '{target_event}';
                var __target_index = {target_index};
                var __call_counter = {{}};

                var e = {{
                    collection: __event_data.collection,
                    recordId: __event_data.record_id,
                    operation: __event_data.operation,
                    auth: __event_data.auth,
                    metadata: __event_data.metadata,
                    record: {{
                        _data: __event_data.record,
                        get: function(key) {{ return this._data[key]; }},
                        set: function(key, value) {{
                            this._data[key] = value;
                            __modified = this._data;
                        }},
                        data: function() {{ return this._data; }},
                    }},
                    httpError: function(status, message) {{
                        __http_error = {{ status: status || 400, message: message || "Hook aborted the request" }};
                        throw new Error("__HTTP_ERROR__: " + (message || "Hook aborted the request"));
                    }},
                }};
                "#,
            );

            js_ctx
                .eval(Source::from_bytes(&setup_code))
                .map_err(|e| {
                    ZerobaseError::internal(format!(
                        "JS hook '{}' setup error: {}",
                        callback.source_file, e
                    ))
                })?;

            // Register event functions that invoke the callback at the target index.
            self.register_execution_globals(&mut js_ctx, &target_event, target_index);

            // Register $app bindings with DAO handler and shared mail queue.
            let dummy_routes = Arc::new(RwLock::new(Vec::new()));
            let mail_queue: Arc<RwLock<Vec<MailMessage>>> = Arc::new(RwLock::new(Vec::new()));
            bindings::register_app_bindings_with_mail_queue(
                &mut js_ctx,
                dummy_routes,
                self.dao_handler.clone(),
                mail_queue.clone(),
            );

            // Re-evaluate the entire file. The target registration function
            // will invoke its callback with `e`.
            let eval_result = js_ctx.eval(Source::from_bytes(file_source));

            // Check if the hook called e.httpError() to abort the operation.
            let http_error_val = js_ctx
                .eval(Source::from_bytes("JSON.stringify(__http_error)"))
                .ok()
                .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()));

            if let Some(ref err_json) = http_error_val {
                if err_json != "null" && !err_json.is_empty() {
                    if let Ok(err_obj) = serde_json::from_str::<serde_json::Value>(err_json) {
                        let status = err_obj["status"].as_u64().unwrap_or(400) as u16;
                        let message = err_obj["message"]
                            .as_str()
                            .unwrap_or("Hook aborted the request")
                            .to_string();
                        debug!(
                            file = %callback.source_file,
                            event = %callback.event,
                            status = status,
                            "JS hook called httpError"
                        );
                        return Err(ZerobaseError::hook_abort(status, message));
                    }
                }
            }

            // If eval failed for reasons other than httpError, report it.
            eval_result.map_err(|e| {
                let msg = format!(
                    "JS hook '{}' (event: {}) threw: {}",
                    callback.source_file, callback.event, e
                );
                warn!("{}", msg);
                ZerobaseError::internal(msg)
            })?;

            // Read back __modified.
            let result = js_ctx
                .eval(Source::from_bytes("JSON.stringify(__modified)"))
                .map_err(|e| {
                    ZerobaseError::internal(format!(
                        "JS hook '{}' failed to read modified record: {}",
                        callback.source_file, e
                    ))
                })?;

            if let Some(result_str) = result.as_string() {
                let result_str = result_str.to_std_string_escaped();
                if result_str != "null" && !result_str.is_empty() {
                    match serde_json::from_str::<HashMap<String, Value>>(&result_str) {
                        Ok(modified) => {
                            current_record = modified.clone();
                            modified_record = Some(modified);
                        }
                        Err(e) => {
                            warn!(
                                file = %callback.source_file,
                                event = %callback.event,
                                "failed to parse modified record from JS: {e}"
                            );
                        }
                    }
                }
            }

            // Collect any mail messages queued during this callback execution.
            let queued_mail = bindings::drain_mail_queue_from(&mail_queue);
            accumulated_mail.extend(queued_mail);
        }

        // Release the read lock before acquiring write for mail.
        drop(state);

        if !accumulated_mail.is_empty() {
            let mut engine_state = self.state.write();
            engine_state.mail_queue.extend(accumulated_mail);
        }

        Ok(modified_record)
    }

    /// Register event functions for execution mode.
    ///
    /// All event functions (onRecordBefore/AfterXxxRequest) are registered.
    /// Only the one matching `target_event` at `target_index` will actually
    /// invoke its callback with the global `e` object. All others are no-ops.
    fn register_execution_globals(
        &self,
        context: &mut Context,
        target_event: &str,
        target_index: usize,
    ) {
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
            let is_target = event_name == target_event;
            let counter_key = event_name.to_string();
            let target_idx = target_index;

            // SAFETY: Captured values (String, usize, bool) don't contain
            // Boa GC-traced types.
            let func = unsafe {
                boa_engine::NativeFunction::from_closure(
                    move |_this, args, context| {
                        // Track call count for this event within the file.
                        let current_count_val = context
                            .eval(Source::from_bytes(&format!(
                                "__call_counter['{}'] = (__call_counter['{}'] || 0) + 1; __call_counter['{}']",
                                counter_key, counter_key, counter_key
                            )))
                            .unwrap_or(JsValue::from(1));

                        let current_index = current_count_val
                            .to_number(context)
                            .unwrap_or(1.0) as usize - 1;

                        if is_target && current_index == target_idx {
                            // This is the target callback — invoke it with `e`.
                            let callback = args.get_or_undefined(0);
                            if callback.is_callable() {
                                let e = context
                                    .eval(Source::from_bytes("e"))
                                    .unwrap_or(JsValue::undefined());
                                let _ = callback.as_callable().unwrap().call(
                                    &JsValue::undefined(),
                                    &[e],
                                    context,
                                );
                            }
                        }
                        // Non-target registrations are silently skipped.

                        Ok(JsValue::undefined())
                    },
                )
            };

            context
                .register_global_callable(
                    boa_engine::JsString::from(event_name),
                    2,
                    func,
                )
                .expect("failed to register execution event function");
        }
    }
}

impl Hook for JsHook {
    fn name(&self) -> &str {
        "js_hooks"
    }

    fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
        match self.execute_callbacks(ctx, HookPhase::Before) {
            Ok(Some(modified_record)) => {
                // Apply modifications from JS back to the context.
                for (key, value) in modified_record {
                    ctx.record.insert(key, value);
                }
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        match self.execute_callbacks(ctx, HookPhase::After) {
            Ok(_) => Ok(()),
            Err(e) => {
                warn!(error = %e, "JS after-hook error (non-fatal)");
                Err(e)
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;
    use zerobase_core::hooks::HookPhase as CoreHookPhase;

    fn create_hooks_dir(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            std::fs::write(dir.path().join(name), content).unwrap();
        }
        dir
    }

    fn make_ctx(op: RecordOperation, collection: &str) -> HookContext {
        HookContext::new(
            op,
            CoreHookPhase::Before,
            collection,
            "test_id",
            HashMap::new(),
        )
    }

    #[test]
    fn empty_directory_loads_zero_hooks() {
        let dir = TempDir::new().unwrap();
        let engine = JsHookEngine::new(dir.path());
        let count = engine.load_hooks().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn nonexistent_directory_loads_zero_hooks() {
        let engine = JsHookEngine::new("/tmp/nonexistent-zerobase-hooks-dir");
        let count = engine.load_hooks().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn ignores_non_pb_js_files() {
        let dir = create_hooks_dir(&[
            ("regular.js", "var x = 1;"),
            ("readme.txt", "not a hook"),
            ("hooks.pb.js", "onRecordBeforeCreateRequest(function(e) {}, 'test');"),
        ]);

        let engine = JsHookEngine::new(dir.path());
        let count = engine.load_hooks().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn loads_single_hook() {
        let dir = create_hooks_dir(&[(
            "test.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                e.record.set("injected", true);
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        let count = engine.load_hooks().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn loads_multiple_hooks_from_single_file() {
        let dir = create_hooks_dir(&[(
            "multi.pb.js",
            r#"
            onRecordBeforeCreateRequest(function(e) {});
            onRecordAfterCreateRequest(function(e) {});
            onRecordBeforeUpdateRequest(function(e) {});
            "#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        let count = engine.load_hooks().unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn loads_hooks_from_multiple_files() {
        let dir = create_hooks_dir(&[
            ("a.pb.js", "onRecordBeforeCreateRequest(function(e) {});"),
            ("b.pb.js", "onRecordAfterDeleteRequest(function(e) {});"),
        ]);

        let engine = JsHookEngine::new(dir.path());
        let count = engine.load_hooks().unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn hook_with_collection_filter() {
        let dir = create_hooks_dir(&[(
            "filtered.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                e.record.set("filtered", true);
            }, "posts");"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        // Should match: posts collection.
        let mut ctx = make_ctx(RecordOperation::Create, "posts");
        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(ctx.record.get("filtered"), Some(&Value::Bool(true)));

        // Should NOT match: users collection.
        let mut ctx = make_ctx(RecordOperation::Create, "users");
        hook.before_operation(&mut ctx).unwrap();
        assert!(ctx.record.get("filtered").is_none());
    }

    #[test]
    fn hook_without_collection_filter_matches_all() {
        let dir = create_hooks_dir(&[(
            "global.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                e.record.set("global", true);
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        // Should match any collection.
        let mut ctx = make_ctx(RecordOperation::Create, "posts");
        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(ctx.record.get("global"), Some(&Value::Bool(true)));

        let mut ctx = make_ctx(RecordOperation::Create, "users");
        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(ctx.record.get("global"), Some(&Value::Bool(true)));
    }

    #[test]
    fn before_hook_can_modify_record() {
        let dir = create_hooks_dir(&[(
            "modify.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                var name = e.record.get("name");
                e.record.set("greeting", "Hello, " + (name || "world") + "!");
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let mut ctx = make_ctx(RecordOperation::Create, "users");
        ctx.record
            .insert("name".to_string(), Value::String("Alice".to_string()));

        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(
            ctx.record.get("greeting"),
            Some(&Value::String("Hello, Alice!".to_string()))
        );
    }

    #[test]
    fn after_hook_executes_without_modifying_context() {
        let dir = create_hooks_dir(&[(
            "after.pb.js",
            r#"onRecordAfterCreateRequest(function(e) {
                // After-hook side effects (logging, notifications).
                // Modifications to e.record don't propagate back.
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let ctx = make_ctx(RecordOperation::Create, "posts");
        let result = hook.after_operation(&ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn js_syntax_error_fails_load() {
        let dir = create_hooks_dir(&[(
            "bad.pb.js",
            "onRecordBeforeCreateRequest(function(e) { this is not valid JS",
        )]);

        let engine = JsHookEngine::new(dir.path());
        let result = engine.load_hooks();
        assert!(result.is_err());
    }

    #[test]
    fn non_function_argument_fails_load() {
        let dir = create_hooks_dir(&[(
            "bad_arg.pb.js",
            r#"onRecordBeforeCreateRequest("not a function");"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        let result = engine.load_hooks();
        assert!(result.is_err());
    }

    #[test]
    fn reload_replaces_hooks() {
        let dir = create_hooks_dir(&[(
            "hooks.pb.js",
            "onRecordBeforeCreateRequest(function(e) {});",
        )]);

        let engine = JsHookEngine::new(dir.path());
        assert_eq!(engine.load_hooks().unwrap(), 1);

        // Overwrite with two hooks.
        std::fs::write(
            dir.path().join("hooks.pb.js"),
            r#"
            onRecordBeforeCreateRequest(function(e) {});
            onRecordAfterCreateRequest(function(e) {});
            "#,
        )
        .unwrap();

        assert_eq!(engine.load_hooks().unwrap(), 2);
    }

    #[test]
    fn registered_hooks_returns_descriptions() {
        let dir = create_hooks_dir(&[(
            "desc.pb.js",
            r#"
            onRecordBeforeCreateRequest(function(e) {}, "posts");
            onRecordAfterDeleteRequest(function(e) {});
            "#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();

        let descs = engine.registered_hooks();
        assert_eq!(descs.len(), 2);
        assert!(descs[0].contains("onRecordBeforeCreateRequest"));
        assert!(descs[0].contains("posts"));
        assert!(descs[1].contains("onRecordAfterDeleteRequest"));
        assert!(descs[1].contains("*"));
    }

    #[test]
    fn hook_name_is_js_hooks() {
        let dir = TempDir::new().unwrap();
        let engine = JsHookEngine::new(dir.path());
        let hook = engine.into_hook();
        assert_eq!(hook.name(), "js_hooks");
    }

    #[test]
    fn operation_mismatch_does_not_fire() {
        let dir = create_hooks_dir(&[(
            "create_only.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                e.record.set("create_hook_ran", true);
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        // Update operation should not trigger the create hook.
        let mut ctx = make_ctx(RecordOperation::Update, "posts");
        hook.before_operation(&mut ctx).unwrap();
        assert!(ctx.record.get("create_hook_ran").is_none());
    }

    #[test]
    fn hook_can_read_existing_record_data() {
        let dir = create_hooks_dir(&[(
            "compute.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                var a = e.record.get("a") || 0;
                var b = e.record.get("b") || 0;
                e.record.set("sum", a + b);
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let mut ctx = make_ctx(RecordOperation::Create, "math");
        ctx.record.insert("a".to_string(), Value::Number(3.into()));
        ctx.record.insert("b".to_string(), Value::Number(7.into()));

        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(ctx.record.get("sum"), Some(&Value::Number(10.into())));
    }

    #[test]
    fn hook_can_access_auth_info() {
        let dir = create_hooks_dir(&[(
            "auth.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                if (e.auth.is_superuser) {
                    e.record.set("created_by", "admin");
                }
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let mut ctx = make_ctx(RecordOperation::Create, "posts");
        ctx.auth = zerobase_core::hooks::HookAuthInfo::superuser();

        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(
            ctx.record.get("created_by"),
            Some(&Value::String("admin".to_string()))
        );
    }

    #[test]
    fn hook_can_access_collection_and_operation() {
        let dir = create_hooks_dir(&[(
            "meta.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                e.record.set("_collection", e.collection);
                e.record.set("_operation", e.operation);
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let mut ctx = make_ctx(RecordOperation::Create, "posts");
        hook.before_operation(&mut ctx).unwrap();

        assert_eq!(
            ctx.record.get("_collection"),
            Some(&Value::String("posts".to_string()))
        );
        assert_eq!(
            ctx.record.get("_operation"),
            Some(&Value::String("create".to_string()))
        );
    }

    #[test]
    fn multiple_hooks_same_event_all_execute() {
        let dir = create_hooks_dir(&[
            (
                "a.pb.js",
                r#"onRecordBeforeCreateRequest(function(e) {
                    e.record.set("hook_a", true);
                });"#,
            ),
            (
                "b.pb.js",
                r#"onRecordBeforeCreateRequest(function(e) {
                    e.record.set("hook_b", true);
                });"#,
            ),
        ]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let mut ctx = make_ctx(RecordOperation::Create, "posts");
        hook.before_operation(&mut ctx).unwrap();

        assert_eq!(ctx.record.get("hook_a"), Some(&Value::Bool(true)));
        assert_eq!(ctx.record.get("hook_b"), Some(&Value::Bool(true)));
    }

    #[test]
    fn hook_with_dao_handler_can_query_records() {
        use crate::bindings::{DaoHandler, DaoRequest, DaoResponse};

        struct TestDao;
        impl DaoHandler for TestDao {
            fn handle(&self, request: &DaoRequest) -> DaoResponse {
                match request {
                    DaoRequest::FindById { id, .. } => {
                        let mut record = HashMap::new();
                        record.insert("id".to_string(), Value::String(id.clone()));
                        record.insert("title".to_string(), Value::String("Found It".to_string()));
                        DaoResponse::Record(Some(record))
                    }
                    _ => DaoResponse::Record(None),
                }
            }
        }

        let dir = create_hooks_dir(&[(
            "dao_test.pb.js",
            r#"onRecordBeforeCreateRequest(function(e) {
                var related = $app.dao().findRecordById("posts", "abc123");
                if (related && related.title) {
                    e.record.set("related_title", related.title);
                }
            });"#,
        )]);

        let engine = JsHookEngine::with_dao_handler(dir.path(), Arc::new(TestDao));
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let mut ctx = make_ctx(RecordOperation::Create, "comments");
        hook.before_operation(&mut ctx).unwrap();
        assert_eq!(
            ctx.record.get("related_title"),
            Some(&Value::String("Found It".to_string()))
        );
    }

    #[test]
    fn hook_mail_messages_are_collected() {
        let dir = create_hooks_dir(&[(
            "mail_test.pb.js",
            r#"onRecordAfterCreateRequest(function(e) {
                var msg = $app.newMailMessage();
                msg.setTo("admin@example.com");
                msg.setSubject("New record created");
                msg.setBody("A new record was created in " + e.collection);
                msg.send();
            });"#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let ctx = make_ctx(RecordOperation::Create, "posts");
        hook.after_operation(&ctx).unwrap();

        let mail = hook.state.read().mail_queue.clone();
        assert_eq!(mail.len(), 1);
        assert_eq!(mail[0].to, "admin@example.com");
        assert_eq!(mail[0].subject, "New record created");
        assert!(mail[0].body.contains("posts"));
    }

    #[test]
    fn hook_dao_save_and_delete_operations() {
        use crate::bindings::{DaoHandler, DaoRequest, DaoResponse};
        use std::sync::atomic::{AtomicU32, Ordering};

        struct CountingDao {
            save_count: AtomicU32,
            delete_count: AtomicU32,
        }
        impl DaoHandler for CountingDao {
            fn handle(&self, request: &DaoRequest) -> DaoResponse {
                match request {
                    DaoRequest::Save { data, .. } => {
                        self.save_count.fetch_add(1, Ordering::SeqCst);
                        DaoResponse::Saved(data.clone())
                    }
                    DaoRequest::Delete { .. } => {
                        self.delete_count.fetch_add(1, Ordering::SeqCst);
                        DaoResponse::Deleted(true)
                    }
                    _ => DaoResponse::Record(None),
                }
            }
        }

        let dao = Arc::new(CountingDao {
            save_count: AtomicU32::new(0),
            delete_count: AtomicU32::new(0),
        });

        let dir = create_hooks_dir(&[(
            "crud.pb.js",
            r#"onRecordAfterCreateRequest(function(e) {
                $app.dao().saveRecord("audit_log", {action: "created", target: e.recordId});
                $app.dao().deleteRecord("temp_records", "old123");
            });"#,
        )]);

        let engine = JsHookEngine::with_dao_handler(dir.path(), dao.clone());
        engine.load_hooks().unwrap();
        let hook = engine.into_hook();

        let ctx = make_ctx(RecordOperation::Create, "posts");
        hook.after_operation(&ctx).unwrap();

        assert_eq!(dao.save_count.load(Ordering::SeqCst), 1);
        assert_eq!(dao.delete_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn custom_routes_collected_during_load() {
        let dir = create_hooks_dir(&[(
            "routes.pb.js",
            r#"
            routerAdd("GET", "/api/custom/health", function(req) { return "ok"; });
            routerAdd("POST", "/api/custom/webhook", function(req) { return "received"; });
            "#,
        )]);

        let engine = JsHookEngine::new(dir.path());
        engine.load_hooks().unwrap();

        let routes = engine.custom_routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/api/custom/health");
        assert_eq!(routes[1].method, "POST");
        assert_eq!(routes[1].path, "/api/custom/webhook");

        // File sources should be available for re-evaluation.
        let sources = engine.file_sources();
        assert_eq!(sources.len(), 1);
        assert!(sources[0].contains("routerAdd"));
    }
}
