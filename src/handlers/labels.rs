use askama::Template;
use axum::{
    extract::{Path as AxumPath, Query, State},
    response::{IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;

use crate::{
    datefmt,
    error::{AppError, AppResult},
    handlers::{clips, render},
    markdown,
    web::{AppState, CurrentUser, CurrentUserApi},
};

/// Cap on a wiki page's Markdown source. Generous for prose; just keeps
/// a single revision from being unbounded.
const MAX_WIKI_BYTES: usize = 100 * 1024;

#[derive(Template)]
#[template(path = "_label_list.html")]
struct LabelList {
    clip_id: i64,
    labels: Vec<clips::ClipLabel>,
}

#[derive(Deserialize)]
pub struct AddForm {
    name: String,
}

pub async fn add(
    State(state): State<AppState>,
    CurrentUserApi(user): CurrentUserApi,
    AxumPath(clip_id): AxumPath<i64>,
    Form(form): Form<AddForm>,
) -> AppResult<Response> {
    let raw = form.name.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("label is empty".into()));
    }
    // Normalise to lowercase, then validate — so "Verse-1" becomes
    // "verse-1" silently but "verse 1" or "--bad" fails loudly.
    let normalised = raw.to_lowercase();
    if !is_valid_label(&normalised) {
        return Err(AppError::BadRequest(
            "labels must be lower-kebab-case: letters/digits separated by single dashes, no spaces or other punctuation".into(),
        ));
    }

    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM clips WHERE id = ?")
        .bind(clip_id)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO labels (name) VALUES (?)")
        .bind(&normalised)
        .execute(&mut *tx)
        .await?;
    let label_id: (i64,) = sqlx::query_as("SELECT id FROM labels WHERE name = ?")
        .bind(&normalised)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO clip_labels (clip_id, label_id, added_by) VALUES (?, ?, ?)")
        .bind(clip_id)
        .bind(label_id.0)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let labels = clips::load_labels(&state, clip_id).await?;
    render(LabelList { clip_id, labels })
}

pub async fn remove(
    State(state): State<AppState>,
    _user: CurrentUserApi,
    AxumPath((clip_id, label_id)): AxumPath<(i64, i64)>,
) -> AppResult<Response> {
    sqlx::query("DELETE FROM clip_labels WHERE clip_id = ? AND label_id = ?")
        .bind(clip_id)
        .bind(label_id)
        .execute(&state.pool)
        .await?;
    let labels = clips::load_labels(&state, clip_id).await?;
    render(LabelList { clip_id, labels })
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    clip_id: Option<i64>,
}

#[derive(Template)]
#[template(path = "_label_suggestions.html")]
struct Suggestions<'a> {
    query: &'a str,
    matches: Vec<String>,
    can_create: bool,
}

pub async fn search(
    State(state): State<AppState>,
    _user: CurrentUserApi,
    Query(q): Query<SearchQuery>,
) -> AppResult<Response> {
    let raw_query = q.q.unwrap_or_default();
    let trimmed = raw_query.trim();

    // Empty query: show the labels most recently used on any clip,
    // minus any already on this clip. This way focusing the input
    // immediately shows pickable options.
    if trimmed.is_empty() {
        let matches = recent_labels(&state, q.clip_id).await?;
        return render(Suggestions {
            query: "",
            matches,
            can_create: false,
        });
    }

    let normalised = trimmed.to_lowercase();
    // Escape SQL LIKE metacharacters: `\` first (so we don't double-
    // escape our own backslashes), then `%` and `_`. The query is sent
    // with `ESCAPE '\'` so SQLite treats the prefixed chars as literal.
    let pattern = format!(
        "%{}%",
        normalised
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );

    let matches: Vec<(String,)> = if let Some(clip_id) = q.clip_id {
        sqlx::query_as(
            "SELECT name FROM labels WHERE name LIKE ? ESCAPE '\\'
             AND id NOT IN (SELECT label_id FROM clip_labels WHERE clip_id = ?)
             ORDER BY name COLLATE NOCASE LIMIT 10",
        )
        .bind(&pattern)
        .bind(clip_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT name FROM labels WHERE name LIKE ? ESCAPE '\\'
             ORDER BY name COLLATE NOCASE LIMIT 10",
        )
        .bind(&pattern)
        .fetch_all(&state.pool)
        .await?
    };
    let names: Vec<String> = matches.into_iter().map(|(n,)| n).collect();

    // "Create" option only when (a) it's a valid label and (b) it
    // isn't already an exact match in the result list.
    let exact = names.iter().any(|n| n == &normalised);
    let can_create = !exact && is_valid_label(&normalised);

    render(Suggestions {
        query: &normalised,
        matches: names,
        can_create,
    })
}

