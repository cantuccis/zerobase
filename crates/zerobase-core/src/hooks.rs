//! Hook registration and execution system for record lifecycle events.
//!
//! Hooks allow external code to intercept and modify record operations
//! (create, update, delete, view, list) at well-defined points:
//!
//! - **Before hooks** run before the operation is persisted. They can modify
//!   the record data or abort the operation entirely.
//! - **After hooks** run after the operation has been persisted. They can
//!   perform side effects (notifications, audit logs, etc.) but cannot abort.
//!
//! # Design
//!
//! - [`Hook`] is the trait that hook implementors provide.
//! - [`HookContext`] carries mutable operation state through the hook chain.
//! - [`HookRegistry`] stores hooks and executes them in priority order.
//! - Hooks are `Send + Sync` so the registry can be shared across threads.
//!
//! # Example
//!
//! ```
//! use zerobase_core::hooks::{
//!     Hook, HookContext, HookRegistry, RecordOperation, HookResult,
//! };
//!
//! struct AuditLogger;
//!
//! impl Hook for AuditLogger {
//!     fn name(&self) -> &str { "audit_logger" }
//!
//!     fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
//!         // Log the operation (side effect)
//!         Ok(())
//!     }
//! }
//!
//! let mut registry = HookRegistry::new();
//! registry.register(AuditLogger, 100);
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::error::ZerobaseError;

// ── Types ──────────────────────────────────────────────────────────────────────

/// Result type for hook operations.
///
/// Hooks return `Ok(())` to continue the chain, or `Err(ZerobaseError)` to
/// signal a failure. For before-hooks, returning an error aborts the operation.
pub type HookResult<T> = std::result::Result<T, ZerobaseError>;

/// The kind of record operation being performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordOperation {
    /// A new record is being created.
    Create,
    /// An existing record is being updated.
    Update,
    /// A record is being deleted.
    Delete,
    /// A single record is being viewed.
    View,
    /// Records are being listed.
    List,
}

impl fmt::Display for RecordOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create => write!(f, "create"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::View => write!(f, "view"),
            Self::List => write!(f, "list"),
        }
    }
}

/// The phase within an operation where a hook fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    /// Before persistence — hooks can modify data or abort.
    Before,
    /// After persistence — hooks perform side effects.
    After,
}

impl fmt::Display for HookPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before => write!(f, "before"),
            Self::After => write!(f, "after"),
        }
    }
}

// ── HookContext ────────────────────────────────────────────────────────────────

/// Mutable context passed through the hook chain.
///
/// Hooks can read and modify the record data, inspect auth info, and set
/// custom metadata for downstream hooks. Before-hooks may also abort the
/// operation by returning an error.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// The operation being performed.
    pub operation: RecordOperation,
    /// The phase (before or after persistence).
    pub phase: HookPhase,
    /// The collection name the operation targets.
    pub collection: String,
    /// The record ID (empty string for create-before when ID isn't assigned yet,
    /// or for list operations).
    pub record_id: String,
    /// The record data. For create/update this is the full record (or merged
    /// record). For delete this is the record about to be removed. For view/list
    /// this is the record(s) being returned.
    ///
    /// Before-hooks may mutate this to change what gets persisted.
    pub record: HashMap<String, Value>,
    /// Authentication information about the caller.
    pub auth: HookAuthInfo,
    /// Arbitrary key-value metadata that hooks can use to communicate with
    /// each other within the same chain. Initialized empty.
    pub metadata: HashMap<String, Value>,
}

impl HookContext {
    /// Create a new context for an operation.
    pub fn new(
        operation: RecordOperation,
        phase: HookPhase,
        collection: impl Into<String>,
        record_id: impl Into<String>,
        record: HashMap<String, Value>,
    ) -> Self {
        Self {
            operation,
            phase,
            collection: collection.into(),
            record_id: record_id.into(),
            record,
            auth: HookAuthInfo::anonymous(),
            metadata: HashMap::new(),
        }
    }

    /// Attach authentication info to the context.
    pub fn with_auth(mut self, auth: HookAuthInfo) -> Self {
        self.auth = auth;
        self
    }

    /// Get a metadata value by key.
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Set a metadata value.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }
}

