//! Reading and writing labels, and the label vocabulary itself.

use sqlx::SqlitePool;

use crate::error::AppResult;

use super::clips::Label;

/// Lower-kebab-case validator. Allows any Unicode lowercase letter (so
/// å, ä, é, ü, ñ all work), ASCII digits, and hyphens. No leading,
/// trailing or consecutive hyphens. The caller is expected to have
/// lowercased the input already.
pub fn is_valid(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut prev_was_dash = false;
    let mut first = true;
    let mut last_char = '\0';
    for c in s.chars() {
        let is_letter = c.is_alphabetic() && c.is_lowercase();
        let is_digit = c.is_ascii_digit();
        let is_dash = c == '-';
        if !(is_letter || is_digit || is_dash) {
            return false;
        }
        if is_dash {
            if first || prev_was_dash {
                return false;
            }
            prev_was_dash = true;
        } else {
            prev_was_dash = false;
        }
        first = false;
        last_char = c;
    }
    last_char != '-'
}

/// Resolve a label id to its canonical name, or `None` if there is no
/// such label.
pub async fn name_of(pool: &SqlitePool, label_id: i64) -> AppResult<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT name FROM labels WHERE id = ?")
        .bind(label_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(n,)| n))
}

/// Resolve a label name to its id and canonical spelling, matching
/// case-insensitively. `None` when the label has never been used.
pub async fn find_by_name(pool: &SqlitePool, name: &str) -> AppResult<Option<Label>> {
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT id, name FROM labels WHERE name = ? COLLATE NOCASE")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id, name)| Label { id, name }))
}

/// Put a label on a clip, creating the label if this is its first use.
/// Both steps are INSERT OR IGNORE inside one transaction, so adding a
/// label that is already there is a no-op rather than an error.
pub async fn add_to_clip(
    pool: &SqlitePool,
    clip_id: i64,
    normalised_name: &str,
    added_by: i64,
) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO labels (name) VALUES (?)")
        .bind(normalised_name)
        .execute(&mut *tx)
        .await?;
    let label_id: (i64,) = sqlx::query_as("SELECT id FROM labels WHERE name = ?")
        .bind(normalised_name)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO clip_labels (clip_id, label_id, added_by) VALUES (?, ?, ?)")
        .bind(clip_id)
        .bind(label_id.0)
        .bind(added_by)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Take a label off a clip. The label row itself stays, so its wiki page
/// and history survive the last clip losing it.
pub async fn remove_from_clip(pool: &SqlitePool, clip_id: i64, label_id: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM clip_labels WHERE clip_id = ? AND label_id = ?")
        .bind(clip_id)
        .bind(label_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// What the label picker offers for a query.
pub struct Suggestions {
    /// The normalised query the suggestions were computed for. The
    /// caller uses it as the name to create when `can_create` is set.
    pub query: String,
    pub matches: Vec<String>,
    pub can_create: bool,
}

/// Labels matching `raw_query`, for the picker.
///
/// An empty query means "what would I most likely want?", answered with
/// the most recently used labels rather than nothing, so focusing the
/// field already offers something to tap. `exclude_clip` drops the
/// labels a clip already carries.
pub async fn suggest(
    pool: &SqlitePool,
    raw_query: &str,
    exclude_clip: Option<i64>,
) -> AppResult<Suggestions> {
    let trimmed = raw_query.trim();
    if trimmed.is_empty() {
        return Ok(Suggestions {
            query: String::new(),
            matches: recent(pool, exclude_clip).await?,
            can_create: false,
        });
    }

    let normalised = trimmed.to_lowercase();
    // Escape the LIKE metacharacters: backslash first, so we don't
    // double-escape our own escapes, then % and _. The query goes out
    // with ESCAPE '\' so SQLite reads the prefixed characters as
    // literal.
    let pattern = format!(
        "%{}%",
        normalised
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );

    let matches: Vec<(String,)> = if let Some(clip_id) = exclude_clip {
        sqlx::query_as(
            "SELECT name FROM labels WHERE name LIKE ? ESCAPE '\\'
             AND id NOT IN (SELECT label_id FROM clip_labels WHERE clip_id = ?)
             ORDER BY name COLLATE NOCASE LIMIT 10",
        )
        .bind(&pattern)
        .bind(clip_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT name FROM labels WHERE name LIKE ? ESCAPE '\\'
             ORDER BY name COLLATE NOCASE LIMIT 10",
        )
        .bind(&pattern)
        .fetch_all(pool)
        .await?
    };
    let names: Vec<String> = matches.into_iter().map(|(n,)| n).collect();

    // Offer to create only when the name is a valid label and isn't
    // already sitting in the results.
    let exact = names.iter().any(|n| n == &normalised);
    let can_create = !exact && is_valid(&normalised);

    Ok(Suggestions {
        query: normalised,
        matches: names,
        can_create,
    })
}

/// The ten most recently used labels across all clips, optionally
/// excluding those already on a clip.
async fn recent(pool: &SqlitePool, exclude_clip: Option<i64>) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = if let Some(clip_id) = exclude_clip {
        sqlx::query_as(
            "SELECT l.name FROM labels l
             LEFT JOIN clip_labels ct ON ct.label_id = l.id
             WHERE l.id NOT IN (SELECT label_id FROM clip_labels WHERE clip_id = ?)
             GROUP BY l.id
             ORDER BY COALESCE(MAX(ct.added_at), '') DESC, l.name COLLATE NOCASE
             LIMIT 10",
        )
        .bind(clip_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT l.name FROM labels l
             LEFT JOIN clip_labels ct ON ct.label_id = l.id
             GROUP BY l.id
             ORDER BY COALESCE(MAX(ct.added_at), '') DESC, l.name COLLATE NOCASE
             LIMIT 10",
        )
        .fetch_all(pool)
        .await?
    };
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

#[cfg(test)]
mod tests {
    use super::is_valid;

    #[test]
    fn accepts_kebab_case_with_diacritics() {
        for s in [
            "verse",
            "verse-1",
            "verse-one",
            "pålägg",
            "café-version",
            "über-mix",
            "x",
        ] {
            assert!(is_valid(s), "expected valid: {s}");
        }
    }

    #[test]
    fn rejects_bad_shapes() {
        for s in [
            "", "-verse", "verse-", "verse--1", "Verse", "verse 1", "verse_1", "verse.1",
        ] {
            assert!(!is_valid(s), "expected invalid: {s}");
        }
    }
}
