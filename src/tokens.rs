//! Bearer tokens, one per app install.
//!
//! A token is 32 bytes of randomness, rendered in the URL-safe base64
//! alphabet so it can sit in an `Authorization` header untouched. The
//! plaintext is shown once, when it is issued; the database keeps only
//! its SHA-256, so every request authenticates by hashing what it was
//! given and looking that up.
//!
//! There is no expiry. An app that silently signs itself out every few
//! weeks is worse than useless, and the thing a user actually wants when
//! a phone is lost is a button that kills that one device — which is
//! `revoke`.

use rand::Rng;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{error::AppResult, models::SessionUser};

/// A freshly issued token. `secret` is the only time the plaintext
/// exists outside the client.
pub struct Issued {
    pub id: i64,
    pub secret: String,
}

/// One row of the account screen's device list.
#[derive(Debug)]
pub struct Device {
    pub id: i64,
    pub device_name: String,
    pub created_at: String,
    pub last_used_on: Option<String>,
    /// Whether this is the token the current request arrived with, so a
    /// client can label it "this device".
    pub is_current: bool,
}

/// 32 bytes of randomness in the URL-safe base64 alphabet. 256 bits, so
/// guessing is not a threat model and the stored hash needs no salt.
fn generate() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut rng = rand::thread_rng();
    (0..43)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

/// What gets stored and looked up: lowercase hex of the SHA-256.
fn fingerprint(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Issue a token for a user. The caller has already checked the password.
pub async fn issue(pool: &SqlitePool, user_id: i64, device_name: &str) -> AppResult<Issued> {
    let device_name = device_name.trim();
    let device_name = if device_name.is_empty() {
        "Unnamed device"
    } else {
        device_name
    };
    let secret = generate();
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO api_tokens (user_id, token_hash, device_name) VALUES (?, ?, ?)
         RETURNING id",
    )
    .bind(user_id)
    .bind(fingerprint(&secret))
    .bind(device_name)
    .fetch_one(pool)
    .await?;
    Ok(Issued { id: row.0, secret })
}

/// Resolve a bearer token to its user, or `None` if it isn't one of
/// ours. Also stamps today's date on the token, at most once a day —
/// enough for the device list to be useful without a write per request.
pub async fn authenticate(
    pool: &SqlitePool,
    secret: &str,
) -> AppResult<Option<(SessionUser, i64)>> {
    let row: Option<(i64, i64, String, i64, Option<String>)> = sqlx::query_as(
        "SELECT t.id, u.id, u.username, u.is_admin, t.last_used_on
         FROM api_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = ?",
    )
    .bind(fingerprint(secret))
    .fetch_optional(pool)
    .await?;

    let Some((token_id, user_id, username, is_admin, last_used_on)) = row else {
        return Ok(None);
    };

    let today = crate::datefmt::today_utc();
    if last_used_on.as_deref() != Some(today.as_str()) {
        sqlx::query("UPDATE api_tokens SET last_used_on = ? WHERE id = ?")
            .bind(&today)
            .bind(token_id)
            .execute(pool)
            .await?;
    }

    Ok(Some((
        SessionUser {
            id: user_id,
            username,
            is_admin: is_admin != 0,
        },
        token_id,
    )))
}

/// A user's tokens, newest first.
pub async fn list(pool: &SqlitePool, user_id: i64, current: Option<i64>) -> AppResult<Vec<Device>> {
    let rows: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, device_name, created_at, last_used_on
         FROM api_tokens WHERE user_id = ? ORDER BY id DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, device_name, created_at, last_used_on)| Device {
            id,
            device_name,
            created_at,
            last_used_on,
            is_current: current == Some(id),
        })
        .collect())
}

/// Revoke one of a user's tokens. Scoped to `user_id` so a token id
/// belonging to someone else is a miss, not someone else's sign-out.
/// Returns whether anything was revoked.
pub async fn revoke(pool: &SqlitePool, user_id: i64, token_id: i64) -> AppResult<bool> {
    let result = sqlx::query("DELETE FROM api_tokens WHERE id = ? AND user_id = ?")
        .bind(token_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Revoke every token a user has. Used when they change their password:
/// the reason to change it is usually that someone else might know it.
pub async fn revoke_all(pool: &SqlitePool, user_id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM api_tokens WHERE user_id = ?")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_url_safe_and_distinct() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn fingerprint_is_stable_hex() {
        // The SHA-256 of the empty string, as a check that we hash what
        // we say we hash and render it the way the column expects.
        assert_eq!(
            fingerprint(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(fingerprint("x").len(), 64);
        assert_ne!(fingerprint("x"), fingerprint("y"));
    }
}
