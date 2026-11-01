//! Playback copies: a second file next to a clip's upload, streamed in
//! its place, for formats some client can't play as stored.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::audio_format::{self, AudioFormat};

pub const PLAYBACK_CONTENT_TYPE: &str = "audio/mp4";

/// Remux `storage_filename` into an MP4 beside it and return the new
/// file's name. A failure is logged and leaves no file: the clip then
/// plays as it always did, everywhere but on iPhones.
pub fn make_copy(audio_dir: &Path, storage_filename: &str) -> Option<String> {
    let stem = Path::new(storage_filename).file_stem()?.to_str()?;
    let name = format!("{stem}.playback.m4a");
    let dst = audio_dir.join(&name);
    match audio_format::remux_adts_to_mp4(&audio_dir.join(storage_filename), &dst) {
        Ok(()) => Some(name),
        Err(e) => {
            tracing::warn!(error = ?e, storage_filename, "could not make a playback copy");
            let _ = std::fs::remove_file(&dst);
            None
        }
    }
}

/// Detect the format of every clip stored before detection existed, and
/// make the playback copies they need. Clips already seen are skipped, so
/// running it on every start costs one query.
pub async fn backfill(pool: &SqlitePool, audio_dir: &Path) -> anyhow::Result<()> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, storage_filename FROM clips WHERE format IS NULL")
            .fetch_all(pool)
            .await?;
    for (id, storage_filename) in rows {
        let dir: PathBuf = audio_dir.to_owned();
        let file = storage_filename.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            audio_format::detect(&dir.join(&file)).map(|detected| {
                detected.map(|format| {
                    let copy = format
                        .needs_playback_copy()
                        .then(|| make_copy(&dir, &file))
                        .flatten();
                    (format, copy)
                })
            })
        })
        .await?;
        match outcome {
            Ok(Ok((format, copy))) => record(pool, id, format, copy.as_deref()).await?,
            Ok(Err(unsupported)) => tracing::warn!(
                clip_id = id,
                ?unsupported,
                "stored clip is in a format uploads no longer accept"
            ),
            Err(e) => tracing::warn!(clip_id = id, error = ?e, "could not read stored clip"),
        }
    }
    Ok(())
}

async fn record(
    pool: &SqlitePool,
    id: i64,
    format: AudioFormat,
    playback_filename: Option<&str>,
) -> sqlx::Result<()> {
    if playback_filename.is_some() {
        tracing::info!(clip_id = id, format = format.as_str(), "made playback copy");
    }
    sqlx::query(
        "UPDATE clips
         SET format = ?, content_type = ?, playback_filename = ?, playback_content_type = ?
         WHERE id = ?",
    )
    .bind(format.as_str())
    .bind(format.content_type())
    .bind(playback_filename)
    .bind(playback_filename.map(|_| PLAYBACK_CONTENT_TYPE))
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
