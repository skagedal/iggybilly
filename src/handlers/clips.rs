use std::path::{Path, PathBuf};

use askama::Template;
use axum::{
    Form,
    body::Body,
    extract::{Multipart, Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::Query;
use serde::Deserialize;
use sqlx::SqlitePool;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::{
    error::{AppError, AppResult},
    handlers::render,
    web::{AppState, CurrentUser, CurrentUserApi, MAX_UPLOAD_BYTES},
};

#[derive(Debug)]
pub struct ClipRow {
    pub id: i64,
    pub name: String,
    pub original_filename: String,
    pub uploaded_at: String,
    pub recording_date: Option<String>,
    pub uploader: String,
    pub labels: Vec<LabelLink>,
    /// Compact JSON peaks array for WaveSurfer, or None if not computed.
    pub peaks: Option<String>,
    pub duration_seconds: Option<f64>,
}

#[derive(Debug)]
pub struct LabelLink {
    pub name: String,
    pub href: String,
}

#[derive(Debug)]
pub struct FilterChip {
    pub name: String,
    pub remove_href: String,
}

/// Build a "/?label=…&label=…" link from a list of active label filters.
/// Returns "/" when the list is empty so the home page is reachable
/// with no query string.
fn filter_url(labels: &[&str]) -> String {
    if labels.is_empty() {
        return "/".to_string();
    }
    let qs =
        serde_urlencoded::to_string(labels.iter().map(|l| ("label", *l)).collect::<Vec<_>>())
            .unwrap_or_default();
    format!("/?{qs}")
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Repeated ?label=foo&label=bar AND-filters the clip list.
    /// axum_extra's Query (backed by serde_html_form) collects repeated
    /// keys into a Vec.
    #[serde(default, rename = "label")]
    pub labels: Vec<String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexPage {
    username: String,
    clips: Vec<ClipRow>,
    active_filters: Vec<FilterChip>,
    /// Pre-rendered wiki view partials, one per active filter label that
    /// has a label row — shown above the clips.
    active_wikis: Vec<String>,
}

pub async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Query(q): Query<ListQuery>,
) -> AppResult<Response> {
    // Normalise filter labels: trim, dedupe case-insensitively, drop empties.
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
    let clips = load_clips(&state.pool, &active_strs).await?;
    let active_wikis = super::labels::active_wikis_html(&state, &active_strs).await?;

    // For each active filter, the X-button link drops just that one label.
    let active_filters: Vec<FilterChip> = active
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let remaining: Vec<&str> = active_strs
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, s)| *s)
                .collect();
            FilterChip { name: name.clone(), remove_href: filter_url(&remaining) }
        })
        .collect();

    render(IndexPage { username: user.username, clips, active_filters, active_wikis })
}

