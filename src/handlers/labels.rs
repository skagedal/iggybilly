use askama::Template;
use axum::{
    Form,
    extract::{Path as AxumPath, Query, State},
    response::Response,
};
use serde::Deserialize;

use crate::{
    error::{AppError, AppResult},
    handlers::{clips, render},
    web::{AppState, CurrentUserApi},
};

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
    sqlx::query(
        "INSERT OR IGNORE INTO clip_labels (clip_id, label_id, added_by) VALUES (?, ?, ?)",
    )
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
        return render(Suggestions { query: "", matches, can_create: false });
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

    render(Suggestions { query: &normalised, matches: names, can_create })
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

#[cfg(test)]
mod tests {
    use super::is_valid_label;

    #[test]
    fn accepts_kebab_case_with_diacritics() {
        for s in ["verse", "verse-1", "verse-one", "pålägg", "café-version", "über-mix", "x"] {
            assert!(is_valid_label(s), "expected valid: {s}");
        }
    }

    #[test]
    fn rejects_bad_shapes() {
        for s in ["", "-verse", "verse-", "verse--1", "Verse", "verse 1", "verse_1", "verse.1"] {
            assert!(!is_valid_label(s), "expected invalid: {s}");
        }
    }
}
