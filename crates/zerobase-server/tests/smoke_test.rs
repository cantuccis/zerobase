//! Smoke test — verifies the zerobase binary starts and serves the dashboard.
//!
//! This integration test:
//! 1. Builds and starts the binary on an ephemeral port
//! 2. Checks the version subcommand outputs build metadata
//! 3. Verifies `/_/` serves the embedded admin dashboard (HTML)
//! 4. Verifies `/_/_astro/*.js` serves JavaScript with correct MIME type
//! 5. Shuts down the server cleanly

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Find a free TCP port by binding to :0 and reading back the assigned port.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to ephemeral port");
    listener.local_addr().unwrap().port()
}

/// Create a temporary data directory and config file for the test.
struct TestEnv {
    _tmp: tempfile::TempDir,
    config_path: PathBuf,
    data_dir: PathBuf,
    port: u16,
}

impl TestEnv {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).expect("create data dir");

        let port = free_port();

        // Write a minimal config file
        let config_content = format!(
            r#"
[server]
host = "127.0.0.1"
port = {port}
log_format = "pretty"

[database]
path = "{db_path}"

[auth]
token_secret = "test-secret-key-that-is-long-enough-for-hmac-validation-purposes-1234"
token_duration_secs = 3600

[storage]
backend = "local"
local_path = "{storage_path}"

[smtp]
enabled = false

[logs]
retention_days = 1
"#,
            db_path = data_dir.join("data.db").display(),
            storage_path = data_dir.join("storage").display(),
        );

        let config_path = tmp.path().join("zerobase.toml");
        std::fs::write(&config_path, config_content).expect("write config");

        Self {
            _tmp: tmp,
            config_path,
            data_dir,
            port,
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// Start the zerobase binary and return the child process.
/// Waits until the server is accepting TCP connections.
fn start_server(env: &TestEnv) -> Child {
    let binary = find_binary();

    let mut child = Command::new(&binary)
        .args(["serve"])
        .env("ZEROBASE_CONFIG", &env.config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to start zerobase binary at {}: {e}", binary.display()));

    // Drain stderr in a background thread to prevent pipe buffer deadlock
    let stderr = child.stderr.take().expect("stderr");
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                eprintln!("[server:err] {line}");
            }
        }
    });

    // Drain stdout too
    let stdout = child.stdout.take().expect("stdout");
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(line) = line {
                eprintln!("[server:out] {line}");
            }
        }
    });

    // Poll TCP until the server is accepting connections (max 30s)
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("server did not start accepting connections within 30 seconds on port {}", env.port);
        }

        match std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", env.port).parse().unwrap(),
            Duration::from_millis(200),
        ) {
            Ok(_) => break,
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }

        // Check if process has exited early
        if let Ok(Some(status)) = child.try_wait() {
            panic!("server process exited early with status: {status}");
        }
    }

    // Small grace period after first connection accepted
    std::thread::sleep(Duration::from_millis(100));

    child
}

/// Find the compiled binary path.
fn find_binary() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();

    // Prefer release, fall back to debug
    for profile in ["release", "debug"] {
        let bin = workspace_root.join("target").join(profile).join("zerobase");
        if bin.exists() {
            return bin;
        }
    }

    panic!(
        "zerobase binary not found. Run `cargo build --release --package zerobase-server` first."
    );
}

