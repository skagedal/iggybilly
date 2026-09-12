//! Labels and their wiki pages, as the app sees them.
//!
//! The one real difference from the web is the wiki: these endpoints
//! return Markdown source, never rendered HTML. Flutter has a Markdown
//! widget and wants the source; the browser wants HTML. Rendering at
//! each edge keeps one stored form and lets both be right.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    queries,
    web::{ApiUser, AppState},
};

use super::clips::Label;

/// Cap on a wiki page's Markdown source, matching the web's.
const MAX_WIKI_BYTES: usize = 100 * 1024;

#[derive(Deserialize)]
pub struct AddRequest {
    name: String,
}

/// POST /api/v1/clips/{id}/labels — returns the clip's full label list,
/// so the client replaces its state rather than guessing what the server
/// did with the name it sent.
pub async fn add(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path(clip_id): Path<i64>,
    Json(req): Json<AddRequest>,
) -> AppResult<Response> {
    let raw = req.name.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("label is empty".into()));
    }
    // Lowercase first, then validate: "Verse-1" becomes "verse-1"
    // quietly, while "verse 1" is rejected loudly.
    let normalised = raw.to_lowercase();
    if !queries::labels::is_valid(&normalised) {
        return Err(AppError::BadRequest(
            "labels must be lower-kebab-case: letters/digits separated by single dashes, no spaces or other punctuation".into(),
        ));
    }
    if !queries::clips::exists(&state.pool, clip_id).await? {
        return Err(AppError::NotFound);
    }
    queries::labels::add_to_clip(&state.pool, clip_id, &normalised, user.id).await?;
    labels_response(&state, clip_id).await
}

/// DELETE /api/v1/clips/{id}/labels/{label_id}
pub async fn remove(
    State(state): State<AppState>,
    _user: ApiUser,
    Path((clip_id, label_id)): Path<(i64, i64)>,
) -> AppResult<Response> {
    queries::labels::remove_from_clip(&state.pool, clip_id, label_id).await?;
    labels_response(&state, clip_id).await
}