/// Authentication information available to hooks.
#[derive(Debug, Clone, Default)]
pub struct HookAuthInfo {
    /// Whether the caller is a superuser (admin).
    pub is_superuser: bool,
    /// The authenticated user's record fields. Empty if anonymous.
    pub auth_record: HashMap<String, Value>,
}

impl HookAuthInfo {
    /// Create auth info for an anonymous (unauthenticated) caller.
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Create auth info for a superuser.
    pub fn superuser() -> Self {
        Self {
            is_superuser: true,
            auth_record: HashMap::new(),
        }
    }

    /// Create auth info for an authenticated user.
    pub fn authenticated(auth_record: HashMap<String, Value>) -> Self {
        Self {
            is_superuser: false,
            auth_record,
        }
    }

    /// Whether the caller is authenticated (not anonymous).
    pub fn is_authenticated(&self) -> bool {
        self.is_superuser || !self.auth_record.is_empty()
    }
}

// ── Hook trait ─────────────────────────────────────────────────────────────────

/// Trait for hook implementations.
///
/// Implementors override the methods they care about. Default implementations
/// are no-ops that pass through successfully, so hooks only need to implement
/// the events they want to intercept.
///
/// # Before vs After
///
/// - `before_operation` is called before persistence. Returning `Err` aborts
///   the operation and the error is propagated to the caller.
/// - `after_operation` is called after persistence. Returning `Err` is logged
///   but does not roll back the already-committed operation.
///
/// # Filtering
///
/// Use [`Hook::matches`] to limit which operations/collections a hook fires for.
/// The registry calls `matches` before invoking `before_operation`/`after_operation`.
pub trait Hook: Send + Sync {
    /// A human-readable name for this hook (used in logging and debugging).
    fn name(&self) -> &str;

    /// Whether this hook should fire for the given operation and collection.
    ///
    /// The default implementation returns `true` for all operations and
    /// collections. Override to restrict when the hook fires.
    fn matches(&self, _operation: RecordOperation, _collection: &str) -> bool {
        true
    }

    /// Called before the operation is persisted.
    ///
    /// The context is mutable — hooks can modify `ctx.record` to change what
    /// gets persisted. Returning `Err` aborts the operation.
    fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
        Ok(())
    }

    /// Called after the operation has been persisted.
    ///
    /// The context is read-only at this point. Returning `Err` is logged but
    /// does not affect the committed result.
    fn after_operation(&self, _ctx: &HookContext) -> HookResult<()> {
        Ok(())
    }
}

// ── HookRegistry ──────────────────────────────────────────────────────────────

/// An entry in the registry: a hook with its priority.
struct HookEntry {
    hook: Arc<dyn Hook>,
    /// Lower number = higher priority (runs first). Default is 100.
    priority: i32,
}

/// Registry for managing and executing hooks.
///
/// Hooks are stored in priority order (lowest priority number first). When
/// executing, the registry:
///
/// 1. Filters hooks via [`Hook::matches`].
/// 2. Calls them in priority order.
/// 3. For before-hooks: stops on the first error (short-circuit).
/// 4. For after-hooks: collects all errors but does not abort.
pub struct HookRegistry {
    hooks: Vec<HookEntry>,
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    /// Create an empty hook registry.
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a hook with the given priority.
    ///
    /// Lower priority numbers execute first. The conventional default is 100.
    /// Use lower numbers (e.g., 10) for hooks that must run early (validation,
    /// authorization) and higher numbers (e.g., 200) for hooks that run late
    /// (logging, notifications).
    pub fn register(&mut self, hook: impl Hook + 'static, priority: i32) {
        self.hooks.push(HookEntry {
            hook: Arc::new(hook),
            priority,
        });
        self.hooks.sort_by_key(|e| e.priority);
    }

    /// Register a hook with the default priority (100).
    pub fn register_default(&mut self, hook: impl Hook + 'static) {
        self.register(hook, 100);
    }

    /// Remove all hooks with the given name.
    ///
    /// Returns the number of hooks removed.
    pub fn unregister(&mut self, name: &str) -> usize {
        let before = self.hooks.len();
        self.hooks.retain(|e| e.hook.name() != name);
        before - self.hooks.len()
    }

    /// The number of registered hooks.
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Whether the registry has no hooks.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// List the names of all registered hooks (in execution order).
    pub fn hook_names(&self) -> Vec<&str> {
        self.hooks.iter().map(|e| e.hook.name()).collect()
    }

