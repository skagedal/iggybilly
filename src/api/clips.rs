//! Clips, as the app sees them.

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::Query;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{
    error::{AppError, AppResult},
    handlers::clips as web_clips,
    queries,
    web::{ApiUser, AppState},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    id: i64,
    name: String,
    original_filename: String,
    content_type: String,
    /// RFC 3339, as stored. The device formats it — a server that
    /// pre-formatted this would be deciding the phone's time zone for it.
    uploaded_at: String,
    /// "YYYY-MM-DD" when the file carried a recording date, else null.
    recording_date: Option<String>,
    uploader: String,
    labels: Vec<Label>,
    /// Normalised waveform peaks, or null when the server couldn't
    /// decode the file. Passed through as raw JSON rather than parsed
    /// and re-emitted.
    peaks: Option<Box<RawValue>>,
    duration_seconds: Option<f64>,
    /// Paths, relative to this server's origin. Named rather than left
    /// for the client to build, so the shape of these URLs stays a
    /// server decision.
    audio_url: String,
    download_url: String,
    /// Whether the caller may delete this clip: only its uploader may.
    /// Sent on every clip so the app never has to re-derive the rule.
    can_delete: bool,
}

pub(super) fn clip(c: queries::clips::Clip, viewer_id: i64) -> Clip {
    Clip {
        audio_url: format!("/clips/{}/audio", c.id),
        download_url: format!("/clips/{}/audio?download=1", c.id),
        can_delete: c.uploaded_by == viewer_id,
        id: c.id,
        name: c.name,
        original_filename: c.original_filename,
        content_type: c.content_type,
        uploaded_at: c.uploaded_at,
        recording_date: c.recording_date,
        uploader: c.uploader,
        labels: c
            .labels
            .into_iter()
            .map(|l| Label {
                id: l.id,
                name: l.name,
            })
            .collect(),
        peaks: c.peaks,
        duration_seconds: c.duration_seconds,
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Repeated ?label=foo&label=bar AND-filters the list, exactly as on
    /// the web.
    #[serde(default, rename = "label")]
    labels: Vec<String>,
}

/// GET /api/v1/clips
pub async fn list(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    // Trim, drop empties, and dedupe case-insensitively, so a filter
    // repeated by a client doesn't make the AND unsatisfiable.
    let mut active: Vec<String> = Vec::new();
    for raw in q.labels {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if !active.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            active.push(t.to_string());
        }
    }
    let active_strs: Vec<&str> = active.iter().map(|s| s.as_str()).collect();

    // Always newest first here: the playlist order of a single label is
    // its own route, so this one keeps the shape every installed app
    // already expects.
    let clips: Vec<Clip> =
        queries::clips::list(&state.pool, &active_strs, queries::clips::ListOrder::Recent)
            .await?
            .into_iter()
            .map(|c| clip(c, user.id))
            .collect();
    Ok(Json(clips).into_response())
}

/// GET /api/v1/clips/{id}
pub async fn detail(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    let found = queries::clips::get(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(clip(found, user.id)).into_response())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadResponse {
    clips: Vec<UploadedClip>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadedClip {
    id: i64,
    name: String,
}

/// POST /api/v1/clips — multipart, one `audio` part per file, same as
/// the web. The storing itself is `handlers::clips::ingest`, so both
/// surfaces share the format allow-list, the size cap, and the waveform
/// pass.
pub async fn upload(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    multipart: Multipart,
) -> AppResult<Response> {
    let uploaded = web_clips::ingest(&state, &user, multipart).await?;
    Ok(Json(UploadResponse {
        clips: uploaded
            .into_iter()
            .map(|(id, name)| UploadedClip { id, name })
            .collect(),
    })
    .into_response())
}

/// DELETE /api/v1/clips/{id} — only the uploader may.
pub async fn delete(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    web_clips::remove(&state, &user, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct RenameRequest {
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameResponse {
    name: String,
}

/// POST /api/v1/clips/{id}/name
pub async fn rename(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(id): Path<i64>,
    Json(req): Json<RenameRequest>,
) -> AppResult<Response> {
    let name = web_clips::set_name(&state, id, &req.name).await?;
    Ok(Json(RenameResponse { name }).into_response())
}