async fn load_clips(pool: &SqlitePool, active: &[&str]) -> AppResult<Vec<ClipRow>> {
    // AND filter: a clip must have every label in `active`. Done with
    // GROUP BY HAVING COUNT(DISTINCT label_id) = N. The IN-list is
    // built with ? placeholders since sqlx doesn't expand Vec<_>.
    type Row = (i64, String, String, String, Option<String>, String, Option<String>, Option<f64>);
    let rows: Vec<Row> = if active.is_empty() {
        sqlx::query_as(
            "SELECT c.id, c.name, c.original_filename, c.uploaded_at,
                    c.recording_date, u.username, c.peaks, c.duration_seconds
             FROM clips c JOIN users u ON u.id = c.uploaded_by
             ORDER BY c.uploaded_at DESC",
        )
        .fetch_all(pool)
        .await?
    } else {
        let placeholders = vec!["?"; active.len()].join(", ");
        let sql = format!(
            "SELECT c.id, c.name, c.original_filename, c.uploaded_at,
                    c.recording_date, u.username, c.peaks, c.duration_seconds
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
    for (id, name, original_filename, uploaded_at, recording_date, uploader, peaks, duration_seconds)
        in rows
    {
        let label_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT l.name FROM labels l
             JOIN clip_labels cl ON cl.label_id = l.id
             WHERE cl.clip_id = ? ORDER BY l.name COLLATE NOCASE",
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        // Per-label link: clicking adds it to the current filter, or
        // is a no-op if already active.
        let labels: Vec<LabelLink> = label_rows
            .into_iter()
            .map(|(lname,)| {
                let already_active = active.iter().any(|a| a.eq_ignore_ascii_case(&lname));
                let href = if already_active {
                    filter_url(active)
                } else {
                    let mut combined: Vec<&str> = active.to_vec();
                    combined.push(&lname);
                    filter_url(&combined)
                };
                LabelLink { name: lname, href }
            })
            .collect();

        clips.push(ClipRow {
            id,
            name,
            original_filename,
            uploaded_at: crate::datefmt::iso_date_from_rfc3339(&uploaded_at),
            recording_date,
            uploader,
            labels,
            peaks,
            duration_seconds,
        });
    }
    Ok(clips)
}

#[derive(Debug)]
pub struct ClipLabel {
    pub id: i64,
    pub name: String,
    pub filter_href: String,
}

#[derive(Template)]
#[template(path = "clip.html")]
struct ClipPage {
    username: String,
    clip_id: i64,
    name: String,
    original_filename: String,
    content_type: String,
    uploader: String,
    uploaded_at: String,
    recording_date: Option<String>,
    labels: Vec<ClipLabel>,
    peaks: Option<String>,
    duration_seconds: Option<f64>,
}

pub async fn detail(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    type DetailRow =
        (i64, String, String, String, String, Option<String>, String, Option<String>, Option<f64>);
    let row: Option<DetailRow> = sqlx::query_as(
        "SELECT c.id, c.name, c.original_filename, c.content_type,
                c.uploaded_at, c.recording_date, u.username, c.peaks, c.duration_seconds
         FROM clips c JOIN users u ON u.id = c.uploaded_by
         WHERE c.id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;

    let (
        clip_id,
        name,
        original_filename,
        content_type,
        uploaded_at,
        recording_date,
        uploader,
        peaks,
        duration_seconds,
    ) = row.ok_or(AppError::NotFound)?;

    let labels = load_labels(&state, clip_id).await?;

    render(ClipPage {
        username: user.username,
        clip_id,
        name,
        original_filename,
        content_type,
        uploader,
        uploaded_at: crate::datefmt::iso_date_from_rfc3339(&uploaded_at),
        recording_date,
        labels,
        peaks,
        duration_seconds,
    })
}

pub(super) async fn load_labels(state: &AppState, clip_id: i64) -> AppResult<Vec<ClipLabel>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT l.id, l.name FROM labels l
         JOIN clip_labels cl ON cl.label_id = l.id
         WHERE cl.clip_id = ? ORDER BY l.name COLLATE NOCASE",
    )
    .bind(clip_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| {
            let filter_href = filter_url(&[name.as_str()]);
            ClipLabel { id, name, filter_href }
        })
        .collect())
}

#[derive(Debug, Deserialize, Default)]
pub struct AudioQuery {
    /// Any value (even empty) flips on Content-Disposition: attachment.
    /// Kept as Option<String> rather than bool because `?download=1`
    /// doesn't match serde's strict "true"/"false" bool deserializer.
    pub download: Option<String>,
}

