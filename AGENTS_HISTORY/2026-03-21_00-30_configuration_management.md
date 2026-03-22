# Configuration Management Setup

**Date:** 2026-03-21 00:30
**Task ID:** 4esfwajt8knnnu1
**Task:** Set up configuration management with config crate

## Summary

Implemented a layered configuration system for Zerobase using the `config` crate. Configuration loads from three sources in order of increasing priority:

1. **Compiled-in defaults** (sensible defaults for all fields)
2. **TOML config file** (`zerobase.toml` or path in `ZEROBASE_CONFIG` env var)
3. **Environment variables** (prefixed with `ZEROBASE__`, using `__` as separator)

The `Settings` struct covers all required sections: server, database, storage, auth, and SMTP. Sensitive fields (token_secret, S3 keys, SMTP password) use `secrecy::SecretString` to prevent accidental logging.

Validation enforces:
- Required fields: `auth.token_secret`, non-empty host, non-zero port
- Conditional requirements: S3 bucket/region when backend=s3, SMTP host/sender when enabled
- Clear error messages pointing to the specific missing field

## Test Coverage

16 configuration tests (all `#[serial]` for env var safety):
- TOML file loading with all fields
- Environment variable overrides
- Env precedence over file values
- Default values applied for unset fields
- Missing required `token_secret` produces clear error
- Empty host rejected
- Zero port rejected
- Zero token duration rejected
- S3 backend requires S3 section
- S3 backend with valid config succeeds
- SMTP enabled requires host
- SMTP enabled requires sender_address
- SMTP disabled does not require host
- Full SMTP config loads all fields
- Server address formatting
- Invalid TOML produces descriptive error

## Files Modified

- `Cargo.toml` (workspace) - Added `config` and `secrecy` workspace dependencies
- `crates/zerobase-core/Cargo.toml` - Added `config`, `secrecy`, `tempfile`, `serial_test` dependencies
- `crates/zerobase-core/src/lib.rs` - Added `configuration` module and `Settings` re-export
- `crates/zerobase-core/src/configuration.rs` - **NEW** - Full configuration module with Settings struct, section structs, loading logic, validation, and 16 tests
