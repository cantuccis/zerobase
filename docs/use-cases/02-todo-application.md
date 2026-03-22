# Use Case: Building a Todo App with Zerobase

> Build a multi-user todo application with projects, tasks, due dates, priorities, and shared collaboration — all powered by Zerobase's auto-generated API.

---

## Overview

This guide covers:

1. Setting up user and project collections
2. Creating a task collection with priorities, due dates, and statuses
3. Implementing user-scoped access rules
4. Building task filtering, sorting, and batch operations
5. A working JavaScript frontend integration

---

## Prerequisites

- Zerobase server running at `http://localhost:8090`
- Superuser account created
- `curl` for API testing

---

## Step 1: Authenticate as Superuser

```bash
TOKEN=$(curl -s -X POST http://localhost:8090/_/api/admins/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "admin@example.com", "password": "admin123456"}' \
  | jq -r '.token')
```

---

## Step 2: Create the Users Collection

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "users",
    "type": "auth",
    "fields": [
      {
        "name": "name",
        "type": "text",
        "required": true,
        "options": { "min": 1, "max": 100 }
      }
    ],
    "listRule": "@request.auth.id != \"\"",
    "viewRule": "@request.auth.id != \"\"",
    "createRule": "",
    "updateRule": "@request.auth.id = id",
    "deleteRule": "@request.auth.id = id"
  }'
```

---

## Step 3: Create the Projects Collection

Projects group tasks and can be shared between users.

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "projects",
    "type": "base",
    "fields": [
      {
        "name": "name",
        "type": "text",
        "required": true,
        "searchable": true,
        "options": { "min": 1, "max": 200 }
      },
      {
        "name": "description",
        "type": "text",
        "options": { "max": 1000 }
      },
      {
        "name": "color",
        "type": "text",
        "options": { "pattern": "^#[0-9a-fA-F]{6}$" }
      },
      {
        "name": "owner",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "users",
          "maxSelect": 1
        }
      },
      {
        "name": "members",
        "type": "relation",
        "options": {
          "collectionId": "users",
          "maxSelect": 50
        }
      }
    ],
    "listRule": "owner = @request.auth.id || members ?= @request.auth.id",
    "viewRule": "owner = @request.auth.id || members ?= @request.auth.id",
    "createRule": "@request.auth.id != \"\"",
    "updateRule": "owner = @request.auth.id",
    "deleteRule": "owner = @request.auth.id"
  }'
```

**Key rule:** `members ?= @request.auth.id` uses the `?=` operator to check if the current user's ID is contained in the multi-relation `members` array.

---

## Step 4: Create the Tasks Collection

```bash
curl -X POST http://localhost:8090/api/collections \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "name": "tasks",
    "type": "base",
    "fields": [
      {
        "name": "title",
        "type": "text",
        "required": true,
        "searchable": true,
        "options": { "min": 1, "max": 500 }
      },
      {
        "name": "description",
        "type": "text",
        "searchable": true,
        "options": { "max": 5000 }
      },
      {
        "name": "status",
        "type": "select",
        "required": true,
        "options": { "values": ["todo", "in_progress", "done", "cancelled"] }
      },
      {
        "name": "priority",
        "type": "select",
        "required": true,
        "options": { "values": ["low", "medium", "high", "urgent"] }
      },
      {
        "name": "due_date",
        "type": "datetime"
      },
      {
        "name": "completed_at",
        "type": "datetime"
      },
      {
        "name": "project",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "projects",
          "maxSelect": 1
        }
      },
      {
        "name": "assignee",
        "type": "relation",
        "options": {
          "collectionId": "users",
          "maxSelect": 1
        }
      },
      {
        "name": "created_by",
        "type": "relation",
        "required": true,
        "options": {
          "collectionId": "users",
          "maxSelect": 1
        }
      },
      {
        "name": "labels",
        "type": "multiSelect",
        "options": { "values": ["bug", "feature", "chore", "docs", "design"] }
      },
      {
        "name": "attachments",
        "type": "file",
        "options": {
          "maxSelect": 5,
          "maxSize": 10485760
        }
      },
      {
        "name": "sort_order",
        "type": "number",
        "options": { "min": 0 }
      }
    ],
    "listRule": "project.owner = @request.auth.id || project.members ?= @request.auth.id",
    "viewRule": "project.owner = @request.auth.id || project.members ?= @request.auth.id",
    "createRule": "project.owner = @request.auth.id || project.members ?= @request.auth.id",
    "updateRule": "project.owner = @request.auth.id || project.members ?= @request.auth.id",
    "deleteRule": "project.owner = @request.auth.id"
  }'
```

