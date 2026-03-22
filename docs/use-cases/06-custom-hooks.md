# Use Case: Custom Hooks for Business Logic

> Extend Zerobase with custom hooks to implement business logic like audit logging, data validation, notifications, computed fields, and workflow automation.

---

## Overview

Zerobase hooks let you run custom logic **before** and **after** record operations (create, update, delete, view, list). This guide covers:

1. Understanding the hook system
2. Implementing validation hooks
3. Audit logging
4. Computed and derived fields
5. Notifications and side effects
6. Cascading operations
7. Rate limiting and throttling
8. Combining hooks for workflows

---

## Hook Architecture

### Hook lifecycle

```
Client Request
    │
    ▼
┌─────────────────┐
│  Before Hooks    │ ← Can modify data, abort with error
│  (priority order)│
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Database Op     │ ← Create/Update/Delete executed
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  After Hooks     │ ← Side effects, notifications
│  (priority order)│
└────────┬────────┘
         │
         ▼
    API Response
```

### Hook context

Every hook receives a `HookContext` with:

```rust
pub struct HookContext {
    pub operation: Operation,      // Create, Update, Delete, View, List
    pub phase: Phase,              // Before, After
    pub collection: String,        // Collection name
    pub record_id: String,         // Record ID (empty for Create/List)
    pub record: Map<String, Value>,// Record data
    pub auth: HookAuthInfo,        // Current user info
    pub metadata: Map<String, Value>, // Shared data between hooks
}

pub struct HookAuthInfo {
    pub id: String,                // User ID ("" if unauthenticated)
    pub collection_id: String,     // Auth collection ID
    pub is_superuser: bool,        // Superuser flag
}
```

### Hook trait

```rust
pub trait Hook: Send + Sync {
    fn name(&self) -> &str;

    fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
        Ok(()) // Default: no-op
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        Ok(()) // Default: no-op
    }
}
```

---

## Example 1: Input Validation Hook

Enforce business rules beyond what field constraints can express.

```rust
use zerobase_hooks::{Hook, HookContext, HookResult, HookError, Operation};

pub struct OrderValidationHook;

impl Hook for OrderValidationHook {
    fn name(&self) -> &str {
        "order_validation"
    }

    fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
        if ctx.collection != "orders" {
            return Ok(());
        }

        match ctx.operation {
            Operation::Create | Operation::Update => {
                // Validate: quantity must be positive
                if let Some(qty) = ctx.record.get("quantity").and_then(|v| v.as_i64()) {
                    if qty <= 0 {
                        return Err(HookError::validation("quantity must be positive"));
                    }
                }

                // Validate: total must match unit_price * quantity
                let unit_price = ctx.record.get("unit_price")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let quantity = ctx.record.get("quantity")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as f64;
                let total = ctx.record.get("total")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                let expected = unit_price * quantity;
                if (total - expected).abs() > 0.01 {
                    return Err(HookError::validation(
                        &format!("total ({total}) doesn't match unit_price * quantity ({expected})")
                    ));
                }

                // Validate: status transitions
                if ctx.operation == Operation::Update {
                    if let (Some(old_status), Some(new_status)) = (
                        ctx.metadata.get("old_status").and_then(|v| v.as_str()),
                        ctx.record.get("status").and_then(|v| v.as_str()),
                    ) {
                        let valid = match (old_status, new_status) {
                            ("pending", "confirmed" | "cancelled") => true,
                            ("confirmed", "shipped" | "cancelled") => true,
                            ("shipped", "delivered") => true,
                            _ => old_status == new_status,
                        };
                        if !valid {
                            return Err(HookError::validation(
                                &format!("cannot transition from '{old_status}' to '{new_status}'")
                            ));
                        }
                    }
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }
}
```

### Register the hook

```rust
use zerobase_hooks::HookRegistry;

fn setup_hooks(registry: &mut HookRegistry) {
    registry.register("orders", Box::new(OrderValidationHook), 10); // priority 10
}
```

---

## Example 2: Audit Logging Hook

Track who changed what, when.

