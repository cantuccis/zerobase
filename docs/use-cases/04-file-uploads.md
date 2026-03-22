# Use Case: File Uploads and Storage

> A complete guide to uploading, serving, protecting, and generating thumbnails for files using Zerobase's file storage system with both local and S3 backends.

---

## Overview

This guide covers:

1. Configuring file fields on collections
2. Uploading files via multipart/form-data
3. Downloading and serving files
4. Protected file access with tokens
5. Thumbnail generation
6. Multi-file uploads
7. Replacing and deleting files
8. S3 backend configuration
9. Building a file gallery frontend

---

## Prerequisites

- Zerobase server running at `http://localhost:8090`
- Superuser account created
- Sample image files for testing

---

## Step 1: Create a Collection with File Fields

### Photo gallery collection

```bash
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "admin@example.com", "password": "admin123456"}' \
  | jq -r '.token')

curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "photos",
    "type": "base",
    "fields": [
      {
        "name": "title",
        "type": "text",
        "required": true
      },
      {
        "name": "image",
        "type": "file",
        "required": true,
        "options": {
          "maxSelect": 1,
          "maxSize": 10485760,
          "mimeTypes": ["image/jpeg", "image/png", "image/webp", "image/gif"]
        }
      },
      {
        "name": "gallery",
        "type": "file",
        "options": {
          "maxSelect": 20,
          "maxSize": 10485760,
          "mimeTypes": ["image/jpeg", "image/png", "image/webp"]
        }
      },
      {
        "name": "document",
        "type": "file",
        "options": {
          "maxSelect": 1,
          "maxSize": 52428800,
          "mimeTypes": ["application/pdf", "application/msword", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
          "protected": true
        }
      },
      {
        "name": "owner",
        "type": "relation",
        "options": {
          "collectionId": "users",
          "maxSelect": 1
        }
      }
    ],
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "owner = @request.auth.id",
    "deleteRule": "owner = @request.auth.id"
  }'
```

### File field options explained

| Option | Type | Description |
|---|---|---|
| `maxSelect` | int | `1` for single file, `>1` for multiple files |
| `maxSize` | int | Maximum file size in bytes (10485760 = 10MB) |
| `mimeTypes` | string[] | Allowed MIME types (empty = allow all) |
| `protected` | bool | Requires a file token to download |

---

## Step 2: Upload a Single File

Use `multipart/form-data` — not JSON — for file uploads.

```bash
# Login first
USER_TOKEN=$(curl -s -X POST http://localhost:8090/api/collections/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "user@example.com", "password": "password123456"}' \
  | jq -r '.token')

USER_ID=$(curl -s -X POST http://localhost:8090/api/collections/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "user@example.com", "password": "password123456"}' \
  | jq -r '.record.id')

# Upload a photo with metadata
curl -X POST http://localhost:8090/api/collections/photos/records \
  -H "Authorization: Bearer $USER_TOKEN" \
  -F "title=Sunset at the beach" \
  -F "owner=$USER_ID" \
  -F "image=@/path/to/sunset.jpg"
```

**Response:**
```json
{
  "id": "rec_abc123",
  "collectionId": "col_photos",
  "collectionName": "photos",
  "title": "Sunset at the beach",
  "image": "a1b2c3d4_sunset.jpg",
  "gallery": [],
  "document": "",
  "owner": "usr_xyz",
  "created": "2026-03-21T10:00:00Z",
  "updated": "2026-03-21T10:00:00Z"
}
```

The file is stored with a random prefix: `a1b2c3d4_sunset.jpg`.

---

## Step 3: Upload Multiple Files

For multi-file fields (`maxSelect > 1`), send multiple form fields with the same name:

```bash
curl -X POST http://localhost:8090/api/collections/photos/records \
  -H "Authorization: Bearer $USER_TOKEN" \
  -F "title=Beach Trip" \
  -F "owner=$USER_ID" \
  -F "image=@/path/to/main.jpg" \
  -F "gallery=@/path/to/photo1.jpg" \
  -F "gallery=@/path/to/photo2.jpg" \
  -F "gallery=@/path/to/photo3.jpg"
```

**Response:**
```json
{
  "id": "rec_def456",
  "title": "Beach Trip",
  "image": "x1y2z3_main.jpg",
  "gallery": [
    "a1b2c3_photo1.jpg",
    "d4e5f6_photo2.jpg",
    "g7h8i9_photo3.jpg"
  ]
}
```

---

## Step 4: Download Files

### Public file URL

Files from non-protected fields are publicly accessible:

```
GET http://localhost:8090/api/files/{collectionId}/{recordId}/{filename}
```

Example:

```bash
curl -o sunset.jpg \
  "http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg"
```

