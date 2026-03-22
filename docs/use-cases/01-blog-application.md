# Use Case: Building a Blog with Zerobase

> A step-by-step guide to building a fully functional blog with posts, authors, comments, and tags using Zerobase collections, relations, and the auto-generated REST API.

---

## Overview

This guide walks you through:

1. Creating collections for authors, posts, comments, and tags
2. Setting up relations between collections
3. Configuring access rules
4. Using the API for CRUD operations
5. Filtering, sorting, and expanding relations
6. Implementing a simple frontend that consumes the API

---

## Prerequisites

- Zerobase server running at `http://localhost:8090`
- A superuser account created via `zerobase superuser create --email admin@example.com --password admin123456`
- `curl` or any HTTP client for testing

---

## Step 1: Authenticate as Superuser

All collection management requires superuser access.

```bash
# Authenticate and store the token
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "admin@example.com", "password": "admin123456"}' \
  | jq -r '.token')

echo "Superuser token: $TOKEN"
```

---

## Step 2: Create the Authors Collection (Auth Type)

Authors are users who can log in and create posts. Use an **auth** collection.

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "authors",
    "type": "auth",
    "fields": [
      {
        "name": "display_name",
        "type": "text",
        "required": true,
        "options": { "min": 2, "max": 100 }
      },
      {
        "name": "bio",
        "type": "text",
        "options": { "max": 500 }
      },
      {
        "name": "avatar",
        "type": "file",
        "options": {
          "maxSelect": 1,
          "maxSize": 2097152,
          "mimeTypes": ["image/jpeg", "image/png", "image/webp"]
        }
      }
    ],
    "listRule": "",
    "viewRule": "",
    "createRule": "",
    "updateRule": "@request.auth.id = id",
    "deleteRule": "@request.auth.id = id"
  }'
```

**What the rules mean:**
- Anyone can list and view authors (public blog)
- Anyone can register (create)
- Only the author themselves can update or delete their profile

---

## Step 3: Create the Tags Collection

Tags are simple labels for categorizing posts.

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "tags",
    "type": "base",
    "fields": [
      {
        "name": "name",
        "type": "text",
        "required": true,
        "unique": true,
        "options": { "min": 1, "max": 50 }
      },
      {
        "name": "color",
        "type": "text",
        "options": { "pattern": "^#[0-9a-fA-F]{6}$" }
      }
    ],
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "@request.auth.id != \"\"",
    "deleteRule": null
  }'
```

**Rules:** Public read, authenticated users can create/update, only superusers can delete.

---

## Step 4: Create the Posts Collection

Posts reference an author and can have multiple tags.

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "posts",
    "type": "base",
    "fields": [
      {
        "name": "title",
        "type": "text",
        "required": true,
        "searchable": true,
        "options": { "min": 1, "max": 200 }
      },
      {
        "name": "slug",
        "type": "text",
        "required": true,
        "unique": true,
        "options": { "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" }
      },
      {
        "name": "content",
        "type": "editor",
        "required": true,
        "searchable": true
      },
      {
        "name": "excerpt",
        "type": "text",
        "options": { "max": 300 }
      },
      {
        "name": "cover_image",
        "type": "file",
        "options": {
          "maxSelect": 1,
          "maxSize": 5242880,
          "mimeTypes": ["image/jpeg", "image/png", "image/webp"]
        }
      },
      {
        "name": "status",
        "type": "select",
        "required": true,
        "options": { "values": ["draft", "published", "archived"] }
      },
      {
        "name": "published_at",
        "type": "datetime"
      },
      {
        "name": "author",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "authors",
          "maxSelect": 1
        }
      },
      {
        "name": "tags",
        "type": "relation",
        "options": {
          "collectionId": "tags",
          "maxSelect": 10
        }
      }
    ],
    "listRule": "status = \"published\" || author = @request.auth.id",
    "viewRule": "status = \"published\" || author = @request.auth.id",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "author = @request.auth.id",
    "deleteRule": "author = @request.auth.id"
  }'
