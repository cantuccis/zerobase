# Use Case: Realtime Chat with SSE

> Build a multi-room chat application using Zerobase's Server-Sent Events (SSE) realtime subscriptions for live message delivery.

---

## Overview

This guide covers:

1. Designing chat room and message collections
2. Establishing SSE connections
3. Subscribing to realtime events
4. Sending and receiving messages in real time
5. Handling connection lifecycle (reconnect, disconnect)
6. Building a complete chat client in JavaScript

---

## Prerequisites

- Zerobase server running at `http://localhost:8090`
- Superuser account and at least one auth collection (`users`)

---

## Step 1: Create Chat Collections

### Rooms collection

```bash
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "admin@example.com", "password": "admin123456"}' \
  | jq -r '.token')

curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "rooms",
    "type": "base",
    "fields": [
      {
        "name": "name",
        "type": "text",
        "required": true,
        "unique": true
      },
      {
        "name": "description",
        "type": "text"
      },
      {
        "name": "members",
        "type": "relation",
        "options": {
          "collectionId": "users",
          "maxSelect": 100
        }
      },
      {
        "name": "is_public",
        "type": "bool"
      }
    ],
    "listRule": "is_public = true || members ?= @request.auth.id",
    "viewRule": "is_public = true || members ?= @request.auth.id",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "members ?= @request.auth.id",
    "deleteRule": null
  }'
```

### Messages collection

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "messages",
    "type": "base",
    "fields": [
      {
        "name": "room",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "rooms",
          "maxSelect": 1
        }
      },
      {
        "name": "sender",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "users",
          "maxSelect": 1
        }
      },
      {
        "name": "text",
        "type": "text",
        "required": true,
        "options": { "max": 5000 }
      },
      {
        "name": "attachment",
        "type": "file",
        "options": {
          "maxSelect": 1,
          "maxSize": 10485760
        }
      },
      {
        "name": "type",
        "type": "select",
        "required": true,
        "options": { "values": ["text", "image", "file", "system"] }
      }
    ],
    "listRule": "room.is_public = true || room.members ?= @request.auth.id",
    "viewRule": "room.is_public = true || room.members ?= @request.auth.id",
    "createRule": "room.is_public = true || room.members ?= @request.auth.id",
    "updateRule": "sender = @request.auth.id",
    "deleteRule": "sender = @request.auth.id"
  }'
```

---

## Step 2: Understanding SSE in Zerobase

Zerobase uses **Server-Sent Events (SSE)** for realtime — a unidirectional protocol where the server pushes events to the client over a long-lived HTTP connection.

### Connection protocol

1. **Connect:** `GET /api/realtime` — opens an SSE stream
2. **Receive client ID:** Server sends a `PB_CONNECT` event with your `clientId`
3. **Subscribe:** `POST /api/realtime` with `clientId` and topic list
4. **Receive events:** Server pushes `record_create`, `record_update`, `record_delete` events
5. **Keep-alive:** Server sends `: ping` comments every 30 seconds

### Event format

```
event: messages
data: {"action":"create","record":{"id":"msg_123","text":"Hello!","sender":"usr_abc",...}}
```

---

## Step 3: Connect and Subscribe (curl)

### Open SSE connection

```bash
# This is a long-lived connection — it stays open
curl -N "http://localhost:8090/api/realtime" \
  -H "Authorization: Bearer $USER_TOKEN"
```

**First event received:**
```
event: PB_CONNECT
data: {"clientId":"sse_abc123def"}
```

### Subscribe to a room's messages

In a separate terminal, subscribe using the client ID:

```bash
curl -X POST http://localhost:8090/api/realtime \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{
    "clientId": "sse_abc123def",
    "subscriptions": ["messages"]
  }'