pub async fn audio(
    State(state): State<AppState>,
    _user: CurrentUser,
    AxumPath(id): AxumPath<i64>,
    Query(q): Query<AudioQuery>,
) -> AppResult<Response> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT storage_filename, content_type, original_filename FROM clips WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (storage, content_type, original_filename) = row.ok_or(AppError::NotFound)?;

    let path = state.config.audio_dir.join(&storage);
    // Stream from disk so we don't pull the whole file into RAM. Browsers
    // need Content-Length up front to show download progress and to
    // support range requests if/when we add them later.
    let file = tokio::fs::File::open(&path).await?;
    let len = file.metadata().await?.len();
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    // content_type is what we wrote at upload time — one of our
    // allow-listed audio MIME types, never the uploader's claim. Pair
    // with nosniff so a curious browser doesn't reinterpret it as HTML.
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    if q.download.is_some() {
        // RFC 6266 / 5987: ASCII filename + UTF-8 filename* so browsers
        // pick the right one for non-ASCII characters in the original
        // upload name.
        let ascii = sanitise_for_ascii_filename(&original_filename);
        let utf8 = percent_encode_filename(&original_filename);
        let disp = format!(
            "attachment; filename=\"{ascii}\"; filename*=UTF-8''{utf8}"
        );
        if let Ok(v) = HeaderValue::from_str(&disp) {
            headers.insert(header::CONTENT_DISPOSITION, v);
        }
    }
    Ok((headers, body).into_response())
}

/// Replace anything that isn't a plain ASCII filename-safe character
/// with '_'. Used for the legacy `filename=` parameter; the modern
/// `filename*=UTF-8''…` parameter carries the real name.
fn sanitise_for_ascii_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect()
}

fn percent_encode_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == '~' {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Allow-list mapping from file extension to the MIME type we'll serve
/// the clip with. We never trust the uploader's claimed Content-Type:
/// otherwise someone could upload a `.html` and get same-origin XSS
/// against every other authenticated user.
fn audio_content_type(ext: &str) -> Option<&'static str> {
    match ext {
        "mp3" => Some("audio/mpeg"),
        "m4a" | "mp4" => Some("audio/mp4"),
        "wav" => Some("audio/wav"),
        "flac" => Some("audio/flac"),
        "ogg" | "oga" | "opus" => Some("audio/ogg"),
        "aac" => Some("audio/aac"),
        "webm" => Some("audio/webm"),
        _ => None,
    }
}

pub async fn upload(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    mut multipart: Multipart,
) -> AppResult<Response> {
    // Each selected or dropped file arrives as its own "audio" multipart
    // field. We stream each straight to disk rather than buffering: a
    // batch of 10 MB files would otherwise cost that much resident memory
    // per concurrent request.
    let mut uploaded = 0usize;
    while let Some(mut field) = multipart.next_field().await? {
        if field.name() != Some("audio") {
            continue;
        }
        // An empty file input still posts an "audio" part with no
        // filename; skip it so submitting with nothing chosen (or a
        // stray empty field) isn't a hard error.
        let original_filename = match field.file_name() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => continue,
        };

        let ext = Path::new(&original_filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let content_type = audio_content_type(&ext).ok_or_else(|| {
            AppError::BadRequest(format!(
                "unsupported audio format: .{ext} (allowed: mp3, m4a, wav, flac, ogg, opus, aac, webm)"
            ))
        })?;

        let storage_filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
        let path = state.config.audio_dir.join(&storage_filename);

        // Stream field → file with a hard byte cap. axum's
        // DefaultBodyLimit on this route catches the request before we
        // get here for the common case; the in-loop check is defence in
        // depth and lets us return a specific message.
        let size_bytes = match stream_field_to_file(&mut field, &path).await {
            Ok(n) => n,
            Err(e) => {
                // Don't leave half-written turds in the audio dir.
                let _ = tokio::fs::remove_file(&path).await;
                return Err(e);
            }
        };

        // Metadata extraction and waveform decoding are sync, CPU/IO
        // bound work on the just-written file; run both off the runtime
        // thread in one hop.
        let path_for_probe = path.clone();
        let (recording_date, waveform) = tokio::task::spawn_blocking(move || {
            (
                extract_recording_date(&path_for_probe),
                crate::audio::compute(&path_for_probe),
            )
        })
        .await
        .unwrap_or((None, None));
        let (peaks_json, duration_seconds) = match waveform {
            Some(w) => (Some(crate::audio::peaks_to_json(&w.peaks)), Some(w.duration_seconds)),
            None => (None, None),
        };

        let display_name_stem = Path::new(&original_filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&original_filename)
            .to_string();

        // Pick a name and insert. If two uploads race on the same name,
        // the loser sees a UNIQUE violation on the index — catch that
        // and try the next suffix instead of bubbling a 500.
        if let Err(e) = insert_with_unique_name(
            &state.pool,
            &display_name_stem,
            &storage_filename,
            &original_filename,
            content_type,
            size_bytes as i64,
            user.id,
            recording_date.as_deref(),
            peaks_json.as_deref(),
            duration_seconds,
        )
        .await
        {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(e);
        }
        uploaded += 1;
    }

    if uploaded == 0 {
        return Err(AppError::BadRequest("at least one audio file is required".into()));
    }
    Ok(Redirect::to("/").into_response())
}

