//! Build script for the zerobase binary.
//!
//! Embeds compile-time metadata (git commit, build target, timestamp)
//! into the binary so `zerobase version` can report useful diagnostics.

use std::process::Command;

fn main() {
    // Git commit hash (short)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Whether the working tree is dirty
    let git_dirty = Command::new("git")
        .args(["diff", "--quiet", "HEAD"])
        .status()
        .map(|s| if s.success() { "" } else { "-dirty" })
        .unwrap_or("");

    let git_describe = format!("{git_hash}{git_dirty}");

    // Build timestamp (UTC)
    let build_date = chrono_free_utc_date();

    // Target triple (set by Cargo)
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());

    // Rust compiler version
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Build profile
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=ZEROBASE_GIT_HASH={git_describe}");
    println!("cargo:rustc-env=ZEROBASE_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=ZEROBASE_BUILD_TARGET={target}");
    println!("cargo:rustc-env=ZEROBASE_RUSTC_VERSION={rustc_version}");
    println!("cargo:rustc-env=ZEROBASE_BUILD_PROFILE={profile}");

    // Only re-run when git HEAD changes or this script changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}

/// Returns the current UTC date as YYYY-MM-DD without pulling in chrono.
fn chrono_free_utc_date() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