**Key rule:** Tasks inherit access from their parent project via `project.owner` and `project.members` — this uses dot-notation to traverse relations in filter rules.

---

## Step 5: Register Users and Create Data

### Register two users

```bash
# User 1
USER1_RES=$(curl -s -X POST http://localhost:8090/api/collections/users/records \
  -H "Content-Type: application/json" \
  -d '{
    "email": "alice@example.com",
    "password": "password123456",
    "passwordConfirm": "password123456",
    "name": "Alice"
  }')
USER1_ID=$(echo $USER1_RES | jq -r '.id')

# User 2
USER2_RES=$(curl -s -X POST http://localhost:8090/api/collections/users/records \
  -H "Content-Type: application/json" \
  -d '{
    "email": "bob@example.com",
    "password": "password123456",
    "passwordConfirm": "password123456",
    "name": "Bob"
  }')
USER2_ID=$(echo $USER2_RES | jq -r '.id')

# Login as Alice
ALICE_TOKEN=$(curl -s -X POST http://localhost:8090/api/collections/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity": "alice@example.com", "password": "password123456"}' \
  | jq -r '.token')
```

### Create a project with a shared member

```bash
PROJECT_RES=$(curl -s -X POST http://localhost:8090/api/collections/projects/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d "{
    \"name\": \"Website Redesign\",
    \"description\": \"Complete overhaul of the company website\",
    \"color\": \"#4A90D9\",
    \"owner\": \"$USER1_ID\",
    \"members\": [\"$USER2_ID\"]
  }")
PROJECT_ID=$(echo $PROJECT_RES | jq -r '.id')
```

### Create tasks

```bash
# Task 1: High priority, assigned to Alice
curl -s -X POST http://localhost:8090/api/collections/tasks/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d "{
    \"title\": \"Design new homepage mockup\",
    \"description\": \"Create wireframes and high-fidelity mockups for the new homepage\",
    \"status\": \"in_progress\",
    \"priority\": \"high\",
    \"due_date\": \"2026-04-01T17:00:00Z\",
    \"project\": \"$PROJECT_ID\",
    \"assignee\": \"$USER1_ID\",
    \"created_by\": \"$USER1_ID\",
    \"labels\": [\"design\"],
    \"sort_order\": 1
  }"

# Task 2: Medium priority, assigned to Bob
curl -s -X POST http://localhost:8090/api/collections/tasks/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d "{
    \"title\": \"Set up CI/CD pipeline\",
    \"status\": \"todo\",
    \"priority\": \"medium\",
    \"due_date\": \"2026-04-15T17:00:00Z\",
    \"project\": \"$PROJECT_ID\",
    \"assignee\": \"$USER2_ID\",
    \"created_by\": \"$USER1_ID\",
    \"labels\": [\"chore\"],
    \"sort_order\": 2
  }"

# Task 3: Completed task
curl -s -X POST http://localhost:8090/api/collections/tasks/records \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d "{
    \"title\": \"Gather requirements\",
    \"status\": \"done\",
    \"priority\": \"high\",
    \"completed_at\": \"2026-03-18T14:00:00Z\",
    \"project\": \"$PROJECT_ID\",
    \"created_by\": \"$USER1_ID\",
    \"labels\": [\"docs\"],
    \"sort_order\": 0
  }"
```

---

## Step 6: Common Task Queries

### List all tasks in a project, sorted by priority

```bash
curl -s "http://localhost:8090/api/collections/tasks/records?\
filter=project='$PROJECT_ID'&\
sort=-priority,sort_order&\
expand=assignee"
```

### Filter incomplete tasks due this week

```bash
curl -s "http://localhost:8090/api/collections/tasks/records?\
filter=project='$PROJECT_ID' %26%26 status!='done' %26%26 status!='cancelled' %26%26 due_date<='2026-03-28T23:59:59Z'&\
sort=due_date"
```

### Get tasks assigned to current user

```bash
curl -s "http://localhost:8090/api/collections/tasks/records?\
filter=assignee='$USER1_ID'%26%26status!='done'&\
sort=-priority,due_date&\
expand=project" \
  -H "Authorization: Bearer $ALICE_TOKEN"
```

### Count tasks by status

```bash
# Count open tasks
curl -s "http://localhost:8090/api/collections/tasks/records/count?\
filter=project='$PROJECT_ID'%26%26status='todo'" \
  -H "Authorization: Bearer $ALICE_TOKEN"
```

### Search tasks

```bash
curl -s "http://localhost:8090/api/collections/tasks/records?\
search=homepage&\
expand=assignee,project" \
  -H "Authorization: Bearer $ALICE_TOKEN"
```

---

## Step 7: Update and Complete Tasks

### Mark a task as done