    /// Execute all matching before-hooks for the given context.
    ///
    /// Hooks are called in priority order. If any hook returns an error,
    /// execution stops immediately and the error is returned (short-circuit).
    /// The context may be mutated by hooks.
    pub fn run_before(&self, ctx: &mut HookContext) -> HookResult<()> {
        for entry in &self.hooks {
            if entry.hook.matches(ctx.operation, &ctx.collection) {
                entry.hook.before_operation(ctx).map_err(|e| {
                    tracing::warn!(
                        hook = entry.hook.name(),
                        operation = %ctx.operation,
                        collection = %ctx.collection,
                        phase = "before",
                        "hook aborted operation: {e}"
                    );
                    e
                })?;
            }
        }
        Ok(())
    }

    /// Execute all matching after-hooks for the given context.
    ///
    /// Hooks are called in priority order. Errors are logged but do not stop
    /// execution — all matching hooks will run. Returns a list of errors
    /// from hooks that failed (empty if all succeeded).
    pub fn run_after(&self, ctx: &HookContext) -> Vec<ZerobaseError> {
        let mut errors = Vec::new();
        for entry in &self.hooks {
            if entry.hook.matches(ctx.operation, &ctx.collection) {
                if let Err(e) = entry.hook.after_operation(ctx) {
                    tracing::warn!(
                        hook = entry.hook.name(),
                        operation = %ctx.operation,
                        collection = %ctx.collection,
                        phase = "after",
                        "after-hook failed: {e}"
                    );
                    errors.push(e);
                }
            }
        }
        errors
    }
}