```

Now, when anyone creates a message, the SSE connection receives:

```
event: messages
data: {"action":"create","record":{"id":"msg_789","room":"room_abc","sender":"usr_xyz","text":"Hey everyone!","type":"text","created":"2026-03-21T15:00:00Z"}}
```

### Subscribe to a specific room only

For targeted subscriptions, filter on the client side or subscribe to specific record IDs:

```bash
curl -X POST http://localhost:8090/api/realtime \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d '{
    "clientId": "sse_abc123def",
    "subscriptions": ["messages", "rooms/ROOM_ID"]
  }'
```

---

## Step 4: Send Messages

Messages are created via the standard record API — the SSE system automatically broadcasts them.

```bash
curl -X POST http://localhost:8090/api/collections/messages/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $USER_TOKEN" \
  -d "{
    \"room\": \"ROOM_ID\",
    \"sender\": \"$USER_ID\",
    \"text\": \"Hello, world!\",
    \"type\": \"text\"
  }"
```

All connected clients subscribed to `messages` will receive the event in real time.

### Send a message with an attachment

```bash
curl -X POST http://localhost:8090/api/collections/messages/records \
  -H "Authorization: Bearer $USER_TOKEN" \
  -F "room=ROOM_ID" \
  -F "sender=$USER_ID" \
  -F "text=Check out this photo" \
  -F "type=image" \
  -F "attachment=@/path/to/photo.jpg"
```

---

## Step 5: Load Message History

Before connecting to realtime, load existing messages:

```bash
# Get the last 50 messages in a room, newest first
curl -s "http://localhost:8090/api/collections/messages/records?\
filter=room='ROOM_ID'&\
sort=-created&\
perPage=50&\
expand=sender" \
  -H "Authorization: Bearer $USER_TOKEN"
```

### Infinite scroll (load older messages)

```bash
# Load page 2 (messages 51-100)
curl -s "http://localhost:8090/api/collections/messages/records?\
filter=room='ROOM_ID'&\
sort=-created&\
page=2&perPage=50&\
expand=sender" \
  -H "Authorization: Bearer $USER_TOKEN"
```

---

## Step 6: Complete Chat Client (JavaScript)

```javascript
class RealtimeChat {
  constructor(apiBase) {
    this.api = apiBase;
    this.token = null;
    this.userId = null;
    this.clientId = null;
    this.eventSource = null;
    this.listeners = new Map();
    this.reconnectAttempts = 0;
    this.maxReconnectAttempts = 10;
  }

  // --- Auth ---