```rust
use zerobase_hooks::{Hook, HookContext, HookResult, Operation};
use chrono::Utc;
use serde_json::json;

pub struct AuditLogHook {
    // In a real app, inject a repository or database handle
}

impl Hook for AuditLogHook {
    fn name(&self) -> &str {
        "audit_log"
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        // Only log write operations
        let action = match ctx.operation {
            Operation::Create => "create",
            Operation::Update => "update",
            Operation::Delete => "delete",
            _ => return Ok(()),
        };

        let log_entry = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "action": action,
            "collection": ctx.collection,
            "record_id": ctx.record_id,
            "user_id": ctx.auth.id,
            "is_superuser": ctx.auth.is_superuser,
            "changes": ctx.metadata.get("diff"), // Set by a before hook
        });

        // Write to audit log (your persistence mechanism)
        tracing::info!(
            target: "audit",
            collection = %ctx.collection,
            action = %action,
            record_id = %ctx.record_id,
            user_id = %ctx.auth.id,
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_default()
        );

        Ok(())
    }
}
```

### Track field-level changes with a before hook

```rust
pub struct ChangeTrackingHook;

impl Hook for ChangeTrackingHook {
    fn name(&self) -> &str {
        "change_tracking"
    }

    fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
        if ctx.operation == Operation::Update {
            // Store the original record for comparison in after hooks
            // The original record data would be fetched from the database
            // This is a simplified example
            ctx.metadata.insert(
                "old_status".to_string(),
                ctx.record.get("status").cloned().unwrap_or(json!(null))
            );
        }
        Ok(())
    }
}
```

---

## Example 3: Computed Fields Hook

Auto-populate fields based on other field values.

```rust
use zerobase_hooks::{Hook, HookContext, HookResult, Operation};
use serde_json::json;

pub struct ComputedFieldsHook;

impl Hook for ComputedFieldsHook {
    fn name(&self) -> &str {
        "computed_fields"
    }

    fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
        match ctx.collection.as_str() {
            "posts" => self.compute_post_fields(ctx),
            "users" => self.compute_user_fields(ctx),
            _ => Ok(()),
        }
    }
}

impl ComputedFieldsHook {
    fn compute_post_fields(&self, ctx: &mut HookContext) -> HookResult<()> {
        if ctx.operation != Operation::Create && ctx.operation != Operation::Update {
            return Ok(());
        }

        // Auto-generate slug from title
        if let Some(title) = ctx.record.get("title").and_then(|v| v.as_str()) {
            let slug = title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            ctx.record.insert("slug".to_string(), json!(slug));
        }

        // Auto-generate excerpt from content (strip HTML, take first 200 chars)
        if let Some(content) = ctx.record.get("content").and_then(|v| v.as_str()) {
            let plain_text = ammonia::clean(content); // Strip HTML
            let excerpt: String = plain_text.chars().take(200).collect();
            let excerpt = if plain_text.len() > 200 {
                format!("{excerpt}...")
            } else {
                excerpt
            };
            ctx.record.insert("excerpt".to_string(), json!(excerpt));
        }

        // Set published_at when status changes to "published"
        if ctx.record.get("status").and_then(|v| v.as_str()) == Some("published") {
            if ctx.record.get("published_at").and_then(|v| v.as_str()).is_none() {
                ctx.record.insert(
                    "published_at".to_string(),
                    json!(chrono::Utc::now().to_rfc3339()),
                );
            }
        }

        Ok(())
    }

    fn compute_user_fields(&self, ctx: &mut HookContext) -> HookResult<()> {
        if ctx.operation != Operation::Create && ctx.operation != Operation::Update {
            return Ok(());
        }

        // Normalize email to lowercase
        if let Some(email) = ctx.record.get("email").and_then(|v| v.as_str()) {
            ctx.record.insert("email".to_string(), json!(email.to_lowercase()));
        }

        Ok(())
    }
}
```

---

## Example 4: Notification Hook

Send notifications when certain events occur.