```

**Rules:**
- Anyone can see published posts; authors can see their own drafts
- Authenticated users can create posts
- Only the post author can update or delete

---

## Step 5: Create the Comments Collection

Comments belong to a post and optionally to an authenticated author.

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "comments",
    "type": "base",
    "fields": [
      {
        "name": "post",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "posts",
          "maxSelect": 1
        }
      },
      {
        "name": "author",
        "type": "relation",
        "options": {
          "collectionId": "authors",
          "maxSelect": 1
        }
      },
      {
        "name": "guest_name",
        "type": "text",
        "options": { "max": 100 }
      },
      {
        "name": "body",
        "type": "text",
        "required": true,
        "searchable": true,
        "options": { "min": 1, "max": 2000 }
      }
    ],
    "listRule": "",
    "viewRule": "",
    "createRule": "",
    "updateRule": "author = @request.auth.id",
    "deleteRule": "author = @request.auth.id"
  }'
```

---

## Step 6: Register an Author and Create Content

### Register a new author

```bash
curl -X POST http://localhost:8090/api/collections/authors/records \
  -H "Content-Type: application/json" \
  -d '{
    "email": "jane@example.com",
    "password": "securePassword123",
    "passwordConfirm": "securePassword123",
    "display_name": "Jane Doe",
    "bio": "Tech writer and Rust enthusiast"
  }'
```

### Log in as the author

```bash
AUTHOR_TOKEN=$(curl -s -X POST http://localhost:8090/api/collections/authors/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "jane@example.com", "password": "securePassword123"}' \
  | jq -r '.token')

AUTHOR_ID=$(curl -s -X POST http://localhost:8090/api/collections/authors/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "jane@example.com", "password": "securePassword123"}' \
  | jq -r '.record.id')
```

### Create tags

```bash
TAG1=$(curl -s -X POST http://localhost:8090/api/collections/tags/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AUTHOR_TOKEN" \
  -d '{"name": "rust", "color": "#DEA584"}' | jq -r '.id')

TAG2=$(curl -s -X POST http://localhost:8090/api/collections/tags/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AUTHOR_TOKEN" \
  -d '{"name": "tutorial", "color": "#4A90D9"}' | jq -r '.id')
```

### Create a blog post

```bash
curl -X POST http://localhost:8090/api/collections/posts/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AUTHOR_TOKEN" \
  -d "{
    \"title\": \"Getting Started with Zerobase\",
    \"slug\": \"getting-started-with-zerobase\",
    \"content\": \"<h2>Introduction</h2><p>Zerobase is a powerful Backend-as-a-Service built in Rust...</p>\",
    \"excerpt\": \"Learn how to build your first app with Zerobase\",
    \"status\": \"published\",
    \"published_at\": \"2026-03-21T10:00:00Z\",
    \"author\": \"$AUTHOR_ID\",
    \"tags\": [\"$TAG1\", \"$TAG2\"]
  }"
```

### Add a comment

```bash
# Authenticated comment
curl -X POST http://localhost:8090/api/collections/comments/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $AUTHOR_TOKEN" \
  -d "{
    \"post\": \"POST_ID_HERE\",
    \"author\": \"$AUTHOR_ID\",
    \"body\": \"Great article! Looking forward to more.\"
  }"

# Guest comment (no auth required)
curl -X POST http://localhost:8090/api/collections/comments/records \
  -H "Content-Type: application/json" \
  -d '{
    "post": "POST_ID_HERE",
    "guest_name": "Anonymous Reader",
    "body": "Very helpful, thanks for sharing!"
  }'
```

---

## Step 7: Query the Blog API

### List published posts with author and tags expanded

```bash
curl -s "http://localhost:8090/api/collections/posts/records?\
filter=status='published'&\
sort=-published_at&\
expand=author,tags&\
fields=id,title,slug,excerpt,published_at,expand.author.display_name,expand.tags.name,expand.tags.color"
```

**Response:**
```json
{
  "page": 1,
  "perPage": 30,
  "totalPages": 1,
  "totalItems": 1,
  "items": [
    {
      "id": "abc123",
      "title": "Getting Started with Zerobase",
      "slug": "getting-started-with-zerobase",
      "excerpt": "Learn how to build your first app with Zerobase",
      "published_at": "2026-03-21T10:00:00Z",
      "expand": {
        "author": {
          "display_name": "Jane Doe"
        },
        "tags": [
          { "name": "rust", "color": "#DEA584" },
          { "name": "tutorial", "color": "#4A90D9" }
        ]
      }
    }
  ]
}
```

