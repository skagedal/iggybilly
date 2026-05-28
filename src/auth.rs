use std::{sync::OnceLock, time::Duration};

use anyhow::{Result, anyhow};
use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};
use rand::Rng;

pub fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow!("argon2 hash: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(plain: &str, stored_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

/// A real argon2 hash of a throwaway value, lazily computed once per
/// process. Login uses this when the username isn't found so the
/// response time matches the wrong-password path — otherwise an
/// attacker could enumerate accounts by timing.
pub fn dummy_hash() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| hash_password("dummy-never-matches").expect("init dummy hash"))
}

/// Cheap brute-force damper: every failed login pays this. With argon2
/// already in the verify path the wall-clock floor is well above what
/// an attacker would tolerate at scale; this just adds a bit more cost
/// and is much simpler than a per-IP limiter.
pub async fn failed_login_penalty() {
    tokio::time::sleep(Duration::from_millis(500)).await;
}

/// 16 chars from an unambiguous alphabet. ~85 bits of entropy — plenty
/// for an admin-issued password the user will rotate on first login.
pub fn random_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..16)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}