  async login(email, password) {
    const res = await fetch(`${this.api}/api/collections/users/auth-with-password`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ identity: email, password }),
    }).then(r => r.json());

    this.token = res.token;
    this.userId = res.record.id;
    return res;
  }

  get headers() {
    return {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${this.token}`,
    };
  }

  // --- Realtime Connection ---

  connect() {
    return new Promise((resolve, reject) => {
      // Close existing connection
      if (this.eventSource) {
        this.eventSource.close();
      }

      // Open SSE connection
      // Note: EventSource doesn't support custom headers natively.
      // Pass the token as a query parameter or use a polyfill.
      this.eventSource = new EventSource(
        `${this.api}/api/realtime?token=${this.token}`
      );

      // Handle the initial PB_CONNECT event
      this.eventSource.addEventListener('PB_CONNECT', (event) => {
        const data = JSON.parse(event.data);
        this.clientId = data.clientId;
        this.reconnectAttempts = 0;
        console.log('Connected with clientId:', this.clientId);
        resolve(this.clientId);
      });

      // Handle errors and reconnection
      this.eventSource.onerror = () => {
        console.warn('SSE connection error');
        if (this.reconnectAttempts < this.maxReconnectAttempts) {
          this.reconnectAttempts++;
          const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
          console.log(`Reconnecting in ${delay}ms (attempt ${this.reconnectAttempts})`);
          setTimeout(() => this.connect().then(() => {
            // Re-subscribe after reconnection
            if (this._lastSubscriptions) {
              this.subscribe(this._lastSubscriptions);
            }
          }), delay);
        } else {
          reject(new Error('Max reconnection attempts reached'));
        }
      };
    });
  }

  // Subscribe to topics
  async subscribe(topics) {
    this._lastSubscriptions = topics;

    await fetch(`${this.api}/api/realtime`, {
      method: 'POST',
      headers: this.headers,
      body: JSON.stringify({
        clientId: this.clientId,
        subscriptions: topics,
      }),
    });

    // Set up event listeners for each topic
    for (const topic of topics) {
      const collectionName = topic.split('/')[0]; // "messages" or "messages/id"
      this.eventSource.addEventListener(collectionName, (event) => {
        const data = JSON.parse(event.data);
        this._emit(collectionName, data);
      });
    }
  }

  // Event listener management
  on(event, callback) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, []);
    }
    this.listeners.get(event).push(callback);
  }

  off(event, callback) {
    const cbs = this.listeners.get(event) || [];
    this.listeners.set(event, cbs.filter(cb => cb !== callback));
  }

  _emit(event, data) {
    const cbs = this.listeners.get(event) || [];
    cbs.forEach(cb => cb(data));
  }

  // --- Rooms ---

  async createRoom(name, description, isPublic = true) {
    const res = await fetch(`${this.api}/api/collections/rooms/records`, {
      method: 'POST',
      headers: this.headers,
      body: JSON.stringify({
        name,
        description,
        is_public: isPublic,
        members: [this.userId],
      }),
    });
    return res.json();
  }

  async listRooms() {
    const res = await fetch(
      `${this.api}/api/collections/rooms/records?sort=-created&expand=members`,
      { headers: this.headers }
    );
    return res.json();
  }

  async joinRoom(roomId) {
    // Add current user to members using the + modifier
    const res = await fetch(`${this.api}/api/collections/rooms/records/${roomId}`, {
      method: 'PATCH',
      headers: this.headers,
      body: JSON.stringify({ 'members+': [this.userId] }),
    });
    return res.json();
  }

  // --- Messages ---

  async loadHistory(roomId, page = 1, perPage = 50) {
    const params = new URLSearchParams({
      filter: `room='${roomId}'`,
      sort: '-created',
      page: page.toString(),
      perPage: perPage.toString(),
      expand: 'sender',
    });

    const res = await fetch(
      `${this.api}/api/collections/messages/records?${params}`,
      { headers: this.headers }
    );
    return res.json();
  }

  async sendMessage(roomId, text, type = 'text') {
    const res = await fetch(`${this.api}/api/collections/messages/records`, {
      method: 'POST',
      headers: this.headers,
      body: JSON.stringify({
        room: roomId,
        sender: this.userId,
        text,
        type,
      }),
    });
    return res.json();
  }

  async sendFileMessage(roomId, text, file) {
    const formData = new FormData();
    formData.append('room', roomId);
    formData.append('sender', this.userId);
    formData.append('text', text);
    formData.append('type', 'file');
    formData.append('attachment', file);

    const res = await fetch(`${this.api}/api/collections/messages/records`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${this.token}` },
      body: formData,
    });
    return res.json();
  }

  async deleteMessage(messageId) {
    await fetch(`${this.api}/api/collections/messages/records/${messageId}`, {
      method: 'DELETE',
      headers: this.headers,
    });
  }

  async editMessage(messageId, newText) {
    const res = await fetch(`${this.api}/api/collections/messages/records/${messageId}`, {
      method: 'PATCH',
      headers: this.headers,
      body: JSON.stringify({ text: newText }),
    });
    return res.json();
  }

  // Disconnect
  disconnect() {
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }
    this.clientId = null;
  }
}
```

### Using the chat client

```javascript
// Initialize
const chat = new RealtimeChat('http://localhost:8090');
await chat.login('alice@example.com', 'password123');

// Connect to realtime
await chat.connect();