/// Most recently used labels across all clips, optionally excluding
/// labels already on the given clip. 10 results.
async fn recent_labels(state: &AppState, exclude_clip: Option<i64>) -> AppResult<Vec<String>> {
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
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT l.name FROM labels l
             LEFT JOIN clip_labels ct ON ct.label_id = l.id
             GROUP BY l.id
             ORDER BY COALESCE(MAX(ct.added_at), '') DESC, l.name COLLATE NOCASE
             LIMIT 10",
        )
        .fetch_all(&state.pool)
        .await?
    };
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

/// Lower-kebab-case validator. Allows any Unicode lowercase letter
/// (so å, ä, é, ü, ñ etc. work), ASCII digits, and hyphens. No
/// leading/trailing/consecutive hyphens. Caller is expected to have
/// already lowercased the input.
pub fn is_valid_label(s: &str) -> bool {
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

// ---------------------------------------------------------------------------
// Label wiki pages
// ---------------------------------------------------------------------------
//
// Each label can have a Markdown wiki page, edited in the UI. The page is
// the newest row in label_wiki_revisions for that label; every save
// appends a revision, so the full history (author + timestamp) is kept.
// The view/edit partials share an outer `#wiki-<id>` container and swap
// via HTMX outerHTML, the same shape as the clip-rename inline editor.

#[derive(Template)]
#[template(path = "_label_wiki.html")]
pub(crate) struct LabelWikiView {
    label_id: i64,
    label_name: String,
    /// Rendered Markdown (already safe HTML), empty when there's no page.
    content_html: String,
    has_content: bool,
    /// "edited by <user> on <date time>" for the current revision.
    last_edited: Option<String>,
}

#[derive(Template)]
#[template(path = "_label_wiki_edit.html")]
struct LabelWikiEdit {
    label_id: i64,
    label_name: String,
    /// Raw Markdown source being edited.
    content: String,
}

#[derive(Template)]
#[template(path = "label_wiki_history.html")]
struct LabelWikiHistoryPage {
    username: String,
    label_id: i64,
    label_name: String,
    revisions: Vec<WikiRevision>,
}

struct WikiRevision {
    id: i64,
    author: String,
    edited_at: String,
    content_html: String,
    is_current: bool,
}

#[derive(Deserialize)]
pub struct WikiForm {
    content: String,
}

/// Resolve a label id to its canonical name, 404 if it doesn't exist.
async fn label_name(state: &AppState, label_id: i64) -> AppResult<String> {
    sqlx::query_as::<_, (String,)>("SELECT name FROM labels WHERE id = ?")
        .bind(label_id)
        .fetch_optional(&state.pool)
        .await?
        .map(|(n,)| n)
        .ok_or(AppError::NotFound)
}

/// Build the view partial for a label's current wiki page.
pub(crate) async fn load_wiki_view(
    state: &AppState,
    label_id: i64,
    label_name: &str,
) -> AppResult<LabelWikiView> {
    let latest: Option<(String, String, String)> = sqlx::query_as(
        "SELECT r.content, u.username, r.edited_at
         FROM label_wiki_revisions r JOIN users u ON u.id = r.edited_by
         WHERE r.label_id = ?
         ORDER BY r.id DESC LIMIT 1",
    )
    .bind(label_id)
    .fetch_optional(&state.pool)
    .await?;

    Ok(match latest {
        Some((content, author, edited_at)) => LabelWikiView {
            label_id,
            label_name: label_name.to_string(),
            content_html: markdown::render(&content),
            has_content: true,
            last_edited: Some(format!(
                "edited by {author} on {}",
                datefmt::datetime_from_rfc3339(&edited_at)
            )),
        },
        None => LabelWikiView {
            label_id,
            label_name: label_name.to_string(),
            content_html: String::new(),
            has_content: false,
            last_edited: None,
        },
    })
}

/// Pre-render the wiki view partials for each active filter label that
/// exists, in the given order. Used by the clip list to show wikis above
/// the clips. Labels in `active` that don't exist are skipped.
pub(crate) async fn active_wikis_html(state: &AppState, active: &[&str]) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for name in active {
        let row: Option<(i64, String)> =
            sqlx::query_as("SELECT id, name FROM labels WHERE name = ? COLLATE NOCASE")
                .bind(name)
                .fetch_optional(&state.pool)
                .await?;
        let Some((label_id, canonical)) = row else {
            continue;
        };
        let view = load_wiki_view(state, label_id, &canonical).await?;
        out.push(view.render()?);
    }
    Ok(out)
}