### Search posts by keyword

```bash
curl -s "http://localhost:8090/api/collections/posts/records?\
search=zerobase&\
filter=status='published'"
```

### Get a single post by slug with comments

```bash
# Get the post
curl -s "http://localhost:8090/api/collections/posts/records?\
filter=slug='getting-started-with-zerobase'&\
expand=author,tags"

# Get comments for that post
curl -s "http://localhost:8090/api/collections/comments/records?\
filter=post='POST_ID_HERE'&\
sort=-created&\
expand=author"
```

### Paginate posts

```bash
curl -s "http://localhost:8090/api/collections/posts/records?\
filter=status='published'&\
sort=-published_at&\
page=1&perPage=10"
```

---

## Step 8: Upload a Cover Image

Use multipart/form-data to upload files:

```bash
curl -X PATCH "http://localhost:8090/api/collections/posts/records/POST_ID_HERE" \
  -H "Authorization: Bearer $AUTHOR_TOKEN" \
  -F "cover_image=@/path/to/cover.jpg"
```

Access the uploaded image:

```
http://localhost:8090/api/files/COLLECTION_ID/RECORD_ID/filename.jpg
```

Generate a thumbnail:

```
http://localhost:8090/api/files/COLLECTION_ID/RECORD_ID/filename.jpg?thumb=400x300
```

---

## Step 9: Frontend Integration (JavaScript)

Here's a minimal JavaScript client for the blog API:

```javascript
const API_BASE = 'http://localhost:8090';

// List published posts
async function listPosts(page = 1, perPage = 10) {
  const params = new URLSearchParams({
    filter: "status='published'",
    sort: '-published_at',
    expand: 'author,tags',
    fields: 'id,title,slug,excerpt,published_at,cover_image,collectionId,expand',
    page: page.toString(),
    perPage: perPage.toString(),
  });

  const res = await fetch(`${API_BASE}/api/collections/posts/records?${params}`);
  return res.json();
}

// Get single post by slug
async function getPost(slug) {
  const params = new URLSearchParams({
    filter: `slug='${slug}'`,
    expand: 'author,tags',
  });

  const res = await fetch(`${API_BASE}/api/collections/posts/records?${params}`);
  const data = await res.json();
  return data.items[0] || null;
}

// Get comments for a post
async function getComments(postId) {
  const params = new URLSearchParams({
    filter: `post='${postId}'`,
    sort: '-created',
    expand: 'author',
  });

  const res = await fetch(`${API_BASE}/api/collections/comments/records?${params}`);
  return res.json();
}

// Login
async function login(email, password) {
  const res = await fetch(`${API_BASE}/api/collections/authors/auth-with-password`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ identity: email, password }),
  });
  return res.json(); // { token, record }
}

// Create a post (authenticated)
async function createPost(token, post) {
  const res = await fetch(`${API_BASE}/api/collections/posts/records`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${token}`,
    },
    body: JSON.stringify(post),
  });
  return res.json();
}

// Get file URL (e.g., cover image)
function getFileUrl(record, filename) {
  return `${API_BASE}/api/files/${record.collectionId}/${record.id}/${filename}`;
}

// Get thumbnail URL
function getThumbUrl(record, filename, dimensions = '400x300') {
  return `${getFileUrl(record, filename)}?thumb=${dimensions}`;
}
```

---

## Summary

| What You Built | How |
|---|---|
| Author registration & login | Auth collection + password auth |
| Blog posts with rich content | Base collection + editor field |
| Tags with many-to-many | Relation field with maxSelect > 1 |
| Comments with guest support | Optional relation + text fields |
| Content filtering | API rules with `status` checks |
| Owner-only editing | API rules with `@request.auth.id` |
| Image uploads | File fields + multipart upload |
| Full-text search | `searchable` fields + `search` param |
| Relation expansion | `expand` query parameter |

This pattern scales to any content-driven application: documentation sites, portfolios, news platforms, or knowledge bases.
