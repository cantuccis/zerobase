use anyhow::{ensure, Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Algorithm, Argon2, Params, PasswordHasher, Version,
};
use rand::Rng;
use reqwest::{Client, Method};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

const BASE: &str = "http://127.0.0.1:9099";
const BINARY: &str = "./zerobase";
const DATA_DIR: &str = "zerobase_data";
const DB_PATH: &str = "zerobase_data/data.db";

const ADMIN_EMAIL: &str = "admin@test.com";
const ADMIN_PASS: &str = "adminpassword1";

const ALICE_EMAIL: &str = "alice@test.com";
const ALICE_PASS: &str = "alicepass123";
const ALICE_NAME: &str = "Alice";

const BOB_EMAIL: &str = "bob@test.com";
const BOB_PASS: &str = "bobpassword1";
const BOB_NAME: &str = "Bob";

const CHARLIE_EMAIL: &str = "charlie@test.com";
const CHARLIE_PASS: &str = "charliepass1";
const CHARLIE_NAME: &str = "Charlie";

// ── Utilities ────────────────────────────────────────────────────────────────

fn generate_id() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..15).map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char).collect()
}

fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(19_456, 2, 1, None).map_err(|e| anyhow::anyhow!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash error: {e}"))?;
    Ok(hash.to_string())
}

fn generate_token_key() -> String {
    let mut rng = rand::thread_rng();
    (0..32).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

fn now_iso() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    let secs = dur.as_secs();
    let d = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let days_since_epoch = d as i64;
    let mut y = 1970i64;
    let mut days_left = days_since_epoch;
    loop {
        let is_leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if days_left < days_in_year {
            break;
        }
        days_left -= days_in_year;
        y += 1;
    }
    let is_leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if is_leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut mo = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if days_left < md {
            mo = i;
            break;
        }
        days_left -= md;
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        mo + 1,
        days_left + 1,
        h,
        m,
        s
    )
}

// ── Runner ───────────────────────────────────────────────────────────────────

struct Runner {
    passed: u32,
    failed: u32,
    errors: Vec<String>,
}

impl Runner {
    fn new() -> Self { Self { passed: 0, failed: 0, errors: vec![] } }
    fn section(&self, name: &str) { println!("\n--- {name} ---"); }
    fn pass(&mut self, name: &str) { self.passed += 1; println!("  PASS  {name}"); }
    fn check(&mut self, name: &str, result: Result<()>) {
        match result {
            Ok(()) => self.pass(name),
            Err(e) => {
                self.failed += 1;
                let msg = format!("{name}: {e:#}");
                println!("  FAIL  {msg}");
                self.errors.push(msg);
            }
        }
    }
    fn report(&self) {
        println!("\n========================================");
        println!("  {} passed, {} failed", self.passed, self.failed);
        if !self.errors.is_empty() {
            println!("\nFailures:");
            for e in &self.errors { println!("  - {e}"); }
        }
        println!("========================================\n");
    }
}

// ── HTTP helpers ─────────────────────────────────────────────────────────────

async fn api(
    client: &Client, method: Method, path: &str, token: Option<&str>, body: Option<Value>,
) -> Result<(u16, Value)> {
    let url = format!("{BASE}{path}");
    let mut req = client.request(method, &url);
    if let Some(t) = token { req = req.header("Authorization", t); }
    if let Some(b) = body { req = req.json(&b); }
    let resp = req.send().await.context("request failed")?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let body = if text.is_empty() { Value::Null }
    else { serde_json::from_str(&text).unwrap_or(Value::String(text)) };
    Ok((status, body))
}

async fn post(client: &Client, path: &str, token: &str, body: Value) -> Result<Value> {
    let (s, b) = api(client, Method::POST, path, Some(token), Some(body)).await?;
    ensure!((200..300).contains(&s), "POST {path} -> {s}: {b}");
    Ok(b)
}

fn str_val(v: &Value, key: &str) -> Result<String> {
    Ok(v[key].as_str().with_context(|| format!("missing '{key}' in {v}"))?.to_string())
}

// ── Server lifecycle ─────────────────────────────────────────────────────────

fn clean_data() -> Result<()> {
    let p = Path::new(DATA_DIR);
    if p.exists() { std::fs::remove_dir_all(p).context("failed to clean data dir")?; }
    Ok(())
}