```rust
use zerobase_hooks::{Hook, HookContext, HookResult, Operation};

pub struct NotificationHook {
    // In production: inject an email service, push notification service, etc.
}

impl Hook for NotificationHook {
    fn name(&self) -> &str {
        "notifications"
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        match (ctx.collection.as_str(), &ctx.operation) {
            // Notify post author when someone comments
            ("comments", Operation::Create) => {
                let post_id = ctx.record.get("post")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // In production: look up the post author and send notification
                tracing::info!(
                    "New comment on post {post_id} by user {}",
                    ctx.auth.id
                );

                // Example: queue an email notification
                // email_service.send_notification(
                //     post_author_email,
                //     "New comment on your post",
                //     &format!("Someone commented on your post: {}", ctx.record.get("body")...),
                // );

                Ok(())
            }

            // Notify assignee when a task is assigned to them
            ("tasks", Operation::Update) => {
                if let Some(assignee) = ctx.record.get("assignee").and_then(|v| v.as_str()) {
                    if !assignee.is_empty() && assignee != ctx.auth.id {
                        tracing::info!(
                            "Task {} assigned to user {assignee}",
                            ctx.record_id
                        );
                    }
                }
                Ok(())
            }

            // Notify when order status changes
            ("orders", Operation::Update) => {
                if let Some(status) = ctx.record.get("status").and_then(|v| v.as_str()) {
                    if status == "shipped" {
                        tracing::info!(
                            "Order {} has been shipped — notify customer",
                            ctx.record_id
                        );
                    }
                }
                Ok(())
            }

            _ => Ok(()),
        }
    }
}
```

---

## Example 5: Cascading Operations Hook

Automatically clean up or update related records.

```rust
use zerobase_hooks::{Hook, HookContext, HookResult, Operation};

pub struct CascadeDeleteHook;

impl Hook for CascadeDeleteHook {
    fn name(&self) -> &str {
        "cascade_delete"
    }

    fn after_operation(&self, ctx: &HookContext) -> HookResult<()> {
        if ctx.operation != Operation::Delete {
            return Ok(());
        }

        match ctx.collection.as_str() {
            // When a project is deleted, delete all its tasks
            "projects" => {
                tracing::info!(
                    "Project {} deleted — cascading delete to tasks",
                    ctx.record_id
                );
                // In production: use a repository to delete related records
                // task_repo.delete_where("project", &ctx.record_id).await?;
                Ok(())
            }

            // When a user is deleted, anonymize their comments
            "users" => {
                tracing::info!(
                    "User {} deleted — anonymizing comments",
                    ctx.record_id
                );
                // comment_repo.update_where(
                //     "author", &ctx.record_id,
                //     json!({"author": null, "guest_name": "[deleted]"}),
                // ).await?;
                Ok(())
            }

            _ => Ok(()),
        }
    }
}
```

---

## Example 6: Rate Limiting Hook

Prevent abuse by limiting how often a user can perform operations.

```rust
use zerobase_hooks::{Hook, HookContext, HookResult, HookError, Operation};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Instant, Duration};

pub struct RateLimitHook {
    limits: Mutex<HashMap<String, Vec<Instant>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimitHook {
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            limits: Mutex::new(HashMap::new()),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }
}

impl Hook for RateLimitHook {
    fn name(&self) -> &str {
        "rate_limit"
    }

    fn before_operation(&self, ctx: &mut HookContext) -> HookResult<()> {
        // Only rate-limit creates
        if ctx.operation != Operation::Create {
            return Ok(());
        }

        // Don't rate-limit superusers
        if ctx.auth.is_superuser {
            return Ok(());
        }

        let key = format!("{}:{}", ctx.auth.id, ctx.collection);
        let now = Instant::now();

        let mut limits = self.limits.lock().map_err(|_| HookError::internal("lock error"))?;
        let timestamps = limits.entry(key).or_insert_with(Vec::new);

        // Remove expired entries
        timestamps.retain(|t| now.duration_since(*t) < self.window);

        if timestamps.len() >= self.max_requests {
            return Err(HookError::rate_limited(
                &format!("too many requests — max {} per {} seconds", self.max_requests, self.window.as_secs())
            ));
        }

        timestamps.push(now);
        Ok(())
    }
}
```

