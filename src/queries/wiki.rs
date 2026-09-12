//! Label wiki pages and their revision history.
//!
//! A label's page is simply the newest row in `label_wiki_revisions`.
//! Saving appends; restoring appends a copy of an older revision. So
//! nothing is ever overwritten and the history is the table itself.

use sqlx::SqlitePool;

use crate::error::AppResult;

/// A label's current page, as stored.
///
/// The Markdown is not rendered here. The web wants HTML it can drop
/// into the document; the app renders Markdown itself with a Flutter
/// widget. Rendering at the edges keeps one source of truth in the
/// database and lets each surface choose.
#[derive(Debug)]
pub struct Page {
    pub label_id: i64,
    pub label_name: String,
    /// Markdown source. Empty when the label has no page yet.
    pub content: String,
    pub has_content: bool,
    pub last_edited: Option<Edit>,
}

/// Who last touched a page and when. `edited_at` is the stored RFC 3339
/// string; each surface formats it.
#[derive(Debug, Clone)]
pub struct Edit {
    pub author: String,
    pub edited_at: String,
}

/// One entry in a page's history.
#[derive(Debug)]
pub struct Revision {
    pub id: i64,
    pub author: String,
    pub edited_at: String,
    pub content: String,
    /// Whether this is the revision currently in force.
    pub is_current: bool,
}

/// A label's current page. A label with no revisions yet gets an empty
/// page rather than a 404 — "no wiki here" and "start writing one" are
/// the same screen.
pub async fn page(pool: &SqlitePool, label_id: i64, label_name: &str) -> AppResult<Page> {
    let latest: Option<(String, String, String)> = sqlx::query_as(
        "SELECT r.content, u.username, r.edited_at
         FROM label_wiki_revisions r JOIN users u ON u.id = r.edited_by
         WHERE r.label_id = ?
         ORDER BY r.id DESC LIMIT 1",
    )
    .bind(label_id)
    .fetch_optional(pool)
    .await?;

    Ok(match latest {
        Some((content, author, edited_at)) => Page {
            label_id,
            label_name: label_name.to_string(),
            content,
            has_content: true,
            last_edited: Some(Edit { author, edited_at }),
        },
        None => Page {
            label_id,
            label_name: label_name.to_string(),
            content: String::new(),
            has_content: false,
            last_edited: None,
        },
    })
}

/// Every revision of a label's page, newest first.
pub async fn history(pool: &SqlitePool, label_id: i64) -> AppResult<Vec<Revision>> {
    let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT r.id, u.username, r.edited_at, r.content
         FROM label_wiki_revisions r JOIN users u ON u.id = r.edited_by
         WHERE r.label_id = ?
         ORDER BY r.id DESC",
    )
    .bind(label_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(i, (id, author, edited_at, content))| Revision {
            id,
            author,
            edited_at,
            content,
            is_current: i == 0, // newest first
        })
        .collect())
}

/// Append a revision.
pub async fn save(
    pool: &SqlitePool,
    label_id: i64,
    content: &str,
    edited_by: i64,
) -> AppResult<()> {
    sqlx::query("INSERT INTO label_wiki_revisions (label_id, content, edited_by) VALUES (?, ?, ?)")
        .bind(label_id)
        .bind(content)
        .bind(edited_by)
        .execute(pool)
        .await?;
    Ok(())
}

/// The content of one revision, looked up within its own label so a
/// mismatched pair of ids cannot pull in another label's text.
pub async fn revision_content(
    pool: &SqlitePool,
    label_id: i64,
    rev_id: i64,
) -> AppResult<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT content FROM label_wiki_revisions WHERE id = ? AND label_id = ?")
            .bind(rev_id)
            .bind(label_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(c,)| c))
}
