# Implement Image Thumbnail Generation

**Date:** 2026-03-21
**Task ID:** hq05opragvw2a73
**Phase:** 7

## Summary

Implemented on-the-fly image thumbnail generation for Zerobase's file serving endpoints. Thumbnails are generated lazily on first request and cached in storage for subsequent requests, following PocketBase's thumbnail specification format.

### Features Implemented

1. **Thumb spec parsing** — Parses PocketBase-compatible specs like `100x100`, `200x0` (auto height), `0x150` (auto width), `100x100f` (fit), `100x100t` (top crop), `100x100b` (bottom crop)
2. **Image resizing** — Uses the `image` crate with Lanczos3 filter for high-quality resizing with support for center crop, top crop, bottom crop, fit-within-bounds, and aspect-ratio-preserving resize
3. **Caching** — Thumbnails are stored alongside originals at `<col>/<rec>/thumbs/<spec>_<filename>` and served from cache on subsequent requests
4. **API integration** — The `?thumb=` query parameter on `GET /api/files/:collectionId/:recordId/:filename` triggers thumbnail generation
5. **Error handling** — Returns 400 for non-image files and invalid specs, 404 for missing originals

### Supported Image Formats
- JPEG, PNG, GIF, WebP

### Tests Written
- **32 unit tests** in `zerobase-files` (thumb parsing, image generation, service-level caching)
- **10 integration tests** added to `file_download_endpoints.rs` (HTTP-level thumbnail serving, caching, error cases)
- All existing tests continue to pass

## Files Modified

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Added `image = "0.25"` workspace dependency |
| `crates/zerobase-files/Cargo.toml` | Added `image` dependency |
| `crates/zerobase-api/Cargo.toml` | Added `image` dev-dependency for integration tests |
| `crates/zerobase-files/src/thumb.rs` | Full implementation: `parse_thumb_spec()`, `generate_thumbnail()`, resize/crop functions, comprehensive tests |
| `crates/zerobase-files/src/service.rs` | Added `get_or_generate_thumbnail()` method to `FileService` with caching logic and tests |
| `crates/zerobase-api/src/handlers/files.rs` | Wired `?thumb=` query parameter into `serve_file` handler with proper error handling |
| `crates/zerobase-api/tests/file_download_endpoints.rs` | Added 10 integration tests for thumbnail endpoint |