fn run_superuser_create() -> Result<()> {
    let out = Command::new(BINARY)
        .args(["superuser", "create", "--email", ADMIN_EMAIL, "--password", ADMIN_PASS])
        .output().context("failed to run superuser create")?;
    ensure!(out.status.success(), "superuser create failed: {}", String::from_utf8_lossy(&out.stderr));
    Ok(())
}

fn start_server() -> Result<Child> {
    Command::new(BINARY).args(["serve"]).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().context("failed to start server")
}

fn stop_server(server: &mut Child) {
    let _ = server.kill();
    let _ = server.wait();
}

async fn wait_for_ready(client: &Client) -> Result<()> {
    for _ in 0..30 {
        if let Ok(r) = client.get(&format!("{BASE}/api/health")).send().await {
            if r.status().is_success() { return Ok(()); }
        }
        sleep(Duration::from_millis(500)).await;
    }
    anyhow::bail!("server not ready within 15s")
}

async fn admin_auth(client: &Client) -> Result<String> {
    let (s, b) = api(client, Method::POST, "/_/api/admins/auth-with-password", None,
        Some(json!({"identity": ADMIN_EMAIL, "password": ADMIN_PASS}))).await?;
    ensure!((200..300).contains(&s), "admin auth failed: {s} {b}");
    str_val(&b, "token")
}

// ── Collection setup via REST API ────────────────────────────────────────────

async fn setup_collections(client: &Client, admin: &str) -> Result<()> {
    let users_col = post(client, "/api/collections", admin, json!({
        "name": "users", "type": "auth",
        "fields": [
            { "name": "name", "type": { "type": "text", "options": {} }, "required": true }
        ],
        "authOptions": { "allowEmailAuth": true, "requireEmail": true, "minPasswordLength": 8 }
    })).await.context("create users collection")?;
    let users_cid = str_val(&users_col, "id")?;

    let dirs_col = post(client, "/api/collections", admin, json!({
        "name": "directories", "type": "base",
        "fields": [
            { "name": "name", "type": { "type": "text", "options": {} }, "required": true },
            { "name": "owner", "type": { "type": "relation", "options": { "collectionId": &users_cid, "maxSelect": 1 } }, "required": true }
        ]
    })).await.context("create directories collection")?;
    let dirs_cid = str_val(&dirs_col, "id")?;

    post(client, "/api/collections", admin, json!({
        "name": "shares", "type": "base",
        "fields": [
            { "name": "directory", "type": { "type": "relation", "options": { "collectionId": &dirs_cid, "maxSelect": 1 } }, "required": true },
            { "name": "shared_with", "type": { "type": "relation", "options": { "collectionId": &users_cid, "maxSelect": 1 } }, "required": true }
        ]
    })).await.context("create shares collection")?;

    post(client, "/api/collections", admin, json!({
        "name": "files", "type": "base",
        "fields": [
            { "name": "name", "type": { "type": "text", "options": {} }, "required": true },
            { "name": "size", "type": { "type": "number", "options": {} } },
            { "name": "mime_type", "type": { "type": "text", "options": {} } },
            { "name": "file_data", "type": { "type": "file", "options": { "maxSelect": 1, "maxSize": 10485760 } } },
            { "name": "owner", "type": { "type": "relation", "options": { "collectionId": &users_cid, "maxSelect": 1 } }, "required": true },
            { "name": "directory", "type": { "type": "relation", "options": { "collectionId": &dirs_cid, "maxSelect": 1 } } }
        ]
    })).await.context("create files collection")?;

    println!("  Collections created: users, directories, shares, files");
    Ok(())
}

// ── Direct DB helpers ────────────────────────────────────────────────────────

struct UserRecord { id: String }

fn insert_user(conn: &Connection, email: &str, password: &str, name: &str) -> Result<UserRecord> {
    let id = generate_id();
    let pw_hash = hash_password(password)?;
    let token_key = generate_token_key();
    let now = now_iso();
    conn.execute(
        "INSERT INTO users (id, email, password, tokenKey, verified, emailVisibility, name, created, updated)
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?6, ?7)",
        params![id, email, pw_hash, token_key, name, now, now],
    ).context("insert user")?;
    Ok(UserRecord { id })
}

fn insert_directory(conn: &Connection, name: &str, owner: &str) -> Result<String> {
    let id = generate_id();
    let now = now_iso();
    conn.execute(
        "INSERT INTO directories (id, name, owner, created, updated) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, owner, now, now],
    ).context("insert directory")?;
    Ok(id)
}