/// Stream a multipart field to disk, enforcing MAX_UPLOAD_BYTES as we
/// go. Returns the total bytes written on success.
async fn stream_field_to_file(
    field: &mut axum::extract::multipart::Field<'_>,
    path: &Path,
) -> AppResult<usize> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut total: usize = 0;
    while let Some(chunk) = field.chunk().await? {
        total = total.saturating_add(chunk.len());
        if total > MAX_UPLOAD_BYTES {
            return Err(AppError::BadRequest(format!(
                "upload exceeds {MAX_UPLOAD_BYTES}-byte limit"
            )));
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(total)
}

/// Try a sequence of names ("base", "base (2)", "base (3)", …) until
/// the INSERT succeeds. Each loop iteration both picks a candidate and
/// inserts; a UNIQUE-violation just means another upload claimed that
/// name between our SELECT and INSERT, so we try the next one.
#[allow(clippy::too_many_arguments)]
async fn insert_with_unique_name(
    pool: &SqlitePool,
    base: &str,
    storage_filename: &str,
    original_filename: &str,
    content_type: &str,
    size_bytes: i64,
    uploader_id: i64,
    recording_date: Option<&str>,
    peaks: Option<&str>,
    duration_seconds: Option<f64>,
) -> AppResult<()> {
    let base = base.trim();
    let base = if base.is_empty() { "clip" } else { base };
    let mut n: u32 = 1;
    loop {
        let candidate = if n == 1 { base.to_string() } else { format!("{base} ({n})") };
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM clips WHERE name = ? COLLATE NOCASE")
                .bind(&candidate)
                .fetch_optional(pool)
                .await?;
        if exists.is_some() {
            n += 1;
            if n > 1000 {
                return Err(AppError::Other(anyhow::anyhow!(
                    "couldn't find a unique name after 1000 tries"
                )));
            }
            continue;
        }

        let result = sqlx::query(
            "INSERT INTO clips (name, storage_filename, original_filename, content_type,
                                size_bytes, uploaded_by, recording_date, peaks, duration_seconds)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&candidate)
        .bind(storage_filename)
        .bind(original_filename)
        .bind(content_type)
        .bind(size_bytes)
        .bind(uploader_id)
        .bind(recording_date)
        .bind(peaks)
        .bind(duration_seconds)
        .execute(pool)
        .await;

        match result {
            Ok(_) => return Ok(()),
            Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
                // Either the name or the storage_filename collided.
                // storage_filename is a UUID v4 so that's astronomically
                // unlikely; assume it's the name and try the next.
                n += 1;
                if n > 1000 {
                    return Err(AppError::Other(anyhow::anyhow!(
                        "couldn't find a unique name after 1000 tries"
                    )));
                }
                continue;
            }
            Err(e) => return Err(AppError::Sqlx(e)),
        }
    }
}

