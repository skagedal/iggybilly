//! Reading clips and their labels.

use serde_json::value::RawValue;
use sqlx::SqlitePool;

use crate::error::AppResult;

/// A label as the database holds it. The web turns this into a filter
/// link and the app into a chip; neither concern belongs here.
#[derive(Debug, Clone)]
pub struct Label {
    pub id: i64,
    pub name: String,
}

/// One clip, with everything both surfaces need.
///
/// `uploaded_at` is the stored RFC 3339 string, not a formatted date:
/// the web renders it as a plain ISO day and the app formats it in the
/// device's locale and time zone, so the raw value is the only one that
/// serves both.
#[derive(Debug)]
pub struct Clip {
    pub id: i64,
    pub name: String,
    pub original_filename: String,
    pub content_type: String,
    pub uploaded_at: String,
    pub recording_date: Option<String>,
    pub uploader: String,
    /// The uploader's user id, which is what "may I delete this?" turns
    /// on — only the uploader may.
    pub uploaded_by: i64,
    pub labels: Vec<Label>,
    /// The stored peaks array, passed through as raw JSON so a few
    /// hundred floats per clip are never parsed just to be re-emitted.
    pub peaks: Option<Box<RawValue>>,
    pub duration_seconds: Option<f64>,
}

/// The columns every clip read selects, in the order `row_to_clip`
/// expects. Kept as one string so the list and the single-clip query
/// cannot drift apart.
const CLIP_COLUMNS: &str = "c.id, c.name, c.original_filename, c.content_type,
     c.uploaded_at, c.recording_date, u.username, c.uploaded_by,
     c.peaks, c.duration_seconds";

type ClipRow = (
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    i64,
    Option<String>,
    Option<f64>,
);

fn row_to_clip(row: ClipRow, labels: Vec<Label>) -> Clip {
    let (
        id,
        name,
        original_filename,
        content_type,
        uploaded_at,
        recording_date,
        uploader,
        uploaded_by,
        peaks,
        duration_seconds,
    ) = row;
    Clip {
        id,
        name,
        original_filename,
        content_type,
        uploaded_at,
        recording_date,
        uploader,
        uploaded_by,
        labels,
        peaks: peaks.and_then(|s| RawValue::from_string(s).ok()),
        duration_seconds,
    }
}

/// Every clip carrying *all* of `active` (an AND filter), newest first.
/// An empty filter lists everything.
pub async fn list(pool: &SqlitePool, active: &[&str]) -> AppResult<Vec<Clip>> {
    let rows: Vec<ClipRow> = if active.is_empty() {
        sqlx::query_as(&format!(
            "SELECT {CLIP_COLUMNS}
             FROM clips c JOIN users u ON u.id = c.uploaded_by
             ORDER BY c.uploaded_at DESC"
        ))
        .fetch_all(pool)
        .await?
    } else {
        // AND semantics: match any of the labels, then keep only the
        // clips that matched all of them. sqlx doesn't expand a Vec into
        // an IN-list, so the placeholders are built by hand — the values
        // are still bound, never interpolated.
        let placeholders = vec!["?"; active.len()].join(", ");
        let sql = format!(
            "SELECT {CLIP_COLUMNS}
             FROM clips c
             JOIN users u ON u.id = c.uploaded_by
             JOIN clip_labels cl ON cl.clip_id = c.id
             JOIN labels l ON l.id = cl.label_id
             WHERE l.name IN ({placeholders}) COLLATE NOCASE
             GROUP BY c.id
             HAVING COUNT(DISTINCT l.id) = ?
             ORDER BY c.uploaded_at DESC"
        );
        let mut q = sqlx::query_as(&sql);
        for t in active {
            q = q.bind(*t);
        }
        q = q.bind(active.len() as i64);
        q.fetch_all(pool).await?
    };

    let mut clips = Vec::with_capacity(rows.len());
    for row in rows {
        let labels = labels_for(pool, row.0).await?;
        clips.push(row_to_clip(row, labels));
    }
    Ok(clips)
}

/// One clip by id, or `None` if there isn't one.
pub async fn get(pool: &SqlitePool, id: i64) -> AppResult<Option<Clip>> {
    let row: Option<ClipRow> = sqlx::query_as(&format!(
        "SELECT {CLIP_COLUMNS}
         FROM clips c JOIN users u ON u.id = c.uploaded_by
         WHERE c.id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => {
            let labels = labels_for(pool, row.0).await?;
            Ok(Some(row_to_clip(row, labels)))
        }
        None => Ok(None),
    }
}

/// A clip's labels, alphabetically and case-insensitively.
pub async fn labels_for(pool: &SqlitePool, clip_id: i64) -> AppResult<Vec<Label>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT l.id, l.name FROM labels l
         JOIN clip_labels cl ON cl.label_id = l.id
         WHERE cl.clip_id = ? ORDER BY l.name COLLATE NOCASE",
    )
    .bind(clip_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| Label { id, name })
        .collect())
}

/// Whether a clip exists, without loading it.
pub async fn exists(pool: &SqlitePool, clip_id: i64) -> AppResult<bool> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM clips WHERE id = ?")
        .bind(clip_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// What `audio` needs to serve the bytes: where they are, what to call
/// the type, and the name to offer on download.
pub struct AudioFile {
    pub storage_filename: String,
    pub content_type: String,
    pub original_filename: String,
}

pub async fn audio_file(pool: &SqlitePool, id: i64) -> AppResult<Option<AudioFile>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT storage_filename, content_type, original_filename FROM clips WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(storage_filename, content_type, original_filename)| AudioFile {
            storage_filename,
            content_type,
            original_filename,
        },
    ))
}