async fn labels_response(state: &AppState, clip_id: i64) -> AppResult<Response> {
    let labels: Vec<Label> = queries::clips::labels_for(&state.pool, clip_id)
        .await?
        .into_iter()
        .map(|l| Label {
            id: l.id,
            name: l.name,
        })
        .collect();
    Ok(Json(labels).into_response())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Playlist {
    label_id: i64,
    label_name: String,
    /// Summed over the clips whose duration is known; the app counts the
    /// unknown ones off the list itself.
    total_seconds: f64,
    clips: Vec<super::clips::Clip>,
}

/// GET /api/v1/labels/{id}/playlist — the label's clips in playlist
/// order, in the shape `/api/v1/clips` already sends them.
///
/// A route of its own rather than a mode of `/api/v1/clips`: making that
/// answer an object instead of an array when it happens to be given one
/// label would break every installed app the moment the server rolled.
pub async fn playlist(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path(label_id): Path<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    let rows = queries::clips::list(
        &state.pool,
        &[name.as_str()],
        queries::clips::ListOrder::Playlist { label_id },
    )
    .await?;
    let total_seconds: f64 = rows.iter().filter_map(|c| c.duration_seconds).sum();
    Ok(Json(Playlist {
        label_id,
        label_name: name,
        total_seconds,
        clips: rows
            .into_iter()
            .map(|c| super::clips::clip(c, user.id))
            .collect(),
    })
    .into_response())
}

/// POST /api/v1/labels/{id}/order — move a clip within the playlist.
/// The move itself is the web's `set_order`, so both surfaces order a
/// playlist the same way and refuse the same requests.
pub async fn order(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(label_id): Path<i64>,
    Json(req): Json<crate::handlers::labels::OrderRequest>,
) -> AppResult<Response> {
    let order = crate::handlers::labels::set_order(&state, label_id, &req).await?;
    Ok(Json(order).into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    q: Option<String>,
    clip_id: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Suggestions {
    query: String,
    matches: Vec<String>,
    can_create: bool,
}

/// GET /api/v1/labels/search
pub async fn search(
    State(state): State<AppState>,
    _user: ApiUser,
    Query(q): Query<SearchQuery>,
) -> AppResult<Response> {
    let s = queries::labels::suggest(&state.pool, &q.q.unwrap_or_default(), q.clip_id).await?;
    Ok(Json(Suggestions {
        query: s.query,
        matches: s.matches,
        can_create: s.can_create,
    })
    .into_response())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WikiPage {
    label_id: i64,
    label_name: String,
    /// Markdown source. Empty when the label has no page yet, which the
    /// app shows as an invitation to write one.
    content: String,
    has_content: bool,
    last_edited_by: Option<String>,
    /// RFC 3339, as stored.
    last_edited_at: Option<String>,
}

fn wiki_page(p: queries::wiki::Page) -> WikiPage {
    let (by, at) = match p.last_edited {
        Some(e) => (Some(e.author), Some(e.edited_at)),
        None => (None, None),
    };
    WikiPage {
        label_id: p.label_id,
        label_name: p.label_name,
        content: p.content,
        has_content: p.has_content,
        last_edited_by: by,
        last_edited_at: at,
    }
}

async fn label_name(state: &AppState, label_id: i64) -> AppResult<String> {
    queries::labels::name_of(&state.pool, label_id)
        .await?
        .ok_or(AppError::NotFound)
}

/// GET /api/v1/labels/{id}/wiki
pub async fn wiki(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(label_id): Path<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    let page = queries::wiki::page(&state.pool, label_id, &name).await?;
    Ok(Json(wiki_page(page)).into_response())
}

#[derive(Deserialize)]
pub struct WikiRequest {
    content: String,
}

/// POST /api/v1/labels/{id}/wiki — append a revision, return the page as
/// it now stands.
pub async fn save_wiki(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path(label_id): Path<i64>,
    Json(req): Json<WikiRequest>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    if req.content.len() > MAX_WIKI_BYTES {
        return Err(AppError::BadRequest(format!(
            "wiki page exceeds {MAX_WIKI_BYTES}-byte limit"
        )));
    }
    queries::wiki::save(&state.pool, label_id, &req.content, user.id).await?;
    state.discord.wiki_edited(&user.username, &name);
    let page = queries::wiki::page(&state.pool, label_id, &name).await?;
    Ok(Json(wiki_page(page)).into_response())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Revision {
    id: i64,
    author: String,
    /// RFC 3339, as stored.
    edited_at: String,
    /// Markdown source of this revision.
    content: String,
    is_current: bool,
}

/// GET /api/v1/labels/{id}/wiki/history
pub async fn wiki_history(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(label_id): Path<i64>,
) -> AppResult<Response> {
    // Resolve the label first, so a bad id is a 404 rather than an
    // empty history that looks like a page nobody has written.
    label_name(&state, label_id).await?;
    let revisions: Vec<Revision> = queries::wiki::history(&state.pool, label_id)
        .await?
        .into_iter()
        .map(|r| Revision {
            id: r.id,
            author: r.author,
            edited_at: r.edited_at,
            content: r.content,
            is_current: r.is_current,
        })
        .collect();
    Ok(Json(revisions).into_response())
}

/// POST /api/v1/labels/{id}/wiki/restore/{rev} — copy an old revision
/// forward as a new one, so nothing is lost by restoring.
pub async fn restore_wiki(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path((label_id, rev_id)): Path<(i64, i64)>,
) -> AppResult<Response> {
    let content = queries::wiki::revision_content(&state.pool, label_id, rev_id)
        .await?
        .ok_or(AppError::NotFound)?;
    queries::wiki::save(&state.pool, label_id, &content, user.id).await?;
    let name = label_name(&state, label_id).await?;
    state.discord.wiki_edited(&user.username, &name);
    Ok(StatusCode::NO_CONTENT.into_response())
}