### Build the URL from a record

```javascript
function getFileUrl(record, fieldName) {
  const filename = record[fieldName];
  if (!filename) return null;
  return `http://localhost:8090/api/files/${record.collectionId}/${record.id}/${filename}`;
}

// For multi-file fields
function getFileUrls(record, fieldName) {
  const filenames = record[fieldName] || [];
  return filenames.map(f =>
    `http://localhost:8090/api/files/${record.collectionId}/${record.id}/${f}`
  );
}
```

### Force download

Add `?download=1` to trigger a `Content-Disposition: attachment` header:

```
http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?download=1
```

---

## Step 5: Protected Files

Files in fields with `protected: true` require a short-lived file token.

### Get a file token

```bash
FILE_TOKEN=$(curl -s http://localhost:8090/api/files/token \
  -H "Authorization: Bearer $USER_TOKEN" \
  | jq -r '.token')
```

The token is valid for ~2 minutes.

### Download a protected file

```bash
curl -o document.pdf \
  "http://localhost:8090/api/files/col_photos/rec_abc123/x1y2z3_report.pdf?token=$FILE_TOKEN"
```

### Frontend pattern for protected files

```javascript
async function getProtectedFileUrl(record, fieldName, authToken) {
  // Get a short-lived file token
  const { token: fileToken } = await fetch(`${API}/api/files/token`, {
    headers: { Authorization: `Bearer ${authToken}` },
  }).then(r => r.json());

  const filename = record[fieldName];
  return `${API}/api/files/${record.collectionId}/${record.id}/${filename}?token=${fileToken}`;
}

// Usage in <img> or <a> tag
const url = await getProtectedFileUrl(record, 'document', authToken);
window.open(url); // Opens the protected PDF
```

---

## Step 6: Thumbnail Generation

Zerobase auto-generates thumbnails for image files on the fly.

### Thumbnail URL format

```
/api/files/{collectionId}/{recordId}/{filename}?thumb={spec}
```

### Thumbnail specifications

| Spec | Mode | Description |
|---|---|---|
| `100x100` | Center crop | Crops to exact dimensions from center |
| `200x100t` | Top crop | Crops from the top edge |
| `200x100b` | Bottom crop | Crops from the bottom edge |
| `300x300f` | Fit | Scales to fit within dimensions (preserves aspect ratio) |

### Examples

```bash
# Square thumbnail (center crop)
curl -o thumb.jpg \
  "http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?thumb=200x200"

# Large preview (fit within bounds)
curl -o preview.jpg \
  "http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?thumb=800x600f"

# Banner crop from top
curl -o banner.jpg \
  "http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?thumb=1200x400t"
```

### Using thumbnails in HTML

```html
<!-- Responsive image with srcset -->
<img
  src="http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?thumb=400x300"
  srcset="
    http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?thumb=400x300 400w,
    http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg?thumb=800x600 800w,
    http://localhost:8090/api/files/col_photos/rec_abc123/a1b2c3d4_sunset.jpg 1200w
  "
  sizes="(max-width: 600px) 400px, 800px"
  alt="Sunset at the beach"
  loading="lazy"
/>
```

---

## Step 7: Replace and Remove Files

### Replace a file

Upload a new file to the same field — the old file is automatically deleted:

```bash
curl -X PATCH "http://localhost:8090/api/collections/photos/records/rec_abc123" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -F "image=@/path/to/new-photo.jpg"
```

### Remove a file (set field to empty)

```bash
# For single file fields
curl -X PATCH "http://localhost:8090/api/collections/photos/records/rec_abc123" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{"image": ""}'
```

### Remove specific files from multi-file fields

Use the `-` prefix to remove specific files:

```bash
curl -X PATCH "http://localhost:8090/api/collections/photos/records/rec_def456" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{"gallery-": ["d4e5f6_photo2.jpg"]}'
```

### Add files to existing multi-file fields

Use the `+` suffix to append without replacing:

```bash
curl -X PATCH "http://localhost:8090/api/collections/photos/records/rec_def456" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -F "gallery+=@/path/to/photo4.jpg" \
  -F "gallery+=@/path/to/photo5.jpg"
```

---

## Step 8: Configure S3 Storage Backend

For production, use S3-compatible storage (AWS S3, MinIO, DigitalOcean Spaces).

### Update settings (superuser)

```bash
curl -X PATCH http://localhost:8090/api/settings \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "s3": {
      "enabled": true,
      "bucket": "my-zerobase-files",
      "region": "us-east-1",
      "endpoint": "https://s3.amazonaws.com",
      "accessKey": "AKIAIOSFODNN7EXAMPLE",
      "secret": "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
      "forcePathStyle": false
    }
  }'