pub(crate) fn extract_recording_date(path: &PathBuf) -> Option<String> {
    // In priority order:
    //   1. an explicit recording-date tag (older iPhone Voice Memos),
    //   2. the Voice Memos `moov/udta/date` box — the true recording
    //      time, and it survives editing,
    //   3. the mvhd creation time, as a rough last resort.
    // mvhd is least trustworthy because cropping/editing re-exports the
    // file and stamps mvhd with the *export* time, not the recording.
    // The tag read (1) covers older iPhone Voice Memos, which write a
    // `©day` atom; ID3-tagged mp3s etc. land here too.
    crate::audio::recording_date_tag(path)
        .or_else(|| recording_date_from_udta_date(path))
        .or_else(|| recording_date_from_mvhd(path))
}

/// Newer Voice Memos (iPad 26.x) drop the `©day` tag but store the
/// recording time in a `moov/udta/date` box whose payload is a bare
/// RFC 3339 string like `2026-05-22T19:40:08Z`. Crucially this survives
/// editing — cropping a memo rewrites the mvhd creation time to the
/// export moment but leaves this box intact — so it's the date we
/// actually want. MP4 is a flat sequence of `[u32 size][4-byte type]
/// [payload]` boxes, and `udta` here is a direct child of `moov`
/// (the trak-level `udta` holding the transcript is nested inside `trak`,
/// so scanning moov's direct children doesn't reach it).
fn recording_date_from_udta_date(path: &PathBuf) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();

    let (moov_start, moov_end) = find_box(&mut file, b"moov", 0, file_len)?;
    let (udta_start, udta_end) = find_box(&mut file, b"udta", moov_start, moov_end)?;
    let (date_start, date_end) = find_box(&mut file, b"date", udta_start, udta_end)?;

    // The `date` box has no version/flags — its payload is the string
    // directly. Bound the length so a malformed box can't allocate wildly.
    let len = date_end.checked_sub(date_start)?;
    if len == 0 || len > 64 {
        return None;
    }
    file.seek(SeekFrom::Start(date_start)).ok()?;
    let mut buf = vec![0u8; len as usize];
    file.read_exact(&mut buf).ok()?;

    Some(crate::datefmt::iso_date_from_rfc3339(std::str::from_utf8(&buf).ok()?.trim()))
}

/// Last-resort recording date: the MP4 movie header's creation time,
/// read by hand by descending `moov` → `mvhd`. We touch only box headers
/// and the ~20-byte mvhd payload, so the sample tables that trip up
/// general-purpose parsers are never read. This is the *export* time for
/// any file that's been edited, so it's only used when neither a tag nor
/// a `udta/date` box is present.
fn recording_date_from_mvhd(path: &PathBuf) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    use time::OffsetDateTime;

    // mvhd creation_time counts seconds from 1904-01-01 UTC.
    const MP4_EPOCH_TO_UNIX: i64 = 2_082_844_800;

    let mut file = std::fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();

    let (moov_start, moov_end) = find_box(&mut file, b"moov", 0, file_len)?;
    let (mvhd_start, _) = find_box(&mut file, b"mvhd", moov_start, moov_end)?;

    // mvhd payload: 1 version byte + 3 flag bytes, then creation_time
    // (u64 for version 1, u32 for version 0).
    file.seek(SeekFrom::Start(mvhd_start)).ok()?;
    let mut head = [0u8; 4];
    file.read_exact(&mut head).ok()?;
    let creation = if head[0] == 1 {
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).ok()?;
        u64::from_be_bytes(buf)
    } else {
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf).ok()?;
        u32::from_be_bytes(buf) as u64
    };

    // 0 means the muxer left it unset; skip rather than report 1904.
    if creation == 0 {
        return None;
    }
    let unix = (creation as i64).checked_sub(MP4_EPOCH_TO_UNIX)?;
    crate::datefmt::iso_date(OffsetDateTime::from_unix_timestamp(unix).ok()?)
}

