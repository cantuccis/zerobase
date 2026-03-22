# Task: Implement password hashing with argon2

**Date:** 2026-03-21
**Task ID:** okvzz9oxnglm4c8

## Summary

Enhanced the existing Argon2id password hashing implementation in `zerobase-auth` to use explicit OWASP-recommended parameters instead of relying on crate defaults, and added comprehensive tests including timing consistency verification.

### Changes Made

1. **Explicit OWASP parameters**: Replaced `Argon2::default()` with a `build_argon2()` function that explicitly configures:
   - Algorithm: Argon2id
   - Memory cost: 19,456 KiB (19 MiB)
   - Time cost (iterations): 2
   - Parallelism: 1

2. **New tests added** (7 new tests, 15 total):
   - `hash_embeds_owasp_parameters` — verifies PHC string contains correct m/t/p values
   - `verify_rejects_empty_hash` — edge case for empty hash input
   - `hash_handles_special_characters` — special chars in passwords
   - `hash_handles_null_bytes` — null bytes in passwords
   - `verify_timing_is_consistent` — checks correct/incorrect verification takes similar time
   - `build_argon2_uses_argon2id` — confirms algorithm selection

3. **Documentation**: Updated module-level docs to clarify parameters are explicit, not default-dependent.

## Files Modified

- `crates/zerobase-auth/src/password.rs` — Enhanced hashing implementation with explicit OWASP params and expanded test suite

## Test Results

All 15 tests passing in `zerobase-auth` crate.
