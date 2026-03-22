//! Password hashing and verification using Argon2id.
//!
//! Uses the `argon2` crate with parameters explicitly set per OWASP recommendations:
//! - Algorithm: Argon2id (resistant to both side-channel and GPU attacks)
//! - Memory cost: 19,456 KiB (19 MiB)
//! - Iterations (time cost): 2
//! - Parallelism: 1
//!
//! These parameters match the OWASP minimum recommendation for Argon2id.
//! They are hardcoded rather than relying on crate defaults to prevent
//! silent weakening if upstream defaults ever change.
//!
//! Passwords are hashed into PHC string format (e.g., `$argon2id$v=19$m=19456,...`).

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

/// OWASP-recommended Argon2id parameters.
///
/// Reference: <https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html>
/// - `m = 19456` — 19 MiB memory cost
/// - `t = 2`     — 2 iterations
/// - `p = 1`     — single-threaded
const MEMORY_COST_KIB: u32 = 19_456;
const TIME_COST: u32 = 2;
const PARALLELISM: u32 = 1;

/// Build the Argon2id hasher with explicit OWASP-recommended parameters.
fn build_argon2() -> Argon2<'static> {
    let params = Params::new(MEMORY_COST_KIB, TIME_COST, PARALLELISM, None)
        .expect("OWASP Argon2id params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a plaintext password using Argon2id with OWASP-recommended parameters.
///
/// Returns a PHC-format string that includes the algorithm, parameters,
/// salt, and hash — everything needed for later verification.
///
/// # Errors
///
/// Returns an error if hashing fails (e.g., OsRng unavailable).
pub fn hash_password(plain: &str) -> Result<String, PasswordHashError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = build_argon2();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| PasswordHashError::HashFailed(e.to_string()))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored PHC-format hash.
///
/// The stored hash's embedded parameters are used for verification,
/// so hashes created with different parameters will still verify
/// correctly. This enables safe parameter migration over time.
///
/// Returns `true` if the password matches the hash, `false` otherwise.
///
/// # Errors
///
/// Returns an error if the stored hash is malformed or uses an
/// unsupported algorithm.
pub fn verify_password(plain: &str, hash: &str) -> Result<bool, PasswordHashError> {
    let parsed =
        PasswordHash::new(hash).map_err(|e| PasswordHashError::InvalidHash(e.to_string()))?;
    // Use default Argon2 for verification — parameters are read from the
    // PHC string itself, so verification works regardless of the Argon2
    // instance's configured parameters.
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

/// Errors from password hashing/verification operations.
#[derive(Debug, thiserror::Error)]
pub enum PasswordHashError {
    #[error("failed to hash password: {0}")]
    HashFailed(String),
    #[error("invalid password hash format: {0}")]
    InvalidHash(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_produces_argon2id_phc_string() {
        let hash = hash_password("mypassword123").unwrap();
        assert!(
            hash.starts_with("$argon2id$"),
            "hash should use argon2id: {hash}"
        );
    }

    #[test]
    fn hash_embeds_owasp_parameters() {
        let hash = hash_password("test").unwrap();
        // PHC format: $argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>
        assert!(
            hash.contains("m=19456"),
            "memory cost should be 19456 KiB: {hash}"
        );
        assert!(hash.contains("t=2"), "time cost should be 2: {hash}");
        assert!(hash.contains("p=1"), "parallelism should be 1: {hash}");
    }

    #[test]
    fn hash_is_unique_per_call() {
        let h1 = hash_password("same_password").unwrap();
        let h2 = hash_password("same_password").unwrap();
        assert_ne!(h1, h2, "each hash should use a unique salt");
    }

    #[test]
    fn verify_correct_password() {
        let hash = hash_password("correct_password").unwrap();
        assert!(verify_password("correct_password", &hash).unwrap());
    }

    #[test]
    fn verify_wrong_password() {
        let hash = hash_password("correct_password").unwrap();
        assert!(!verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn verify_empty_password() {
        let hash = hash_password("").unwrap();
        assert!(verify_password("", &hash).unwrap());
        assert!(!verify_password("notempty", &hash).unwrap());
    }

    #[test]
    fn verify_rejects_malformed_hash() {
        let result = verify_password("password", "not-a-valid-hash");
        assert!(result.is_err());
    }

    #[test]
    fn verify_rejects_empty_hash() {
        let result = verify_password("password", "");
        assert!(result.is_err());
    }

    #[test]
    fn hash_handles_unicode() {
        let hash = hash_password("пароль密码パスワード").unwrap();
        assert!(verify_password("пароль密码パスワード", &hash).unwrap());
        assert!(!verify_password("password", &hash).unwrap());
    }

    #[test]
    fn hash_handles_long_password() {
        let long_pwd = "a".repeat(1000);
        let hash = hash_password(&long_pwd).unwrap();
        assert!(verify_password(&long_pwd, &hash).unwrap());
    }

    #[test]
    fn hash_handles_special_characters() {
        let special = "p@$$w0rd!#%^&*()_+-=[]{}|;':\",./<>?`~";
        let hash = hash_password(special).unwrap();
        assert!(verify_password(special, &hash).unwrap());
    }

    #[test]
    fn hash_handles_null_bytes() {
        let with_null = "pass\0word";
        let hash = hash_password(with_null).unwrap();
        assert!(verify_password(with_null, &hash).unwrap());
        assert!(!verify_password("pass", &hash).unwrap());
    }

    /// Verify that verification timing is roughly consistent regardless
    /// of whether the password is correct or incorrect. This helps ensure
    /// that the underlying implementation does not short-circuit in a way
    /// that leaks match/no-match information through timing.
    #[test]
    fn verify_timing_is_consistent() {
        let hash = hash_password("benchmark_password").unwrap();
        let iterations = 5;

        // Time correct password verifications
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = verify_password("benchmark_password", &hash);
        }
        let correct_duration = start.elapsed();

        // Time incorrect password verifications
        let start = std::time::Instant::now();
        for _ in 0..iterations {
            let _ = verify_password("wrong_password_value", &hash);
        }
        let incorrect_duration = start.elapsed();

        // Both should take roughly the same time. We allow a generous
        // 5x ratio to avoid flaky failures on loaded CI machines while
        // still catching egregious short-circuit behaviour.
        let ratio = correct_duration.as_nanos() as f64 / incorrect_duration.as_nanos() as f64;
        assert!(
            (0.2..=5.0).contains(&ratio),
            "timing ratio {ratio:.2} outside [0.2, 5.0] — \
             correct={correct_duration:?}, incorrect={incorrect_duration:?}"
        );
    }

    #[test]
    fn build_argon2_uses_argon2id() {
        // Verify the builder produces Argon2id (not Argon2i or Argon2d)
        let argon2 = build_argon2();
        let salt = SaltString::generate(&mut OsRng);
        let hash = argon2.hash_password(b"test", &salt).unwrap().to_string();
        assert!(hash.starts_with("$argon2id$"));
    }
}