### Register with limits

```rust
fn setup_hooks(registry: &mut HookRegistry) {
    // Max 10 messages per minute
    registry.register("messages", Box::new(RateLimitHook::new(10, 60)), 1);

    // Max 5 posts per hour
    registry.register("posts", Box::new(RateLimitHook::new(5, 3600)), 1);
}
```

---

## Example 7: Complete Workflow — E-commerce Order Processing

Combine multiple hooks for a complete business workflow:

```rust
fn setup_ecommerce_hooks(registry: &mut HookRegistry) {
    // Priority 1: Rate limiting (runs first)
    registry.register("orders", Box::new(RateLimitHook::new(20, 60)), 1);

    // Priority 5: Change tracking (captures old state)
    registry.register("orders", Box::new(ChangeTrackingHook), 5);

    // Priority 10: Validation (rejects invalid data)
    registry.register("orders", Box::new(OrderValidationHook), 10);

    // Priority 20: Computed fields (auto-calculate totals)
    registry.register("orders", Box::new(OrderComputedFieldsHook), 20);

    // Priority 100: Notifications (send after successful operation)
    registry.register("orders", Box::new(NotificationHook), 100);

    // Priority 110: Audit logging (record everything)
    registry.register("orders", Box::new(AuditLogHook {}), 110);
}
```

**Flow for creating an order:**

```
1. RateLimitHook (before) → Check user hasn't exceeded order limit
2. ChangeTrackingHook (before) → N/A for create
3. OrderValidationHook (before) → Validate quantities, totals, required fields
4. OrderComputedFieldsHook (before) → Calculate tax, shipping, final total
5. --- Database Insert ---
6. NotificationHook (after) → Email order confirmation to customer
7. AuditLogHook (after) → Log "order created" with full details
```

**Flow for updating order status to "shipped":**

```
1. RateLimitHook (before) → Check rate limit
2. ChangeTrackingHook (before) → Store old status = "confirmed"
3. OrderValidationHook (before) → Validate transition "confirmed" → "shipped" is valid
4. OrderComputedFieldsHook (before) → Set shipped_at timestamp
5. --- Database Update ---
6. NotificationHook (after) → Email shipping notification with tracking info
7. AuditLogHook (after) → Log "order updated" with status change diff
```

---

## Hook Registration Summary

```rust
use zerobase_hooks::HookRegistry;

pub fn register_all_hooks(registry: &mut HookRegistry) {
    // Global hooks (apply to all collections)
    registry.register_global(Box::new(AuditLogHook {}), 200);

    // Collection-specific hooks
    registry.register("posts", Box::new(ComputedFieldsHook), 10);
    registry.register("orders", Box::new(OrderValidationHook), 10);
    registry.register("orders", Box::new(RateLimitHook::new(20, 60)), 1);
    registry.register("comments", Box::new(NotificationHook), 100);
    registry.register("projects", Box::new(CascadeDeleteHook), 100);
    registry.register("users", Box::new(CascadeDeleteHook), 100);
}
```

---

## Summary

| Pattern | Hook Phase | Use Case |
|---|---|---|
| Validation | Before | Business rule enforcement, status transitions |
| Computed fields | Before | Auto-generate slugs, excerpts, timestamps |
| Rate limiting | Before | Prevent abuse, throttle writes |
| Audit logging | After | Track changes, compliance, debugging |
| Notifications | After | Email, push, webhook triggers |
| Cascading ops | After | Delete/update related records |
| Data enrichment | Before | Normalize, sanitize, transform input |

### Hook priorities (convention)

| Priority | Category |
|---|---|
| 1-9 | Rate limiting, auth checks |
| 10-49 | Validation, data integrity |
| 50-99 | Computed fields, transformations |
| 100-149 | Notifications, side effects |
| 150-199 | Cascading operations |
| 200+ | Audit logging, telemetry |
