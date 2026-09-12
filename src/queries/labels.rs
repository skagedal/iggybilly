//! Reading and writing labels, and the label vocabulary itself.

use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};

use super::clips::Label;

/// The gap left between neighbouring positions in a playlist, so a clip
/// dropped between two rows takes their midpoint and only its own row is
/// written. See `migrations/0005_playlist_order.sql`.
const GAP: i64 = 1024;

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
///
/// The clip lands at the end of the label's playlist: one gap past the
/// last position, or at 0 when it is the label's first clip.
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
    sqlx::query(
        "INSERT OR IGNORE INTO clip_labels (clip_id, label_id, added_by, position)
         VALUES (?, ?, ?,
                 (SELECT COALESCE(MAX(position), ?) + ? FROM clip_labels WHERE label_id = ?))",
    )
    .bind(clip_id)
    .bind(label_id.0)
    .bind(added_by)
    .bind(-GAP)
    .bind(GAP)
    .bind(label_id.0)
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

/// Move a clip to just after `after_clip_id` in a label's playlist — or
/// to the front when that is `None` — and return the resulting order.
///
/// The move names a neighbour rather than an index on purpose. An index
/// is a statement about a list that may have changed since the client
/// read it; a neighbour is a statement about content, and still means
/// the right thing when someone else has inserted a clip meanwhile.
///
/// The common case writes exactly one row, at the midpoint of the gap
/// the clip was dropped into. Only when that gap is used up — the
/// neighbours less than two apart, which includes two rows tied by a
/// race — is the whole label renumbered, in the same transaction.
pub async fn reorder(
    pool: &SqlitePool,
    label_id: i64,
    clip_id: i64,
    after_clip_id: Option<i64>,
) -> AppResult<Vec<i64>> {
    if after_clip_id == Some(clip_id) {
        return Err(AppError::BadRequest(
            "a clip cannot be moved after itself".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT clip_id, position FROM clip_labels WHERE label_id = ? ORDER BY position, clip_id",
    )
    .bind(label_id)
    .fetch_all(&mut *tx)
    .await?;

    // A clip the request names but the playlist doesn't hold means the
    // client is working from a list that no longer exists — worth saying
    // so, rather than moving something to a place that isn't there.
    let carries = |id: i64| rows.iter().any(|(c, _)| *c == id);
    if !carries(clip_id) {
        return Err(AppError::BadRequest(format!(
            "clip {clip_id} does not carry this label"
        )));
    }
    if let Some(after) = after_clip_id {
        if !carries(after) {
            return Err(AppError::BadRequest(format!(
                "clip {after} does not carry this label"
            )));
        }
    }

    // The order as it will be: the moved clip lifted out, then put back
    // where it was dropped.
    let mut order: Vec<(i64, i64)> = rows
        .iter()
        .copied()
        .filter(|(id, _)| *id != clip_id)
        .collect();
    let at = match after_clip_id {
        Some(after) => {
            order
                .iter()
                .position(|(id, _)| *id == after)
                .expect("after_clip_id was found above")
                + 1
        }
        None => 0,
    };
    let before = at.checked_sub(1).map(|i| order[i].1);
    let behind = order.get(at).map(|(_, pos)| *pos);

    // Where the moved row goes, or None when the neighbours have no room
    // between them and the label has to be renumbered.
    let target = match (before, behind) {
        (Some(a), Some(b)) if b - a >= 2 => Some(a + (b - a) / 2),
        (Some(_), Some(_)) => None,
        (None, Some(b)) => Some(b - GAP),
        (Some(a), None) => Some(a + GAP),
        (None, None) => Some(0),
    };

    match target {
        Some(pos) => {
            set_position(&mut tx, label_id, clip_id, pos).await?;
            order.insert(at, (clip_id, pos));
        }
        None => {
            order.insert(at, (clip_id, 0));
            for (i, (id, _)) in order.iter().enumerate() {
                set_position(&mut tx, label_id, *id, i as i64 * GAP).await?;
            }
        }
    }
    tx.commit().await?;

    Ok(order.into_iter().map(|(id, _)| id).collect())
}

async fn set_position(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    label_id: i64,
    clip_id: i64,
    position: i64,
) -> AppResult<()> {
    sqlx::query("UPDATE clip_labels SET position = ? WHERE label_id = ? AND clip_id = ?")
        .bind(position)
        .bind(label_id)
        .bind(clip_id)
        .execute(&mut **tx)
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