// Subscribe to messages
await chat.subscribe(['messages', 'rooms']);

// Listen for new messages
chat.on('messages', (event) => {
  if (event.action === 'create') {
    const msg = event.record;
    console.log(`New message from ${msg.sender}: ${msg.text}`);
    appendMessageToUI(msg);
  }

  if (event.action === 'update') {
    const msg = event.record;
    console.log(`Message edited: ${msg.text}`);
    updateMessageInUI(msg);
  }

  if (event.action === 'delete') {
    console.log(`Message deleted: ${event.record.id}`);
    removeMessageFromUI(event.record.id);
  }
});

// Listen for room changes
chat.on('rooms', (event) => {
  if (event.action === 'create') {
    console.log(`New room: ${event.record.name}`);
  }
});

// Load history for a room
const history = await chat.loadHistory('ROOM_ID');
history.items.reverse().forEach(msg => appendMessageToUI(msg));

// Send a message
await chat.sendMessage('ROOM_ID', 'Hello everyone!');

// Clean up on page unload
window.addEventListener('beforeunload', () => chat.disconnect());
```

### Rendering messages (HTML example)

```javascript
function appendMessageToUI(msg) {
  const container = document.getElementById('messages');
  const senderName = msg.expand?.sender?.name || msg.sender;

  const div = document.createElement('div');
  div.id = `msg-${msg.id}`;
  div.className = 'message';
  div.innerHTML = `
    <div class="message-header">
      <strong>${senderName}</strong>
      <time>${new Date(msg.created).toLocaleTimeString()}</time>
    </div>
    <div class="message-body">${escapeHtml(msg.text)}</div>
    ${msg.attachment ? `<div class="message-attachment">
      <a href="${chat.api}/api/files/${msg.collectionId}/${msg.id}/${msg.attachment}"
         target="_blank">📎 ${msg.attachment}</a>
    </div>` : ''}
  `;

  container.appendChild(div);
  container.scrollTop = container.scrollHeight;
}

function updateMessageInUI(msg) {
  const el = document.getElementById(`msg-${msg.id}`);
  if (el) {
    el.querySelector('.message-body').textContent = msg.text;
  }
}

function removeMessageFromUI(msgId) {
  const el = document.getElementById(`msg-${msgId}`);
  if (el) el.remove();
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}
```

---

## Step 7: Handling Edge Cases

### Typing indicators

Use a lightweight "typing" collection or metadata:

```bash
# Create a presence collection
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "typing_status",
    "type": "base",
    "fields": [
      {"name": "room", "type": "relation", "required": true, "options": {"collectionId": "rooms", "maxSelect": 1}},
      {"name": "user", "type": "relation", "required": true, "options": {"collectionId": "users", "maxSelect": 1}},
      {"name": "is_typing", "type": "bool"}
    ],
    "listRule": "",
    "viewRule": "",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "user = @request.auth.id",
    "deleteRule": "user = @request.auth.id"
  }'
```

Subscribe to `typing_status` and update UI accordingly.

### Unread message count

Track the last-read message per user per room:

```bash
curl -s "http://localhost:8090/api/collections/messages/records/count?\
filter=room='ROOM_ID' %26%26 created>'2026-03-21T14:00:00Z'" \
  -H "Authorization: Bearer $USER_TOKEN"
```

---

## Summary

| Feature | Implementation |
|---|---|
| Realtime connection | `GET /api/realtime` (SSE) |
| Client identification | `PB_CONNECT` event with `clientId` |
| Topic subscription | `POST /api/realtime` with topics array |
| Message delivery | Auto-broadcast on record create/update/delete |
| Access control | SSE respects collection API rules |
| Reconnection | Exponential backoff with re-subscription |
| Message history | Standard list API with pagination |
| File sharing | Multipart upload + file field |
| Typing indicators | Lightweight presence collection |
| Keep-alive | Automatic `: ping` every 30 seconds |