```

### MinIO configuration (self-hosted S3)

```bash
curl -X PATCH http://localhost:8090/api/settings \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "s3": {
      "enabled": true,
      "bucket": "zerobase",
      "region": "us-east-1",
      "endpoint": "http://minio:9000",
      "accessKey": "minioadmin",
      "secret": "minioadmin",
      "forcePathStyle": true
    }
  }'
```

The API for uploading/downloading files stays identical — the storage backend is transparent to clients.

---

## Step 9: File Gallery Frontend (JavaScript)

```javascript
class FileGallery {
  constructor(apiBase, authToken) {
    this.api = apiBase;
    this.token = authToken;
  }

  get headers() {
    return { Authorization: `Bearer ${this.token}` };
  }

  // Upload a photo with gallery images
  async uploadPhoto(title, mainImage, galleryImages = []) {
    const formData = new FormData();
    formData.append('title', title);
    formData.append('image', mainImage);
    for (const img of galleryImages) {
      formData.append('gallery', img);
    }

    const res = await fetch(`${this.api}/api/collections/photos/records`, {
      method: 'POST',
      headers: this.headers,
      body: formData,
    });
    return res.json();
  }

  // Get file URL with optional thumbnail
  fileUrl(record, fieldName, thumb = null) {
    const filename = record[fieldName];
    if (!filename) return null;
    let url = `${this.api}/api/files/${record.collectionId}/${record.id}/${filename}`;
    if (thumb) url += `?thumb=${thumb}`;
    return url;
  }

  // Get all URLs for a multi-file field
  fileUrls(record, fieldName, thumb = null) {
    const filenames = record[fieldName] || [];
    return filenames.map(f => {
      let url = `${this.api}/api/files/${record.collectionId}/${record.id}/${f}`;
      if (thumb) url += `?thumb=${thumb}`;
      return url;
    });
  }

  // Get a protected file URL
  async protectedUrl(record, fieldName) {
    const { token } = await fetch(`${this.api}/api/files/token`, {
      headers: this.headers,
    }).then(r => r.json());

    const filename = record[fieldName];
    return `${this.api}/api/files/${record.collectionId}/${record.id}/${filename}?token=${token}`;
  }

  // List photos with pagination
  async listPhotos(page = 1, perPage = 20) {
    const params = new URLSearchParams({
      sort: '-created',
      page: page.toString(),
      perPage: perPage.toString(),
    });

    const res = await fetch(
      `${this.api}/api/collections/photos/records?${params}`,
      { headers: this.headers }
    );
    return res.json();
  }

  // Add images to an existing gallery
  async addToGallery(recordId, newImages) {
    const formData = new FormData();
    for (const img of newImages) {
      formData.append('gallery+', img);
    }

    const res = await fetch(
      `${this.api}/api/collections/photos/records/${recordId}`,
      {
        method: 'PATCH',
        headers: this.headers,
        body: formData,
      }
    );
    return res.json();
  }

  // Remove specific gallery images
  async removeFromGallery(recordId, filenames) {
    const res = await fetch(
      `${this.api}/api/collections/photos/records/${recordId}`,
      {
        method: 'PATCH',
        headers: {
          ...this.headers,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ 'gallery-': filenames }),
      }
    );
    return res.json();
  }
}

// Usage with file input
const gallery = new FileGallery('http://localhost:8090', userToken);

// Upload from a form
const fileInput = document.querySelector('#file-input');
const record = await gallery.uploadPhoto(
  'My Vacation',
  fileInput.files[0],              // main image
  Array.from(fileInput.files).slice(1)  // gallery images
);

// Render thumbnails
const photos = await gallery.listPhotos();
photos.items.forEach(photo => {
  const thumbUrl = gallery.fileUrl(photo, 'image', '200x200');
  const fullUrl = gallery.fileUrl(photo, 'image');
  // Render in your UI
});
```

---

## Summary

| Feature | How |
|---|---|
| Single file upload | `multipart/form-data` with `-F "field=@file"` |
| Multi-file upload | Repeat the field: `-F "field=@file1" -F "field=@file2"` |
| File access | `GET /api/files/{collectionId}/{recordId}/{filename}` |
| Protected files | Get token from `/api/files/token`, pass as `?token=` |
| Thumbnails | Append `?thumb=WxH` (or `WxHt`, `WxHb`, `WxHf`) |
| Replace file | PATCH with new file in same field |
| Remove from multi | Use `field-` modifier with filenames to remove |
| Add to multi | Use `field+` modifier to append files |
| S3 storage | Configure via settings API — transparent to clients |
| MIME validation | Set `mimeTypes` in field options |
| Size limits | Set `maxSize` in field options (bytes) |