/// Scan the boxes in `[start, end)` and return the `(payload_start,
/// payload_end)` of the first one whose type matches `target`. Returns
/// `None` on a malformed/overrunning box rather than looping.
fn find_box(
    file: &mut std::fs::File,
    target: &[u8; 4],
    start: u64,
    end: u64,
) -> Option<(u64, u64)> {
    use std::io::{Read, Seek, SeekFrom};

    let mut pos = start;
    while pos + 8 <= end {
        file.seek(SeekFrom::Start(pos)).ok()?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);

        let (header_len, box_size) = match size32 {
            0 => (8u64, end - pos), // extends to the end of the parent
            1 => {
                let mut ext = [0u8; 8];
                file.read_exact(&mut ext).ok()?;
                (16u64, u64::from_be_bytes(ext))
            }
            n => (8u64, u64::from(n)),
        };

        // Reject a box that's smaller than its header or runs past the
        // parent — either is corrupt and we'd otherwise loop or misread.
        if box_size < header_len || pos.checked_add(box_size)? > end {
            return None;
        }

        if &header[4..8] == target {
            return Some((pos + header_len, pos + box_size));
        }
        pos += box_size;
    }
    None
}

#[derive(Deserialize)]
pub struct RenameForm {
    name: String,
}

#[derive(Template)]
#[template(path = "_clip_header.html")]
struct ClipHeader {
    clip_id: i64,
    name: String,
}

#[derive(Template)]
#[template(path = "_clip_rename_form.html")]
struct ClipRenameForm {
    clip_id: i64,
    name: String,
}

async fn fetch_clip_name(state: &AppState, id: i64) -> AppResult<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT name FROM clips WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    Ok(row.ok_or(AppError::NotFound)?.0)
}

/// HTMX: return the rename form for the clip header.
pub async fn rename_form(
    State(state): State<AppState>,
    CurrentUserApi(_user): CurrentUserApi,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = fetch_clip_name(&state, id).await?;
    render(ClipRenameForm { clip_id: id, name })
}

/// HTMX: return the read-only header (used by the form's Cancel button).
pub async fn rename_display(
    State(state): State<AppState>,
    CurrentUserApi(_user): CurrentUserApi,
    AxumPath(id): AxumPath<i64>,
) -> AppResult<Response> {
    let name = fetch_clip_name(&state, id).await?;
    render(ClipHeader { clip_id: id, name })
}

pub async fn rename(
    State(state): State<AppState>,
    CurrentUserApi(_user): CurrentUserApi,
    AxumPath(id): AxumPath<i64>,
    Form(form): Form<RenameForm>,
) -> AppResult<Response> {
    let new_name = form.name.trim();
    if new_name.is_empty() {
        return Err(AppError::BadRequest("name can't be empty".into()));
    }

    // Let the UNIQUE index be the source of truth — no SELECT-then-
    // UPDATE TOCTOU. Map a unique-violation back to a clean 409 for
    // HTMX to display.
    let result = sqlx::query("UPDATE clips SET name = ? WHERE id = ?")
        .bind(new_name)
        .bind(id)
        .execute(&state.pool)
        .await;

    match result {
        Ok(r) if r.rows_affected() == 0 => Err(AppError::NotFound),
        Ok(_) => render(ClipHeader { clip_id: id, name: new_name.to_string() }),
        Err(sqlx::Error::Database(d)) if d.is_unique_violation() => Ok((
            StatusCode::CONFLICT,
            format!("A clip named “{new_name}” already exists."),
        )
            .into_response()),
        Err(e) => Err(AppError::Sqlx(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // guitar-clip.m4a is an iPad Voice Memos recording with no `©day`
    // tag. It was cropped after recording, so its mvhd creation time is
    // the export date (2026-05-29T19:46:11Z) while the true recording
    // time lives in the udta/date box (2026-05-22T19:40:08Z). We must
    // report the recording date, not the edit date.
    #[test]
    fn extracts_recording_date_from_udta_date() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/guitar-clip.m4a");
        assert_eq!(extract_recording_date(&path).as_deref(), Some("2026-05-22"));
    }
}