/// Gracefully stop the server process.
fn stop_server(mut child: Child) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                break;
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn binary_starts_and_serves_dashboard() {
    let env = TestEnv::new();
    let child = start_server(&env);
    let base = env.base_url();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // 1. Dashboard root (/_/) returns HTML
    let resp = client
        .get(format!("{base}/_/"))
        .send()
        .expect("GET /_/ should succeed");

    assert_eq!(resp.status().as_u16(), 200, "/_/ should return 200 OK");
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/html"),
        "/_/ should serve text/html, got: {content_type}"
    );
    let body = resp.text().unwrap();
    assert!(
        body.contains("<!DOCTYPE html>") || body.contains("<!doctype html>") || body.contains("<html"),
        "/_/ should return an HTML document"
    );

    // 2. Dashboard login page returns HTML
    let resp = client
        .get(format!("{base}/_/login"))
        .send()
        .expect("GET /_/login should succeed");
    assert_eq!(resp.status().as_u16(), 200, "/_/login should return 200");
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/html"), "/_/login should serve text/html");

    // 3. Static JS assets are served with correct MIME type
    let root_html = client
        .get(format!("{base}/_/"))
        .send()
        .unwrap()
        .text()
        .unwrap();

    if let Some(js_ref) = extract_js_reference(&root_html) {
        let resp = client
            .get(format!("{base}/_/{js_ref}"))
            .send()
            .expect("GET JS bundle should succeed");
        assert_eq!(resp.status().as_u16(), 200, "JS bundle should return 200");
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ct.contains("javascript"),
            "JS bundle should have javascript content-type, got: {ct}"
        );

        let cache = resp
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            cache.contains("immutable"),
            "hashed _astro assets should have immutable cache-control, got: {cache}"
        );
    }

    // 4. CSS assets are served
    if let Some(css_ref) = extract_css_reference(&root_html) {
        let resp = client
            .get(format!("{base}/_/{css_ref}"))
            .send()
            .expect("GET CSS should succeed");
        assert_eq!(resp.status().as_u16(), 200, "CSS should return 200");
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("css"), "CSS should have css content-type, got: {ct}");
    }

    // 5. Favicon is served
    let resp = client
        .get(format!("{base}/_/favicon.svg"))
        .send()
        .expect("GET favicon should succeed");
    assert_eq!(resp.status().as_u16(), 200, "favicon should return 200");

    // 6. Non-existent path under /_/ falls back to index.html (SPA)
    let resp = client
        .get(format!("{base}/_/some/nonexistent/path"))
        .send()
        .expect("GET SPA fallback should succeed");
    assert_eq!(resp.status().as_u16(), 200, "SPA fallback should return 200");
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/html"), "SPA fallback should serve text/html");

    // Clean shutdown
    stop_server(child);
}

#[test]
fn binary_version_subcommand_works() {
    let binary = find_binary();

    let output = Command::new(&binary)
        .args(["version"])
        .output()
        .expect("should run version subcommand");

    assert!(output.status.success(), "version subcommand should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("zerobase v"),
        "version output should contain 'zerobase v', got: {stdout}"
    );
    assert!(
        stdout.contains("git commit:"),
        "version output should contain build metadata"
    );
    assert!(
        stdout.contains("target:"),
        "version output should contain target info"
    );
}

#[test]
fn binary_size_is_reasonable() {
    let binary = find_binary();
    let metadata = std::fs::metadata(&binary).expect("read binary metadata");
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);

    // The binary with embedded dashboard should be under 200MB
    assert!(
        size_mb < 200.0,
        "binary size ({size_mb:.1} MB) should be under 200 MB"
    );

    // It should be at least 1MB (sanity check that something is compiled)
    assert!(
        size_mb > 1.0,
        "binary size ({size_mb:.1} MB) suspiciously small, expected > 1 MB"
    );

    eprintln!("Binary size: {size_mb:.1} MB at {}", binary.display());
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Extract a JS bundle reference from HTML (e.g., `_astro/Foo.ABC.js`).
fn extract_js_reference(html: &str) -> Option<String> {
    for segment in html.split('"') {
        let trimmed = segment.trim_start_matches('/');
        if trimmed.starts_with("_astro/") && trimmed.ends_with(".js") {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Extract a CSS reference from HTML (e.g., `_astro/global.XYZ.css`).
fn extract_css_reference(html: &str) -> Option<String> {
    for segment in html.split('"') {
        let trimmed = segment.trim_start_matches('/');
        if trimmed.starts_with("_astro/") && trimmed.ends_with(".css") {
            return Some(trimmed.to_string());
        }
    }
    None
}