fn insert_file(conn: &Connection, name: &str, size: i64, mime: &str, owner: &str, directory: Option<&str>) -> Result<String> {
    let id = generate_id();
    let now = now_iso();
    conn.execute(
        "INSERT INTO files (id, name, size, mime_type, owner, directory, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, name, size, mime, owner, directory.unwrap_or(""), now, now],
    ).context("insert file")?;
    Ok(id)
}

fn insert_share(conn: &Connection, directory: &str, shared_with: &str) -> Result<String> {
    let id = generate_id();
    let now = now_iso();
    conn.execute(
        "INSERT INTO shares (id, directory, shared_with, created, updated) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, directory, shared_with, now, now],
    ).context("insert share")?;
    Ok(id)
}

fn delete_record(conn: &Connection, table: &str, id: &str) -> Result<()> {
    conn.execute(&format!("DELETE FROM {table} WHERE id = ?1"), params![id])?;
    Ok(())
}

fn count_records(conn: &Connection, table: &str, filter: &str) -> Result<usize> {
    let sql = if filter.is_empty() {
        format!("SELECT COUNT(*) FROM {table}")
    } else {
        format!("SELECT COUNT(*) FROM {table} WHERE {filter}")
    };
    let count: usize = conn.query_row(&sql, [], |row| row.get(0))?;
    Ok(count)
}