```bash
curl -X PATCH "http://localhost:8090/api/collections/tasks/records/TASK_ID" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d '{
    "status": "done",
    "completed_at": "2026-03-21T15:30:00Z"
  }'
```

### Reassign a task

```bash
curl -X PATCH "http://localhost:8090/api/collections/tasks/records/TASK_ID" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d "{\"assignee\": \"$USER2_ID\"}"
```

### Batch update: move multiple tasks to "in_progress"

```bash
curl -X POST http://localhost:8090/api/batch \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -d '{
    "requests": [
      {
        "method": "PATCH",
        "url": "/api/collections/tasks/records/TASK_ID_1",
        "body": { "status": "in_progress" }
      },
      {
        "method": "PATCH",
        "url": "/api/collections/tasks/records/TASK_ID_2",
        "body": { "status": "in_progress" }
      }
    ]
  }'
```

---

## Step 8: Frontend Integration (JavaScript)

```javascript
const API = 'http://localhost:8090';

class TodoClient {
  constructor() {
    this.token = null;
  }

  // --- Auth ---

  async login(email, password) {
    const res = await fetch(`${API}/api/collections/users/auth-with-password`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ identity: email, password }),
    });
    const data = await res.json();
    this.token = data.token;
    return data;
  }

  get headers() {
    return {
      'Content-Type': 'application/json',
      ...(this.token && { Authorization: `Bearer ${this.token}` }),
    };
  }

  // --- Projects ---

  async listProjects() {
    const res = await fetch(
      `${API}/api/collections/projects/records?sort=-created&expand=owner,members`,
      { headers: this.headers }
    );
    return res.json();
  }

  async createProject(name, description, color) {
    const res = await fetch(`${API}/api/collections/projects/records`, {
      method: 'POST',
      headers: this.headers,
      body: JSON.stringify({ name, description, color, owner: this.userId }),
    });
    return res.json();
  }

  // --- Tasks ---

  async listTasks(projectId, { status, priority, assignee, page = 1 } = {}) {
    const filters = [`project='${projectId}'`];
    if (status) filters.push(`status='${status}'`);
    if (priority) filters.push(`priority='${priority}'`);
    if (assignee) filters.push(`assignee='${assignee}'`);

    const params = new URLSearchParams({
      filter: filters.join(' && '),
      sort: '-priority,sort_order,due_date',
      expand: 'assignee',
      page: page.toString(),
      perPage: '50',
    });

    const res = await fetch(
      `${API}/api/collections/tasks/records?${params}`,
      { headers: this.headers }
    );
    return res.json();
  }

  async createTask(task) {
    const res = await fetch(`${API}/api/collections/tasks/records`, {
      method: 'POST',
      headers: this.headers,
      body: JSON.stringify(task),
    });
    return res.json();
  }

  async updateTask(taskId, updates) {
    const res = await fetch(`${API}/api/collections/tasks/records/${taskId}`, {
      method: 'PATCH',
      headers: this.headers,
      body: JSON.stringify(updates),
    });
    return res.json();
  }

  async completeTask(taskId) {
    return this.updateTask(taskId, {
      status: 'done',
      completed_at: new Date().toISOString(),
    });
  }

  async deleteTask(taskId) {
    await fetch(`${API}/api/collections/tasks/records/${taskId}`, {
      method: 'DELETE',
      headers: this.headers,
    });
  }

  // --- Drag & drop reorder ---

  async reorderTasks(taskOrders) {
    // taskOrders: [{ id: "...", sort_order: 0 }, { id: "...", sort_order: 1 }]
    const res = await fetch(`${API}/api/batch`, {
      method: 'POST',
      headers: this.headers,
      body: JSON.stringify({
        requests: taskOrders.map(({ id, sort_order }) => ({
          method: 'PATCH',
          url: `/api/collections/tasks/records/${id}`,
          body: { sort_order },
        })),
      }),
    });
    return res.json();
  }
}

// Usage
const client = new TodoClient();
await client.login('alice@example.com', 'password123456');
const projects = await client.listProjects();
const tasks = await client.listTasks(projects.items[0].id, { status: 'todo' });
```

---

## Summary

| Feature | Implementation |
|---|---|
| Multi-user with isolation | Auth collection + relation-based rules |
| Shared projects | Multi-relation `members` field with `?=` operator |
| Cascading access control | Dot-notation in rules (`project.owner`) |
| Priority & status tracking | Select fields with enumerated values |
| Due dates & completion | DateTime fields |
| File attachments | Multi-file field (maxSelect: 5) |
| Task search | Full-text search via `searchable` fields |
| Drag-and-drop ordering | `sort_order` number field + batch API |
| Bulk operations | Batch endpoint for atomic updates |
