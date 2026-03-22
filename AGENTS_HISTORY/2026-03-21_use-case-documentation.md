# Use Case Documentation Creation

**Date:** 2026-03-21
**Task ID:** if2yt8goewm0oah

## Summary

Created 6 comprehensive use case guides covering common patterns for building applications with Zerobase. Each guide includes step-by-step instructions, complete curl examples, working JavaScript client code, and explains the relevant Zerobase features used.

## Guides Created

1. **Blog Application** (`docs/use-cases/01-blog-application.md`) — Collections for authors, posts, comments, tags; relation setup; access rules; filtering/sorting/expanding; file uploads; frontend integration.

2. **Todo Application** (`docs/use-cases/02-todo-application.md`) — Multi-user task management with projects, shared collaboration via multi-relations, cascading access control via dot-notation rules, batch operations for drag-and-drop reorder.

3. **Authentication Flows** (`docs/use-cases/03-authentication-flows.md`) — Complete coverage of all auth methods: email/password, OAuth2 (Google, GitHub), OTP, MFA (TOTP), passkeys (WebAuthn/FIDO2), email verification, password reset, email change, token refresh. Includes a reusable AuthManager JavaScript class.

4. **File Uploads** (`docs/use-cases/04-file-uploads.md`) — Single and multi-file uploads, protected file access with tokens, thumbnail generation (crop modes), file replacement/removal, S3 backend configuration, FileGallery JavaScript class.

5. **Realtime Chat** (`docs/use-cases/05-realtime-chat.md`) — SSE connection protocol, topic subscriptions, live message delivery, message history with pagination, file attachments, typing indicators, reconnection with exponential backoff, complete RealtimeChat JavaScript class.

6. **Custom Hooks** (`docs/use-cases/06-custom-hooks.md`) — Hook architecture and lifecycle, input validation, audit logging, computed fields, notifications, cascading operations, rate limiting, complete e-commerce workflow example combining all patterns.

## Files Created

- `docs/use-cases/README.md` — Index of all guides
- `docs/use-cases/01-blog-application.md`
- `docs/use-cases/02-todo-application.md`
- `docs/use-cases/03-authentication-flows.md`
- `docs/use-cases/04-file-uploads.md`
- `docs/use-cases/05-realtime-chat.md`
- `docs/use-cases/06-custom-hooks.md`