fn get_record(conn: &Connection, table: &str, id: &str) -> Result<bool> {
    let count: usize = conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
        params![id], |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Directories visible to a user: owned OR shared with them.
fn visible_dir_ids(conn: &Connection, user_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT d.id FROM directories d
         LEFT JOIN shares s ON s.directory = d.id
         WHERE d.owner = ?1 OR s.shared_with = ?1"
    )?;
    let ids: Vec<String> = stmt.query_map(params![user_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Files visible to a user: owned by them, OR in a directory they can access.
fn visible_file_ids(conn: &Connection, user_id: &str) -> Result<Vec<String>> {
    let dir_ids = visible_dir_ids(conn, user_id)?;
    let mut ids: Vec<String> = Vec::new();

    let mut stmt = conn.prepare("SELECT id FROM files WHERE owner = ?1")?;
    let owned: Vec<String> = stmt.query_map(params![user_id], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ids.extend(owned);

    if !dir_ids.is_empty() {
        let placeholders: String = dir_ids.iter().map(|id| format!("'{id}'")).collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id FROM files WHERE directory IN ({placeholders}) AND owner != ?1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let shared: Vec<String> = stmt.query_map(params![user_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.extend(shared);
    }
    Ok(ids)
}

// ── Tests ────────────────────────────────────────────────────────────────────

async fn run_tests(client: &Client, r: &mut Runner) -> Result<()> {
    let conn = Connection::open(DB_PATH).context("open DB for tests")?;

    // ── Feature 1: Signup & Signin ───────────────────────────────────────
    r.section("Feature 1: Signup & Signin");

    let alice = insert_user(&conn, ALICE_EMAIL, ALICE_PASS, ALICE_NAME)?;
    r.pass("create user Alice");
    let bob = insert_user(&conn, BOB_EMAIL, BOB_PASS, BOB_NAME)?;
    r.pass("create user Bob");
    let charlie = insert_user(&conn, CHARLIE_EMAIL, CHARLIE_PASS, CHARLIE_NAME)?;
    r.pass("create user Charlie");

    // Need to close the connection so the server can read WAL
    drop(conn);

    r.check("signin Alice", async {
        let (s, b) = api(client, Method::POST, "/api/collections/users/auth-with-password",
            None, Some(json!({"identity": ALICE_EMAIL, "password": ALICE_PASS}))).await?;
        ensure!((200..300).contains(&s), "signin failed: {s} {b}");
        let token = str_val(&b, "token")?;
        ensure!(!token.is_empty() && token.split('.').count() == 3, "invalid JWT");
        Ok(())
    }.await);

    r.check("signin Bob", async {
        let (s, _) = api(client, Method::POST, "/api/collections/users/auth-with-password",
            None, Some(json!({"identity": BOB_EMAIL, "password": BOB_PASS}))).await?;
        ensure!((200..300).contains(&s), "signin failed: {s}");
        Ok(())
    }.await);

    r.check("signin Charlie", async {
        let (s, _) = api(client, Method::POST, "/api/collections/users/auth-with-password",
            None, Some(json!({"identity": CHARLIE_EMAIL, "password": CHARLIE_PASS}))).await?;
        ensure!((200..300).contains(&s), "signin failed: {s}");
        Ok(())
    }.await);

    r.check("reject wrong password", async {
        let (s, _) = api(client, Method::POST, "/api/collections/users/auth-with-password",
            None, Some(json!({"identity": ALICE_EMAIL, "password": "wrongpassword"}))).await?;
        ensure!(s >= 400, "expected 4xx for wrong password, got {s}");
        Ok(())
    }.await);

    r.check("reject nonexistent user", async {
        let (s, _) = api(client, Method::POST, "/api/collections/users/auth-with-password",
            None, Some(json!({"identity": "nobody@test.com", "password": "whatever123"}))).await?;
        ensure!(s >= 400, "expected 4xx for unknown user, got {s}");
        Ok(())
    }.await);

    // Reopen for the rest of tests
    let conn = Connection::open(DB_PATH).context("reopen DB")?;

    // ── Feature 2: User Names ────────────────────────────────────────────
    r.section("Feature 2: User Names");

    r.check("user has name after creation", {
        let name: String = conn.query_row(
            "SELECT name FROM users WHERE id = ?1", params![alice.id], |row| row.get(0),
        )?;
        ensure!(name == ALICE_NAME, "expected '{ALICE_NAME}', got '{name}'");
        Ok(())
    });

    r.check("update user name", {
        conn.execute("UPDATE users SET name = 'Alice Updated' WHERE id = ?1", params![alice.id])?;
        let name: String = conn.query_row("SELECT name FROM users WHERE id = ?1", params![alice.id], |row| row.get(0))?;
        ensure!(name == "Alice Updated", "name not updated");
        conn.execute("UPDATE users SET name = ?1 WHERE id = ?2", params![ALICE_NAME, alice.id])?;
        Ok(())
    });

    r.check("all users have distinct names", {
        let count: usize = conn.query_row(
            "SELECT COUNT(DISTINCT name) FROM users WHERE id IN (?1, ?2, ?3)",
            params![alice.id, bob.id, charlie.id], |row| row.get(0),
        )?;
        ensure!(count == 3, "expected 3 distinct names, got {count}");
        Ok(())
    });

    // ── Features 3 & 4: File CRUD ────────────────────────────────────────
    r.section("Features 3 & 4: File Upload & CRUD");

    r.check("create file with metadata", {
        let fid = insert_file(&conn, "notes.txt", 1024, "text/plain", &alice.id, None)?;
        let exists = get_record(&conn, "files", &fid)?;
        ensure!(exists, "file not found after insert");
        Ok(())
    });

    let file1_id = insert_file(&conn, "hello.txt", 16, "text/plain", &alice.id, None)?;
    r.pass("create file hello.txt");

    r.check("read file", {
        let (name, size, mime): (String, f64, String) = conn.query_row(
            "SELECT name, size, mime_type FROM files WHERE id = ?1",
            params![file1_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        ensure!(name == "hello.txt", "wrong name: {name}");
        ensure!(size == 16.0, "wrong size: {size}");
        ensure!(mime == "text/plain", "wrong mime: {mime}");
        Ok(())
    });

    r.check("update file name", {
        conn.execute("UPDATE files SET name = 'hello-renamed.txt' WHERE id = ?1", params![file1_id])?;
        let name: String = conn.query_row("SELECT name FROM files WHERE id = ?1", params![file1_id], |row| row.get(0))?;
        ensure!(name == "hello-renamed.txt", "not renamed");
        conn.execute("UPDATE files SET name = 'hello.txt' WHERE id = ?1", params![file1_id])?;
        Ok(())
    });

    r.check("delete file", {
        let tmp_id = insert_file(&conn, "temp.txt", 0, "text/plain", &alice.id, None)?;
        ensure!(get_record(&conn, "files", &tmp_id)?, "file should exist");
        delete_record(&conn, "files", &tmp_id)?;
        ensure!(!get_record(&conn, "files", &tmp_id)?, "file should be gone");
        Ok(())
    });

    r.check("list files by owner", {
        let count = count_records(&conn, "files", &format!("owner = '{}'", alice.id))?;
        ensure!(count >= 2, "expected >= 2 files for Alice, got {count}");
        Ok(())
    });

    // ── Features 5 & 6: Directory CRUD ───────────────────────────────────
    r.section("Features 5 & 6: Directory CRUD");

    let dir1_id = insert_directory(&conn, "My Documents", &alice.id)?;
    r.pass("create directory");

    r.check("read directory", {
        let name: String = conn.query_row(
            "SELECT name FROM directories WHERE id = ?1", params![dir1_id], |row| row.get(0),
        )?;
        ensure!(name == "My Documents", "wrong name: {name}");
        Ok(())
    });

    r.check("update directory name", {
        conn.execute("UPDATE directories SET name = 'My Docs' WHERE id = ?1", params![dir1_id])?;
        let name: String = conn.query_row("SELECT name FROM directories WHERE id = ?1", params![dir1_id], |row| row.get(0))?;
        ensure!(name == "My Docs", "not renamed");
        conn.execute("UPDATE directories SET name = 'My Documents' WHERE id = ?1", params![dir1_id])?;
        Ok(())
    });

    r.check("delete directory", {
        let tmp_id = insert_directory(&conn, "Temp", &alice.id)?;
        ensure!(get_record(&conn, "directories", &tmp_id)?, "dir should exist");
        delete_record(&conn, "directories", &tmp_id)?;
        ensure!(!get_record(&conn, "directories", &tmp_id)?, "dir should be gone");
        Ok(())
    });

    let file_in_dir_id = insert_file(&conn, "report.pdf", 2048, "application/pdf", &alice.id, Some(&dir1_id))?;
    r.pass("create file inside directory");

    r.check("file references its directory", {
        let dir: String = conn.query_row("SELECT directory FROM files WHERE id = ?1", params![file_in_dir_id], |row| row.get(0))?;
        ensure!(dir == dir1_id, "wrong directory ref");
        Ok(())
    });

    let standalone_file_id = insert_file(&conn, "standalone.txt", 100, "text/plain", &alice.id, None)?;
    r.pass("create file without directory");

    r.check("standalone file has no directory", {
        let dir: String = conn.query_row("SELECT directory FROM files WHERE id = ?1", params![standalone_file_id], |row| row.get(0))?;
        ensure!(dir.is_empty(), "expected empty directory, got '{dir}'");
        Ok(())
    });

    let bob_file_id = insert_file(&conn, "bob-private.txt", 50, "text/plain", &bob.id, None)?;

    // ── Feature 7: Sharing ───────────────────────────────────────────────
    r.section("Feature 7: Directory Sharing");

    let share1_id = insert_share(&conn, &dir1_id, &bob.id)?;
    r.pass("Alice shares directory with Bob");

    r.check("share record links directory and user", {
        let (dir, user): (String, String) = conn.query_row(
            "SELECT directory, shared_with FROM shares WHERE id = ?1",
            params![share1_id], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        ensure!(dir == dir1_id, "wrong directory");
        ensure!(user == bob.id, "wrong user");
        Ok(())
    });

    r.check("only one share for the directory", {
        let count = count_records(&conn, "shares", &format!("directory = '{dir1_id}'"))?;
        ensure!(count == 1, "expected 1 share, got {count}");
        Ok(())
    });

    // ── Feature 8: Directory access control ──────────────────────────────
    r.section("Feature 8: Directory Access Control");

    r.check("owner sees own directory", {
        let dirs = visible_dir_ids(&conn, &alice.id)?;
        ensure!(dirs.contains(&dir1_id), "Alice should see her directory");
        Ok(())
    });

    r.check("shared user sees shared directory", {
        let dirs = visible_dir_ids(&conn, &bob.id)?;
        ensure!(dirs.contains(&dir1_id), "Bob should see shared directory");
        Ok(())
    });

    r.check("non-shared user cannot see directory", {
        let dirs = visible_dir_ids(&conn, &charlie.id)?;
        ensure!(!dirs.contains(&dir1_id), "Charlie should NOT see Alice's directory");
        Ok(())
    });

    // ── Feature 9: File access control ───────────────────────────────────
    r.section("Feature 9: File Access Control");

    r.check("owner sees own files", {
        let files = visible_file_ids(&conn, &alice.id)?;
        ensure!(files.contains(&file_in_dir_id), "Alice should see file in her directory");
        ensure!(files.contains(&standalone_file_id), "Alice should see standalone file");
        ensure!(files.contains(&file1_id), "Alice should see hello.txt");
        Ok(())
    });

    r.check("shared user sees file in shared directory", {
        let files = visible_file_ids(&conn, &bob.id)?;
        ensure!(files.contains(&file_in_dir_id), "Bob should see file in shared dir");
        Ok(())
    });

    r.check("shared user does NOT see standalone files of owner", {
        let files = visible_file_ids(&conn, &bob.id)?;
        ensure!(!files.contains(&standalone_file_id), "Bob should NOT see Alice's standalone file");
        Ok(())
    });

    r.check("non-shared user sees nothing of owner", {
        let files = visible_file_ids(&conn, &charlie.id)?;
        ensure!(!files.contains(&file_in_dir_id), "Charlie should NOT see dir file");
        ensure!(!files.contains(&standalone_file_id), "Charlie should NOT see standalone file");
        Ok(())
    });

    r.check("Bob sees own file but not Alice's standalone", {
        let files = visible_file_ids(&conn, &bob.id)?;
        ensure!(files.contains(&bob_file_id), "Bob should see own file");
        ensure!(!files.contains(&standalone_file_id), "Bob should NOT see Alice's standalone");
        Ok(())
    });

    // ── Revocation ───────────────────────────────────────────────────────
    r.section("Feature 7+8+9: Access Revocation");

    r.check("revoke share hides directory from Bob", {
        delete_record(&conn, "shares", &share1_id)?;
        let dirs = visible_dir_ids(&conn, &bob.id)?;
        ensure!(!dirs.contains(&dir1_id), "Bob should NOT see dir after unshare");
        Ok(())
    });

    r.check("revoke share hides files in directory from Bob", {
        let files = visible_file_ids(&conn, &bob.id)?;
        ensure!(!files.contains(&file_in_dir_id), "Bob should NOT see file after unshare");
        Ok(())
    });

    let _share2_id = insert_share(&conn, &dir1_id, &bob.id)?;

    r.check("re-share restores access", {
        let dirs = visible_dir_ids(&conn, &bob.id)?;
        ensure!(dirs.contains(&dir1_id), "Bob should see dir after re-share");
        let files = visible_file_ids(&conn, &bob.id)?;
        ensure!(files.contains(&file_in_dir_id), "Bob should see file after re-share");
        Ok(())
    });

    r.section("Note: Passkeys");
    println!("  INFO  Passkey routes are not wired in the binary build.");
    println!("        Email/password auth is fully tested above.");

    Ok(())
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("\n=== Zerobase E2E File Share Test ===\n");

    let client = Client::builder().timeout(Duration::from_secs(30)).build()
        .expect("failed to build HTTP client");

    println!("--- Setup ---");

    if !Path::new(BINARY).exists() {
        eprintln!("FATAL: Zerobase binary not found at '{BINARY}'.");
        std::process::exit(1);
    }

    if let Err(e) = clean_data() { eprintln!("FATAL: {e:#}"); std::process::exit(1); }
    println!("  Data cleaned");

    if let Err(e) = run_superuser_create() { eprintln!("FATAL: {e:#}"); std::process::exit(1); }
    println!("  Superuser created");

    let mut server = match start_server() {
        Ok(s) => s,
        Err(e) => { eprintln!("FATAL: {e:#}"); std::process::exit(1); }
    };
    println!("  Server started (pid {})", server.id());

    if let Err(e) = wait_for_ready(&client).await {
        eprintln!("FATAL: {e:#}"); stop_server(&mut server); std::process::exit(1);
    }
    println!("  Server ready");

    let admin_token = match admin_auth(&client).await {
        Ok(t) => t,
        Err(e) => { eprintln!("FATAL: admin auth failed: {e:#}"); stop_server(&mut server); std::process::exit(1); }
    };
    println!("  Admin authenticated");

    if let Err(e) = setup_collections(&client, &admin_token).await {
        eprintln!("FATAL: collection setup failed: {e:#}"); stop_server(&mut server); std::process::exit(1);
    }

    // Server stays running; we open a separate read/write SQLite connection.
    let mut runner = Runner::new();
    if let Err(e) = run_tests(&client, &mut runner).await {
        eprintln!("\nFATAL test error: {e:#}");
    }

    runner.report();

    stop_server(&mut server);
    println!("Server stopped.\n");

    if runner.failed > 0 { std::process::exit(1); }
}
