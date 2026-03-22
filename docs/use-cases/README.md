# Zerobase Use Case Guides

Step-by-step guides for building applications with Zerobase. Each guide includes complete code snippets and working API examples.

## Guides

| # | Guide | What You'll Build |
|---|---|---|
| 1 | [Blog Application](./01-blog-application.md) | Posts, authors, comments, tags with relations and file uploads |
| 2 | [Todo Application](./02-todo-application.md) | Multi-user task management with projects, priorities, and batch operations |
| 3 | [Authentication Flows](./03-authentication-flows.md) | Password, OAuth2, OTP, MFA, passkeys, verification, and password reset |
| 4 | [File Uploads](./04-file-uploads.md) | Single/multi-file uploads, protected files, thumbnails, S3 storage |
| 5 | [Realtime Chat](./05-realtime-chat.md) | Multi-room chat with SSE subscriptions and live message delivery |
| 6 | [Custom Hooks](./06-custom-hooks.md) | Validation, audit logging, notifications, computed fields, rate limiting |

## Prerequisites

All guides assume:
- Zerobase server running at `http://localhost:8090`
- A superuser account created via `zerobase superuser create`
- `curl` and `jq` installed for API testing