impl fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookRegistry")
            .field("hook_count", &self.hooks.len())
            .field("hooks", &self.hook_names())
            .finish()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    // ── Test helpers ─────────────────────────────────────────────────────

    /// A simple hook that records whether it was called.
    struct TrackingHook {
        name: &'static str,
        before_called: AtomicBool,
        after_called: AtomicBool,
    }

    impl TrackingHook {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                before_called: AtomicBool::new(false),
                after_called: AtomicBool::new(false),
            }
        }

        fn was_before_called(&self) -> bool {
            self.before_called.load(Ordering::SeqCst)
        }

        fn was_after_called(&self) -> bool {
            self.after_called.load(Ordering::SeqCst)
        }
    }

    impl Hook for TrackingHook {
        fn name(&self) -> &str {
            self.name
        }

        fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
            self.before_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn after_operation(&self, _ctx: &HookContext) -> HookResult<()> {
            self.after_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A hook that modifies the record data (adds a field).
    struct DataModifyingHook {
        field: &'static str,
        value: Value,
    }

    impl Hook for DataModifyingHook {
        fn name(&self) -> &str {
            "data_modifier"
        }

        fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
            ctx.record
                .insert(self.field.to_string(), self.value.clone());
            Ok(())
        }
    }

    /// A hook that aborts the operation with an error.
    struct AbortingHook {
        message: &'static str,
    }

    impl Hook for AbortingHook {
        fn name(&self) -> &str {
            "aborter"
        }

        fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
            Err(ZerobaseError::forbidden(self.message))
        }
    }

    /// A hook that only matches specific operations.
    struct FilteredHook {
        name: &'static str,
        operations: Vec<RecordOperation>,
        called_count: AtomicU32,
    }

    impl FilteredHook {
        fn new(name: &'static str, operations: Vec<RecordOperation>) -> Self {
            Self {
                name,
                operations,
                called_count: AtomicU32::new(0),
            }
        }

        fn call_count(&self) -> u32 {
            self.called_count.load(Ordering::SeqCst)
        }
    }

    impl Hook for FilteredHook {
        fn name(&self) -> &str {
            self.name
        }

        fn matches(&self, operation: RecordOperation, _collection: &str) -> bool {
            self.operations.contains(&operation)
        }

        fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
            self.called_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn after_operation(&self, _ctx: &HookContext) -> HookResult<()> {
            self.called_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A hook that only matches specific collections.
    struct CollectionFilteredHook {
        name: &'static str,
        collection: &'static str,
        called_count: AtomicU32,
    }

    impl CollectionFilteredHook {
        fn new(name: &'static str, collection: &'static str) -> Self {
            Self {
                name,
                collection,
                called_count: AtomicU32::new(0),
            }
        }

        fn call_count(&self) -> u32 {
            self.called_count.load(Ordering::SeqCst)
        }
    }

    impl Hook for CollectionFilteredHook {
        fn name(&self) -> &str {
            self.name
        }

        fn matches(&self, _operation: RecordOperation, collection: &str) -> bool {
            collection == self.collection
        }

        fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
            self.called_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A hook that records its execution order using a shared counter.
    struct OrderTrackingHook {
        name: &'static str,
        order_counter: Arc<AtomicU32>,
        recorded_order: AtomicU32,
    }

    impl OrderTrackingHook {
        fn new(name: &'static str, counter: Arc<AtomicU32>) -> Self {
            Self {
                name,
                order_counter: counter,
                recorded_order: AtomicU32::new(0),
            }
        }

        fn execution_order(&self) -> u32 {
            self.recorded_order.load(Ordering::SeqCst)
        }
    }

    impl Hook for OrderTrackingHook {
        fn name(&self) -> &str {
            self.name
        }

        fn before_operation(&self, _ctx: &mut HookContext) -> HookResult<()> {
            let order = self.order_counter.fetch_add(1, Ordering::SeqCst) + 1;
            self.recorded_order.store(order, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A hook that fails in after_operation.
    struct FailingAfterHook {
        name: &'static str,
    }

    impl Hook for FailingAfterHook {
        fn name(&self) -> &str {
            self.name
        }

        fn after_operation(&self, _ctx: &HookContext) -> HookResult<()> {
            Err(ZerobaseError::internal("after-hook failed"))
        }
    }

    fn make_ctx(operation: RecordOperation) -> HookContext {
        HookContext::new(
            operation,
            HookPhase::Before,
            "test_collection",
            "test_id",
            HashMap::new(),
        )
    }

    // ── RecordOperation tests ────────────────────────────────────────────

    #[test]
    fn record_operation_display() {
        assert_eq!(RecordOperation::Create.to_string(), "create");
        assert_eq!(RecordOperation::Update.to_string(), "update");
        assert_eq!(RecordOperation::Delete.to_string(), "delete");
        assert_eq!(RecordOperation::View.to_string(), "view");
        assert_eq!(RecordOperation::List.to_string(), "list");
    }

    #[test]
    fn hook_phase_display() {
        assert_eq!(HookPhase::Before.to_string(), "before");
        assert_eq!(HookPhase::After.to_string(), "after");
    }

    // ── HookContext tests ────────────────────────────────────────────────

    #[test]
    fn context_new_sets_fields() {
        let mut record = HashMap::new();
        record.insert("title".to_string(), Value::String("hello".to_string()));

        let ctx = HookContext::new(
            RecordOperation::Create,
            HookPhase::Before,
            "posts",
            "abc123",
            record,
        );

        assert_eq!(ctx.operation, RecordOperation::Create);
        assert_eq!(ctx.phase, HookPhase::Before);
        assert_eq!(ctx.collection, "posts");
        assert_eq!(ctx.record_id, "abc123");
        assert_eq!(
            ctx.record.get("title"),
            Some(&Value::String("hello".to_string()))
        );
        assert!(!ctx.auth.is_authenticated());
    }

    #[test]
    fn context_with_auth() {
        let ctx = make_ctx(RecordOperation::Create).with_auth(HookAuthInfo::superuser());
        assert!(ctx.auth.is_superuser);
        assert!(ctx.auth.is_authenticated());
    }

    #[test]
    fn context_metadata() {
        let mut ctx = make_ctx(RecordOperation::Create);
        assert!(ctx.get_metadata("key").is_none());

        ctx.set_metadata("key", Value::String("value".to_string()));
        assert_eq!(
            ctx.get_metadata("key"),
            Some(&Value::String("value".to_string()))
        );
    }

    #[test]
    fn auth_info_anonymous() {
        let auth = HookAuthInfo::anonymous();
        assert!(!auth.is_superuser);
        assert!(!auth.is_authenticated());
    }

    #[test]
    fn auth_info_authenticated() {
        let mut record = HashMap::new();
        record.insert("id".to_string(), Value::String("user1".to_string()));
        let auth = HookAuthInfo::authenticated(record);
        assert!(!auth.is_superuser);
        assert!(auth.is_authenticated());
    }

    // ── HookRegistry basic tests ─────────────────────────────────────────

    #[test]
    fn new_registry_is_empty() {
        let registry = HookRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.hook_names().is_empty());
    }

    #[test]
    fn default_creates_empty_registry() {
        let registry = HookRegistry::default();
        assert!(registry.is_empty());
    }

    #[test]
    fn register_adds_hooks() {
        let mut registry = HookRegistry::new();
        registry.register_default(TrackingHook::new("hook1"));
        registry.register_default(TrackingHook::new("hook2"));

        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
    }

    #[test]
    fn hook_names_returns_all_names() {
        let mut registry = HookRegistry::new();
        registry.register(TrackingHook::new("alpha"), 50);
        registry.register(TrackingHook::new("beta"), 100);
        registry.register(TrackingHook::new("gamma"), 10);

        // Sorted by priority
        assert_eq!(registry.hook_names(), vec!["gamma", "alpha", "beta"]);
    }

    #[test]
    fn unregister_removes_hook() {
        let mut registry = HookRegistry::new();
        registry.register_default(TrackingHook::new("keep"));
        registry.register_default(TrackingHook::new("remove"));
        registry.register_default(TrackingHook::new("keep_too"));

        let removed = registry.unregister("remove");
        assert_eq!(removed, 1);
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.hook_names(), vec!["keep", "keep_too"]);
    }

    #[test]
    fn unregister_nonexistent_returns_zero() {
        let mut registry = HookRegistry::new();
        registry.register_default(TrackingHook::new("existing"));
        assert_eq!(registry.unregister("nonexistent"), 0);
        assert_eq!(registry.len(), 1);
    }

    // ── Before-hook execution ────────────────────────────────────────────

    #[test]
    fn run_before_calls_matching_hooks() {
        let hook = Arc::new(TrackingHook::new("tracker"));
        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();

        assert!(hook.was_before_called());
    }

    #[test]
    fn run_before_can_modify_record_data() {
        let mut registry = HookRegistry::new();
        registry.register_default(DataModifyingHook {
            field: "injected",
            value: Value::Bool(true),
        });

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();

        assert_eq!(ctx.record.get("injected"), Some(&Value::Bool(true)));
    }

    #[test]
    fn run_before_abort_stops_chain() {
        let hook_after_abort = Arc::new(TrackingHook::new("after_abort"));

        let mut registry = HookRegistry::new();
        registry.register(
            AbortingHook {
                message: "not allowed",
            },
            10,
        );
        registry.hooks.push(HookEntry {
            hook: hook_after_abort.clone(),
            priority: 20,
        });

        let mut ctx = make_ctx(RecordOperation::Create);
        let result = registry.run_before(&mut ctx);

        assert!(result.is_err());
        // The hook after the aborting hook should NOT have been called.
        assert!(!hook_after_abort.was_before_called());
    }

    #[test]
    fn run_before_abort_returns_original_error() {
        let mut registry = HookRegistry::new();
        registry.register_default(AbortingHook {
            message: "custom abort message",
        });

        let mut ctx = make_ctx(RecordOperation::Update);
        let err = registry.run_before(&mut ctx).unwrap_err();

        assert_eq!(err.status_code(), 403); // Forbidden
        assert!(err.to_string().contains("custom abort message"));
    }

    // ── After-hook execution ─────────────────────────────────────────────

    #[test]
    fn run_after_calls_matching_hooks() {
        let hook = Arc::new(TrackingHook::new("tracker"));
        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        let ctx = make_ctx(RecordOperation::Create);
        let errors = registry.run_after(&ctx);

        assert!(errors.is_empty());
        assert!(hook.was_after_called());
    }

    #[test]
    fn run_after_collects_errors_without_stopping() {
        let mut registry = HookRegistry::new();
        registry.register(FailingAfterHook { name: "fail1" }, 10);
        registry.register(FailingAfterHook { name: "fail2" }, 20);

        let ctx = make_ctx(RecordOperation::Delete);
        let errors = registry.run_after(&ctx);

        // Both hooks ran despite errors.
        assert_eq!(errors.len(), 2);
    }

    // ── Priority ordering ────────────────────────────────────────────────

    #[test]
    fn hooks_execute_in_priority_order() {
        let counter = Arc::new(AtomicU32::new(0));
        let hook_a = Arc::new(OrderTrackingHook::new("low_priority", counter.clone()));
        let hook_b = Arc::new(OrderTrackingHook::new("high_priority", counter.clone()));
        let hook_c = Arc::new(OrderTrackingHook::new("medium_priority", counter.clone()));

        let mut registry = HookRegistry::new();
        // Register out of order to prove sorting works.
        registry.hooks.push(HookEntry {
            hook: hook_a.clone(),
            priority: 200,
        });
        registry.hooks.push(HookEntry {
            hook: hook_b.clone(),
            priority: 10,
        });
        registry.hooks.push(HookEntry {
            hook: hook_c.clone(),
            priority: 100,
        });
        registry.hooks.sort_by_key(|e| e.priority);

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();

        assert_eq!(hook_b.execution_order(), 1); // priority 10, runs first
        assert_eq!(hook_c.execution_order(), 2); // priority 100, runs second
        assert_eq!(hook_a.execution_order(), 3); // priority 200, runs third
    }

    // ── Filtering by operation ───────────────────────────────────────────

    #[test]
    fn filtered_hook_only_fires_for_matching_operations() {
        let hook = Arc::new(FilteredHook::new(
            "create_only",
            vec![RecordOperation::Create],
        ));

        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        // Should match: Create
        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 1);

        // Should NOT match: Update
        let mut ctx = make_ctx(RecordOperation::Update);
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 1); // still 1

        // Should NOT match: Delete
        let mut ctx = make_ctx(RecordOperation::Delete);
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 1); // still 1
    }

    #[test]
    fn filtered_hook_matches_multiple_operations() {
        let hook = Arc::new(FilteredHook::new(
            "create_update",
            vec![RecordOperation::Create, RecordOperation::Update],
        ));

        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 1);

        let mut ctx = make_ctx(RecordOperation::Update);
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 2);
    }

    // ── Filtering by collection ──────────────────────────────────────────

    #[test]
    fn collection_filtered_hook_only_fires_for_matching_collection() {
        let hook = Arc::new(CollectionFilteredHook::new("posts_only", "posts"));

        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        // Match: "posts" collection
        let mut ctx = HookContext::new(
            RecordOperation::Create,
            HookPhase::Before,
            "posts",
            "",
            HashMap::new(),
        );
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 1);

        // No match: "users" collection
        let mut ctx = HookContext::new(
            RecordOperation::Create,
            HookPhase::Before,
            "users",
            "",
            HashMap::new(),
        );
        registry.run_before(&mut ctx).unwrap();
        assert_eq!(hook.call_count(), 1); // still 1
    }

    // ── Data modification chain ──────────────────────────────────────────

    #[test]
    fn multiple_hooks_can_modify_data_sequentially() {
        let mut registry = HookRegistry::new();
        registry.register(
            DataModifyingHook {
                field: "field_a",
                value: Value::Number(1.into()),
            },
            10,
        );
        registry.register(
            DataModifyingHook {
                field: "field_b",
                value: Value::Number(2.into()),
            },
            20,
        );

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();

        assert_eq!(ctx.record.get("field_a"), Some(&Value::Number(1.into())));
        assert_eq!(ctx.record.get("field_b"), Some(&Value::Number(2.into())));
    }

    #[test]
    fn later_hook_can_see_earlier_hooks_changes() {
        /// A hook that reads field_a and sets field_b to its value + 10.
        struct ReadAndModifyHook;

        impl Hook for ReadAndModifyHook {
            fn name(&self) -> &str {
                "read_and_modify"
            }

            fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
                let val = ctx
                    .record
                    .get("counter")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                ctx.record
                    .insert("doubled".to_string(), Value::Number((val * 2).into()));
                Ok(())
            }
        }

        let mut registry = HookRegistry::new();
        registry.register(
            DataModifyingHook {
                field: "counter",
                value: Value::Number(5.into()),
            },
            10,
        );
        registry.register(ReadAndModifyHook, 20);

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();

        assert_eq!(ctx.record.get("counter"), Some(&Value::Number(5.into())));
        assert_eq!(ctx.record.get("doubled"), Some(&Value::Number(10.into())));
    }

    // ── Metadata communication between hooks ─────────────────────────────

    #[test]
    fn hooks_can_communicate_via_metadata() {
        struct MetadataWriter;
        impl Hook for MetadataWriter {
            fn name(&self) -> &str {
                "writer"
            }
            fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
                ctx.set_metadata("processed_by", Value::String("writer_hook".to_string()));
                Ok(())
            }
        }

        struct MetadataReader {
            saw_metadata: AtomicBool,
        }
        impl Hook for MetadataReader {
            fn name(&self) -> &str {
                "reader"
            }
            fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
                if ctx.get_metadata("processed_by").is_some() {
                    self.saw_metadata.store(true, Ordering::SeqCst);
                }
                Ok(())
            }
        }

        let reader = Arc::new(MetadataReader {
            saw_metadata: AtomicBool::new(false),
        });

        let mut registry = HookRegistry::new();
        registry.register(MetadataWriter, 10);
        registry.hooks.push(HookEntry {
            hook: reader.clone(),
            priority: 20,
        });

        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();

        assert!(reader.saw_metadata.load(Ordering::SeqCst));
    }

    // ── Empty registry ───────────────────────────────────────────────────

    #[test]
    fn empty_registry_before_succeeds() {
        let registry = HookRegistry::new();
        let mut ctx = make_ctx(RecordOperation::Create);
        assert!(registry.run_before(&mut ctx).is_ok());
    }

    #[test]
    fn empty_registry_after_returns_no_errors() {
        let registry = HookRegistry::new();
        let ctx = make_ctx(RecordOperation::Delete);
        assert!(registry.run_after(&ctx).is_empty());
    }

    // ── Debug formatting ─────────────────────────────────────────────────

    #[test]
    fn registry_debug_format() {
        let mut registry = HookRegistry::new();
        registry.register_default(TrackingHook::new("my_hook"));
        let debug = format!("{:?}", registry);
        assert!(debug.contains("HookRegistry"));
        assert!(debug.contains("my_hook"));
        assert!(debug.contains("hook_count: 1"));
    }

    // ── Auth context in hooks ────────────────────────────────────────────

    #[test]
    fn hook_can_access_auth_info() {
        struct AuthCheckHook {
            saw_superuser: AtomicBool,
        }

        impl Hook for AuthCheckHook {
            fn name(&self) -> &str {
                "auth_check"
            }
            fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
                if ctx.auth.is_superuser {
                    self.saw_superuser.store(true, Ordering::SeqCst);
                }
                Ok(())
            }
        }

        let hook = Arc::new(AuthCheckHook {
            saw_superuser: AtomicBool::new(false),
        });

        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        let mut ctx = make_ctx(RecordOperation::Create).with_auth(HookAuthInfo::superuser());
        registry.run_before(&mut ctx).unwrap();

        assert!(hook.saw_superuser.load(Ordering::SeqCst));
    }

    // ── Conditional abort based on data ──────────────────────────────────

    #[test]
    fn hook_can_abort_based_on_record_data() {
        /// Rejects records with status == "banned".
        struct StatusGuard;

        impl Hook for StatusGuard {
            fn name(&self) -> &str {
                "status_guard"
            }
            fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
                if ctx.record.get("status").and_then(|v| v.as_str()) == Some("banned") {
                    return Err(ZerobaseError::forbidden("banned records cannot be created"));
                }
                Ok(())
            }
        }

        let mut registry = HookRegistry::new();
        registry.register_default(StatusGuard);

        // Should pass: normal record
        let mut ctx = make_ctx(RecordOperation::Create);
        ctx.record
            .insert("status".to_string(), Value::String("active".to_string()));
        assert!(registry.run_before(&mut ctx).is_ok());

        // Should abort: banned record
        let mut ctx = make_ctx(RecordOperation::Create);
        ctx.record
            .insert("status".to_string(), Value::String("banned".to_string()));
        let err = registry.run_before(&mut ctx).unwrap_err();
        assert_eq!(err.status_code(), 403);
    }

    // ── Mixed before + after scenario ────────────────────────────────────

    #[test]
    fn full_lifecycle_before_and_after() {
        let hook = Arc::new(TrackingHook::new("lifecycle"));

        let mut registry = HookRegistry::new();
        registry.hooks.push(HookEntry {
            hook: hook.clone(),
            priority: 100,
        });

        // Before phase
        let mut ctx = make_ctx(RecordOperation::Create);
        registry.run_before(&mut ctx).unwrap();
        assert!(hook.was_before_called());
        assert!(!hook.was_after_called());

        // After phase
        let ctx = make_ctx(RecordOperation::Create);
        registry.run_after(&ctx);
        assert!(hook.was_after_called());
    }
}
