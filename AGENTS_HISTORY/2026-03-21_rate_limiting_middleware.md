# Rate Limiting Middleware Implementation

**Date:** 2026-03-21
**Task ID:** 0zwx8dao4qhlm2m
**Phase:** 12

## Summary

Implemented a custom rate-limiting middleware for the Zerobase API using a token-bucket algorithm with per-client-IP tracking and per-endpoint-category configurable limits. The middleware returns HTTP 429 (Too Many Requests) with a `Retry-After` header when clients exceed their quota.

## Key Design Decisions

- **Custom implementation over `tower-governor`**: Used `DashMap` for a lightweight, zero-contention concurrent hashmap instead of pulling in a heavier dependency. This gives full control over per-category limits and the cleanup strategy.
- **Route categories**: Requests are classified into `Auth` (stricter: 10 req/60s) and `Default` (100 req/60s) categories. Auth endpoints include login, OTP, password reset, OAuth2, MFA, passkey, and verification flows.
- **IP extraction**: Checks `x-forwarded-for` (first IP), then `x-real-ip`, then `ConnectInfo`, with `127.0.0.1` fallback.
- **Response headers**: Successful responses include `x-ratelimit-remaining` and `x-ratelimit-limit` headers. Rate-limited responses include `retry-after`.
- **Configurable & toggleable**: `RateLimitConfig` allows enabling/disabling, customizing per-category limits, and setting the default limit. Both `api_router` and `api_router_with_auth` integrate the limiter.
- **Cleanup method**: `RateLimiter::cleanup()` removes stale buckets (older than 2x window) to prevent unbounded memory growth.

## Test Coverage (30 tests)

- Route classification tests for all auth endpoint patterns (password, OTP, MFA, passkey, OAuth2, verification, email change, password reset, admin login)
- Token bucket unit tests (within limit, exceeded, independent IPs, independent categories, disabled, window reset, cleanup)
- Middleware integration tests (429 response body/headers, per-category enforcement, disabled mode, independent IP tracking)
- IP extraction tests (x-forwarded-for, x-real-ip, precedence, fallback)
- Configuration tests (defaults, fallback behavior)

## Files Modified

- `Cargo.toml` (workspace) — Added `dashmap = "6"` workspace dependency
- `crates/zerobase-api/Cargo.toml` — Added `dashmap` dependency
- `crates/zerobase-api/src/middleware/mod.rs` — Registered `rate_limit` module
- `crates/zerobase-api/src/middleware/rate_limit.rs` — **NEW** — Full rate-limiting implementation with 30 tests
- `crates/zerobase-api/src/lib.rs` — Integrated rate limiter into `api_router`, `api_router_with_auth`, added new `_with_rate_limit` variants, exported public types
