//! Reading bands and membership.
//!
//! A band is the unit everything else belongs to, so these are the
//! queries every other read is scoped by. The important one is
//! [`resolve_for_member`]: it answers "which band is this URL about, and
//! may this person see it?" in a single statement, and answers it the
//! same way for a band that does not exist and a band the caller is not
//! in. Those two have to be indistinguishable, or trying slugs becomes a
//! way to learn who else is on the instance.

use sqlx::SqlitePool;

use crate::{error::AppResult, models::Band};

/// The band this slug names, if the user is a member of it.
///
/// `None` covers both "no such band" and "not your band", deliberately
/// from the one query so the two cannot be told apart by timing either.
pub async fn resolve_for_member(
    pool: &SqlitePool,
    slug: &str,
    user_id: i64,
) -> AppResult<Option<Band>> {
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT b.id, b.slug, b.discord_webhook_url
         FROM bands b
         JOIN band_members m ON m.band_id = b.id AND m.user_id = ?
         WHERE b.slug = ? COLLATE NOCASE",
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(to_band))
}

/// Every band the user is in, by slug.
pub async fn for_user(pool: &SqlitePool, user_id: i64) -> AppResult<Vec<Band>> {
    let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT b.id, b.slug, b.discord_webhook_url
         FROM bands b
         JOIN band_members m ON m.band_id = b.id
         WHERE m.user_id = ?
         ORDER BY b.slug COLLATE NOCASE",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(to_band).collect())
}

/// Where to send a user who hasn't said where they want to go: the band
/// they were last in, or — if that is unset, or they have since been
/// removed from it — the first of their bands, or `None` when they are
/// in none.
///
/// The membership join is what makes the stale case safe. A last-band id
/// outlives a removal from that band, and following it blindly would
/// land someone in a band they can no longer see.
pub async fn landing_band(pool: &SqlitePool, user_id: i64) -> AppResult<Option<Band>> {
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT b.id, b.slug, b.discord_webhook_url
         FROM bands b
         JOIN band_members m ON m.band_id = b.id AND m.user_id = ?
         LEFT JOIN users u ON u.id = m.user_id AND u.last_band_id = b.id
         ORDER BY u.id IS NULL, b.slug COLLATE NOCASE
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(to_band))
}

/// Remember which band a user is looking at.
///
/// Called when someone arrives in a band, not on every read: it is a
/// preference, and writing it on each request would be a write per page
/// view for a value almost always already correct.
pub async fn set_last_band(pool: &SqlitePool, user_id: i64, band_id: i64) -> AppResult<()> {
    sqlx::query("UPDATE users SET last_band_id = ? WHERE id = ? AND last_band_id IS NOT ?")
        .bind(band_id)
        .bind(user_id)
        .bind(band_id)
        .execute(pool)
        .await?;
    Ok(())
}

fn to_band((id, slug, discord_webhook_url): (i64, String, Option<String>)) -> Band {
    Band {
        id,
        slug,
        discord_webhook_url,
    }
}

/// A band slug is validated by exactly the rule a label name is, and by
/// the same code: both are lower-kebab-case names people type and read,
/// and two rules that are meant to be the same drift apart the moment
/// they are written twice.
pub use super::labels::is_valid as is_valid_slug;

/// The slugs a band may not take, because the server already answers on
/// them as the first path segment. A band called `login` would have a
/// clip list nobody could reach.
///
/// It lives here next to the check rather than in the router so that
/// `create-band` and the routes cannot disagree about it — but the list
/// is only correct as long as it is extended with any new static first
/// segment.
pub const RESERVED_SLUGS: &[&str] = &["api", "static", "login", "account", "healthz"];

/// A band by slug, whoever is asking. For the command line, which acts
/// as the site owner and is not a member of anything.
pub async fn find_by_slug(pool: &SqlitePool, slug: &str) -> AppResult<Option<Band>> {
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, slug, discord_webhook_url FROM bands WHERE slug = ? COLLATE NOCASE",
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(to_band))
}

/// Create a band. The caller has already validated the slug.
pub async fn create(pool: &SqlitePool, slug: &str) -> AppResult<i64> {
    let row: (i64,) = sqlx::query_as("INSERT INTO bands (slug) VALUES (?) RETURNING id")
        .bind(slug)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Rename a band, which changes every URL its content is reachable at.
pub async fn rename(pool: &SqlitePool, band_id: i64, new_slug: &str) -> AppResult<()> {
    sqlx::query("UPDATE bands SET slug = ? WHERE id = ?")
        .bind(new_slug)
        .bind(band_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Point a band's Discord posts at its own channel, or (with `None`)
/// back at the instance-wide webhook.
pub async fn set_webhook(pool: &SqlitePool, band_id: i64, url: Option<&str>) -> AppResult<()> {
    sqlx::query("UPDATE bands SET discord_webhook_url = ? WHERE id = ?")
        .bind(url)
        .bind(band_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Put a user in a band. Already being in it is a no-op, not an error.
pub async fn add_member(pool: &SqlitePool, band_id: i64, user_id: i64) -> AppResult<()> {
    sqlx::query("INSERT OR IGNORE INTO band_members (band_id, user_id) VALUES (?, ?)")
        .bind(band_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Take a user out of a band, and clear their last-band if it was this
/// one, so they are not sent somewhere they can no longer see. Their
/// clips stay where they are: a clip belongs to the band, not to the
/// person who uploaded it.
pub async fn remove_member(pool: &SqlitePool, band_id: i64, user_id: i64) -> AppResult<bool> {
    let mut tx = pool.begin().await?;
    let removed = sqlx::query("DELETE FROM band_members WHERE band_id = ? AND user_id = ?")
        .bind(band_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        > 0;
    sqlx::query("UPDATE users SET last_band_id = NULL WHERE id = ? AND last_band_id = ?")
        .bind(user_id)
        .bind(band_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(removed)
}

/// Every band with how many members and clips it has, for `list-bands`.
pub async fn list_with_counts(pool: &SqlitePool) -> AppResult<Vec<(Band, i64, i64)>> {
    let rows: Vec<(i64, String, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT b.id, b.slug, b.discord_webhook_url,
                (SELECT COUNT(*) FROM band_members m WHERE m.band_id = b.id),
                (SELECT COUNT(*) FROM clips c WHERE c.band_id = b.id)
         FROM bands b ORDER BY b.slug COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, slug, webhook, members, clips)| (to_band((id, slug, webhook)), members, clips))
        .collect())
}

/// The slugs each named user is in, for `list-users`.
pub async fn slugs_for_user(pool: &SqlitePool, user_id: i64) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT b.slug FROM bands b
         JOIN band_members m ON m.band_id = b.id
         WHERE m.user_id = ? ORDER BY b.slug COLLATE NOCASE",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(s,)| s).collect())
}
