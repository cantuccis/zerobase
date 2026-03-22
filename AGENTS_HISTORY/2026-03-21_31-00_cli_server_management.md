# CLI for Server Management

**Task ID:** jagbgvj9h1espss
**Date:** 2026-03-21
**Status:** Complete

## Summary

Implemented CLI for server management using the `clap` crate with derive macros. The CLI supports `serve`, `migrate`, `superuser` (create/update/delete/list), and `version` subcommands.

## Files Modified

### `crates/zerobase-server/src/cli.rs` (NEW)
- Defined `Cli` root struct with `Option<Command>` subcommand
- `Command` enum: `Serve`, `Migrate`, `Superuser`, `Version`
- `ServeArgs`: `--host`, `-p/--port`, `--data-dir`, `--log-format` (all optional)
- `MigrateArgs`: `--data-dir` (optional)
- `SuperuserArgs` with `SuperuserAction` enum: `Create`, `Update`, `Delete`, `List`
- `SuperuserCreateArgs`: `--email`, `--password` (required)
- `SuperuserUpdateArgs`: `--email` (required), `--new-email`, `--new-password` (optional)
- `SuperuserDeleteArgs`: `--email` (required)
- 21 unit tests for CLI argument parsing

### `crates/zerobase-server/src/main.rs` (REWRITTEN)
- Dispatches CLI commands via `run()` match
- `cmd_serve`: loads Settings, applies CLI overrides, opens DB, runs migrations, builds API router, starts HTTP server with graceful shutdown
- `cmd_migrate`: opens DB, runs migrations, prints schema version
- `cmd_superuser`: opens DB, runs migrations, uses SuperuserService + Argon2Hasher for create/update/delete/list
- `cmd_version`: prints version string
- Default (no subcommand): prints version

### `crates/zerobase-core/src/services/superuser_service.rs` (MODIFIED)
- Added `update_superuser(email, new_email, new_password)` method with validation
- Added `find_by_email(email)` method (strips sensitive fields)

## Design Decisions

- CLI flags override config-file and environment-variable settings (precedence: defaults < config < env < CLI)
- Removed `env` attributes from clap args since `Settings::load()` already handles env vars
- Default command (no args) prints version info
- Superuser commands run migrations before operating to ensure schema exists

## Test Results

All 21 CLI tests pass. Full workspace test suite passes.
