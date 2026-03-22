# ADR-001: SQLite Database Abstraction Layer

**Status:** Accepted
**Date:** 2026-03-20

## Context

Zerobase needs an embedded SQLite database layer that mirrors Pocketbase's approach: collections map to SQLite tables, with auto-generated CRUD APIs, dynamic schema changes, and full transaction support. The abstraction must be testable (in-memory DBs for tests), support connection pooling for concurrent readers, and provide a migration system for schema evolution.

## Decisions

### 1. Connection Pooling: `r2d2` + `r2d2_sqlite`

**Choice:** `r2d2` with `r2d2_sqlite` (synchronous pool).

**Why not `deadpool-sqlite`?**
- SQLite is fundamentally synchronous — it uses file-level locking. Wrapping it in an async pool adds complexity without real benefit.
- `r2d2` is the most downloaded Rust connection pool (~30M downloads), battle-tested, and simple.
- `r2d2_sqlite` provides a ready-made `ConnectionManager` for `rusqlite`.
- We run blocking SQLite operations via `tokio::task::spawn_blocking`, which pairs naturally with a sync pool.

**Pool configuration:**
- WAL mode enabled on every connection for concurrent readers + single writer.
- `PRAGMA foreign_keys = ON` on every connection.
- `PRAGMA busy_timeout = 5000` to handle write contention gracefully.
- Pool size defaults to `num_cpus` for readers (max 8), with a dedicated write connection.

### 2. Migration System: Custom (embedded SQL files)

**Choice:** Custom migration runner using numbered SQL files embedded at compile time via `include_str!`.

**Why not `refinery`?**
- `refinery` is a solid crate but adds a proc-macro dependency and its own migration table schema. For Zerobase, migrations are tightly coupled with collection schema management — we need full control over the migration table and execution order.
- Our migrations are simple: numbered SQL files applied in order, with a `_migrations` table tracking applied versions.
- This keeps the dependency tree small and gives us full control over the migration lifecycle (important for backup/restore).

**Migration design:**
- Migrations live in `crates/zerobase-db/src/migrations/` as `.sql` files.
- A `Migration` struct holds the version number, name, and SQL content.
- The runner applies unapplied migrations inside a transaction, recording each in `_migrations`.
- Rollback is out of scope — migrations are forward-only (matches Pocketbase's approach).

### 3. Transaction Support

**Choice:** A `Transaction` wrapper that borrows a connection and provides `commit()`/`rollback()` semantics.

**Design:**
- `Database::transaction()` acquires a connection and begins a SQLite transaction.
- The returned `Transaction` implements the same query methods as `Database`, so callers use the same API.
- On drop without explicit `commit()`, the transaction rolls back (fail-safe default).
- Savepoints supported via `Transaction::savepoint()` for nested operations.

### 4. Query Builder Abstraction

**Choice:** A lightweight, internal query builder — not a full ORM.

**Why not `sea-query` or `diesel`?**
- Zerobase's queries are dynamic: collection names, field names, and filter expressions come from user-defined schemas at runtime. Static query builders/ORMs work best with known-at-compile-time schemas.
- A thin builder that produces parameterized SQL strings gives us full control over the dynamic query generation needed for Pocketbase-style filtering (`?filter=(field='value')`).
- The builder enforces parameterized queries to prevent SQL injection.

**Builder capabilities:**
- `SelectBuilder` — SELECT with WHERE, ORDER BY, LIMIT/OFFSET, JOIN (for relation expansion).
- `InsertBuilder` — INSERT with named columns and parameterized values.
- `UpdateBuilder` — UPDATE with SET and WHERE clauses.
- `DeleteBuilder` — DELETE with WHERE clause.
- All builders produce `(String, Vec<rusqlite::types::Value>)` tuples — SQL + bound parameters.

### 5. Interface Traits

Two primary repository traits define the contract for database operations:

- **`RecordRepository`** — CRUD operations on records within collections.
- **`SchemaRepository`** — DDL operations for managing collections (create/alter/drop tables).

Both traits are `Send + Sync` and return `Result<T, ZerobaseError>`. Concrete implementations use `Database` (the pool wrapper) internally. Tests can mock these traits or use an in-memory `Database`.

### 6. Write Serialization

SQLite supports concurrent readers but only one writer. Rather than relying solely on `busy_timeout`, we use a dedicated write connection (separate from the read pool) protected by a `tokio::sync::Mutex`. This ensures:
- Reads never block on writes.
- Writes are serialized without busy-retry loops.
- Write operations are always transactional.

## Architecture Overview

```
┌─────────────────────────────────────────┐
│            Application Layer            │
│  (RecordRepository, SchemaRepository)   │
├─────────────────────────────────────────┤
│              Database                   │
│  ┌──────────────┐  ┌────────────────┐   │
│  │  Read Pool   │  │ Write Conn     │   │
│  │  (r2d2, N    │  │ (Mutex-guarded │   │
│  │   connections)│  │  single conn)  │   │
│  └──────────────┘  └────────────────┘   │
├─────────────────────────────────────────┤
│           Query Builder                 │
│  (SelectBuilder, InsertBuilder, etc.)   │
├─────────────────────────────────────────┤
│           Migration Runner              │
│  (versioned SQL, _migrations table)     │
├─────────────────────────────────────────┤
│         rusqlite (SQLite FFI)           │
└─────────────────────────────────────────┘
```

## Consequences

- **Positive:** Simple, minimal dependencies, full control over SQL generation, testable with `:memory:` databases.
- **Positive:** Write serialization avoids `SQLITE_BUSY` errors under load.
- **Positive:** Custom migration system integrates cleanly with backup/restore workflows.
- **Negative:** Custom query builder requires more upfront code than adopting `sea-query`.
- **Negative:** Custom migration runner means we own the maintenance burden (acceptable given simplicity).

## References

- [r2d2 docs](https://docs.rs/r2d2)
- [rusqlite WAL mode](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html)
- [Pocketbase data layer](https://pocketbase.io/docs/)
- Zero to Production in Rust — Chapter 3 (database layer patterns)
