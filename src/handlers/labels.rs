use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    datefmt,
    error::{AppError, AppResult},
    handlers::{clips, page, PageFormat},
    markdown, queries,
    web::{ApiUser, AppState, CurrentUser},
};

/// Cap on a wiki page's Markdown source. Generous for prose; just keeps
/// a single revision from being unbounded.
const MAX_WIKI_BYTES: usize = 100 * 1024;

#[derive(Deserialize)]
pub struct AddRequest {
    name: String,
}

/// POST /api/clips/{id}/labels — add a label, returning the clip's full
/// label list so the client can replace its state wholesale rather than
/// guess at what the server did with the name.
pub async fn add(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    AxumPath(clip_id): AxumPath<i64>,
    Json(req): Json<AddRequest>,
) -> AppResult<Response> {
    let raw = req.name.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("label is empty".into()));
    }
    // Normalise to lowercase, then validate — so "Verse-1" becomes
    // "verse-1" silently but "verse 1" or "--bad" fails loudly.
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

    let labels = clips::load_labels(&state, clip_id).await?;
    Ok(Json(labels).into_response())
}

pub async fn remove(
    State(state): State<AppState>,
    _user: ApiUser,
    AxumPath((clip_id, label_id)): AxumPath<(i64, i64)>,
) -> AppResult<Response> {
    queries::labels::remove_from_clip(&state.pool, clip_id, label_id).await?;
    let labels = clips::load_labels(&state, clip_id).await?;
    Ok(Json(labels).into_response())
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
    clip_id: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Suggestions {
    /// The normalised query the suggestions were computed for — the
    /// client uses it as the name to create when `can_create` is set.
    query: String,
    matches: Vec<String>,
    can_create: bool,
}

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

// ---------------------------------------------------------------------------
// Label wiki pages
// ---------------------------------------------------------------------------
//
// Each label can have a Markdown wiki page, edited in the UI. The page is
// the newest row in label_wiki_revisions for that label; every save
// appends a revision, so the full history (author + timestamp) is kept.
// One payload carries both the raw source (for the editor) and the
// rendered HTML (for the view), so switching between them is local state
// in the client rather than a round trip.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WikiPage {
    label_id: i64,
    label_name: String,
    /// Raw Markdown source, empty when there's no page yet.
    content: String,
    /// Rendered Markdown (already safe HTML), empty when there's no page.
    content_html: String,
    has_content: bool,
    /// "edited by <user> on <date time>" for the current revision.
    last_edited: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WikiHistoryProps {
    username: String,
    label_id: i64,
    label_name: String,
    revisions: Vec<WikiRevision>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WikiRevision {
    id: i64,
    author: String,
    edited_at: String,
    content_html: String,
    is_current: bool,
}

#[derive(Deserialize)]
pub struct WikiRequest {
    content: String,
}

/// Resolve a label id to its canonical name, 404 if it doesn't exist.
async fn label_name(state: &AppState, label_id: i64) -> AppResult<String> {
    queries::labels::name_of(&state.pool, label_id)
        .await?
        .ok_or(AppError::NotFound)
}

/// A stored wiki page in the shape the frontend expects: rendered HTML
/// alongside the source, and the "edited by … on …" line already
/// assembled, since that sentence is the web's wording.
fn wiki_props(page: queries::wiki::Page) -> WikiPage {
    WikiPage {
        label_id: page.label_id,
        label_name: page.label_name,
        content_html: markdown::render(&page.content),
        content: page.content,
        has_content: page.has_content,
        last_edited: page.last_edited.map(|e| {
            format!(
                "edited by {} on {}",
                e.author,
                datefmt::datetime_from_rfc3339(&e.edited_at)
            )
        }),
    }
}

/// Load a label's current wiki page, in both source and rendered form.
pub(crate) async fn load_wiki_page(
    state: &AppState,
    label_id: i64,
    label_name: &str,
) -> AppResult<WikiPage> {
    Ok(wiki_props(
        queries::wiki::page(&state.pool, label_id, label_name).await?,
    ))
}

/// The wiki page for each active filter label that exists, in the given
/// order. Used by the clip list to show wikis above the clips. Labels in
/// `active` that don't exist are skipped.
pub(crate) async fn active_wikis(state: &AppState, active: &[&str]) -> AppResult<Vec<WikiPage>> {
    let mut out = Vec::new();
    for name in active {
        let Some(label) = queries::labels::find_by_name(&state.pool, name).await? else {
            continue;
        };
        out.push(load_wiki_page(state, label.id, &label.name).await?);
    }
    Ok(out)
}

/// GET /api/labels/{id}/wiki — a label's current wiki page.
pub async fn wiki_view(
    State(state): State<AppState>,
    _user: ApiUser,
    AxumPath(label_id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    Ok(Json(load_wiki_page(&state, label_id, &name).await?).into_response())
}

/// POST /api/labels/{id}/wiki — save a new revision, return the page as
/// it now stands.
pub async fn wiki_save(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    AxumPath(label_id): AxumPath<i64>,
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
    Ok(Json(load_wiki_page(&state, label_id, &name).await?).into_response())
}

/// GET /labels/{id}/wiki/history — full page listing every revision.
pub async fn wiki_history(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    format: PageFormat,
    AxumPath(label_id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = label_name(&state, label_id).await?;
    let revisions: Vec<WikiRevision> = queries::wiki::history(&state.pool, label_id)
        .await?
        .into_iter()
        .map(|r| WikiRevision {
            id: r.id,
            author: r.author,
            edited_at: datefmt::datetime_from_rfc3339(&r.edited_at),
            content_html: markdown::render(&r.content),
            is_current: r.is_current,
        })
        .collect();

    let title = format!("{name} wiki history — iggybilly");
    page(
        &state,
        format,
        &title,
        "wiki-history",
        &WikiHistoryProps {
            username: user.username,
            label_id,
            label_name: name,
            revisions,
        },
    )
}

/// POST /api/labels/{id}/wiki/restore/{rev} — copy an old revision's
/// content into a new revision. The client reloads the history page to
/// see it.
pub async fn wiki_restore(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    AxumPath((label_id, rev_id)): AxumPath<(i64, i64)>,
) -> AppResult<Response> {
    // Scoped to this label, so a mismatched pair of ids cannot pull in
    // another label's content.
    let content = queries::wiki::revision_content(&state.pool, label_id, rev_id)
        .await?
        .ok_or(AppError::NotFound)?;

    queries::wiki::save(&state.pool, label_id, &content, user.id).await?;
    let name = label_name(&state, label_id).await?;
    state.discord.wiki_edited(&user.username, &name);
    Ok(StatusCode::NO_CONTENT.into_response())
}