/// GET /labels/{id}/wiki — the view partial (used by the edit form's
/// Cancel to swap back).
pub async fn wiki_view(
    State(state): State<AppState>,
    _user: CurrentUserApi,
    AxumPath(label_id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    render(load_wiki_view(&state, label_id, &name).await?)
}

/// GET /labels/{id}/wiki/edit — the inline edit form, pre-filled with the
/// current raw Markdown.
pub async fn wiki_edit_form(
    State(state): State<AppState>,
    _user: CurrentUserApi,
    AxumPath(label_id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    let content: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM label_wiki_revisions
         WHERE label_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(label_id)
    .fetch_optional(&state.pool)
    .await?;
    render(LabelWikiEdit {
        label_id,
        label_name: name,
        content: content.map(|(c,)| c).unwrap_or_default(),
    })
}

/// POST /labels/{id}/wiki — save a new revision, return the view partial.
pub async fn wiki_save(
    State(state): State<AppState>,
    CurrentUserApi(user): CurrentUserApi,
    AxumPath(label_id): AxumPath<i64>,
    Form(form): Form<WikiForm>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    if form.content.len() > MAX_WIKI_BYTES {
        return Err(AppError::BadRequest(format!(
            "wiki page exceeds {MAX_WIKI_BYTES}-byte limit"
        )));
    }
    insert_revision(&state, label_id, &form.content, user.id).await?;
    state.discord.wiki_edited(&user.username, &name);
    render(load_wiki_view(&state, label_id, &name).await?)
}

/// GET /labels/{id}/wiki/history — full page listing every revision.
pub async fn wiki_history(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    AxumPath(label_id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    let rows: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT r.id, u.username, r.edited_at, r.content
         FROM label_wiki_revisions r JOIN users u ON u.id = r.edited_by
         WHERE r.label_id = ?
         ORDER BY r.id DESC",
    )
    .bind(label_id)
    .fetch_all(&state.pool)
    .await?;

    let revisions: Vec<WikiRevision> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (id, author, edited_at, content))| WikiRevision {
            id,
            author,
            edited_at: datefmt::datetime_from_rfc3339(&edited_at),
            content_html: markdown::render(&content),
            is_current: i == 0, // newest first
        })
        .collect();

    render(LabelWikiHistoryPage {
        username: user.username,
        label_id,
        label_name: name,
        revisions,
    })
}

/// POST /labels/{id}/wiki/restore/{rev} — copy an old revision's content
/// into a new revision, then back to the history page.
pub async fn wiki_restore(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    AxumPath((label_id, rev_id)): AxumPath<(i64, i64)>,
) -> AppResult<Response> {
    // Scope the lookup to this label so a mismatched id can't pull in
    // another label's content.
    let content: Option<(String,)> =
        sqlx::query_as("SELECT content FROM label_wiki_revisions WHERE id = ? AND label_id = ?")
            .bind(rev_id)
            .bind(label_id)
            .fetch_optional(&state.pool)
            .await?;
    let content = content.map(|(c,)| c).ok_or(AppError::NotFound)?;

    insert_revision(&state, label_id, &content, user.id).await?;
    let name = label_name(&state, label_id).await?;
    state.discord.wiki_edited(&user.username, &name);
    Ok(Redirect::to(&format!("/labels/{label_id}/wiki/history")).into_response())
}

async fn insert_revision(
    state: &AppState,
    label_id: i64,
    content: &str,
    user_id: i64,
) -> AppResult<()> {
    sqlx::query("INSERT INTO label_wiki_revisions (label_id, content, edited_by) VALUES (?, ?, ?)")
        .bind(label_id)
        .bind(content)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_valid_label;

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
            assert!(is_valid_label(s), "expected valid: {s}");
        }
    }

    #[test]
    fn rejects_bad_shapes() {
        for s in [
            "", "-verse", "verse-", "verse--1", "Verse", "verse 1", "verse_1", "verse.1",
        ] {
            assert!(!is_valid_label(s), "expected invalid: {s}");
        }
    }
}
