//! Playback copies: a second file next to a clip's upload, streamed in
//! its place, for formats some client can't play as stored. ADTS gets
//! remuxed into MP4 (003); MIDI gets rendered to MP3 (004).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::audio_format::{self, AudioFormat};
use crate::midi_render::{self, Renderer};

/// A playback copy on disk.
pub struct Copy {
    pub filename: String,
    pub content_type: &'static str,
}

/// What came of making the copy a clip's format calls for.
#[derive(Default)]
pub struct Prepared {
    pub copy: Option<Copy>,
    /// For MIDI: the renderer that looked at it, whether or not it made
    /// a copy. None when there was no renderer to look.
    pub render_version: Option<String>,
}

/// Make the playback copy `format` needs, if any. Failures are logged and
/// leave no file: the clip then plays as stored, or for MIDI not at all.
pub fn prepare(
    format: AudioFormat,
    audio_dir: &Path,
    storage_filename: &str,
    renderer: Option<&Renderer>,
) -> Prepared {
    let Some(stem) = Path::new(storage_filename)
        .file_stem()
        .and_then(|s| s.to_str())
    else {
        return Prepared::default();
    };
    let src = audio_dir.join(storage_filename);
    match format {
        AudioFormat::Adts => {
            let name = format!("{stem}.playback.m4a");
            let dst = audio_dir.join(&name);
            match audio_format::remux_adts_to_mp4(&src, &dst) {
                Ok(()) => Prepared {
                    copy: Some(Copy {
                        filename: name,
                        content_type: "audio/mp4",
                    }),
                    render_version: None,
                },
                Err(e) => {
                    tracing::warn!(error = ?e, storage_filename, "could not remux ADTS");
                    let _ = std::fs::remove_file(&dst);
                    Prepared::default()
                }
            }
        }
        AudioFormat::Midi => {
            let Some(renderer) = renderer else {
                return Prepared::default();
            };
            // The version is in the name, so a re-render is a new file
            // and a new audio URL, which is how app caches notice it.
            let name = format!("{stem}.{}.mp3", renderer.version());
            let dst = audio_dir.join(&name);
            let copy = match renderer.render(&src, &dst) {
                Ok(Ok(())) => Some(Copy {
                    filename: name,
                    content_type: midi_render::CONTENT_TYPE,
                }),
                Ok(Err(declined)) => {
                    tracing::info!(?declined, storage_filename, "MIDI file not rendered");
                    None
                }
                Err(e) => {
                    tracing::warn!(error = ?e, storage_filename, "could not render MIDI file");
                    let _ = std::fs::remove_file(&dst);
                    None
                }
            };
            Prepared {
                copy,
                render_version: Some(renderer.version().to_owned()),
            }
        }
        AudioFormat::Mp4Aac | AudioFormat::Mp3 => Prepared::default(),
    }
}

/// Bring stored clips up to date, in two passes. First, detect the format
/// of every clip stored before detection existed and make the copies they
/// need. Then render every MIDI clip that the running renderer hasn't
/// seen: uploaded before there was one, or rendered by another version.
/// Clips already up to date are skipped, so running it on every start
/// costs two queries.
pub async fn backfill(
    pool: &SqlitePool,
    audio_dir: &Path,
    renderer: Option<Arc<Renderer>>,
) -> anyhow::Result<()> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, storage_filename FROM clips WHERE format IS NULL")
            .fetch_all(pool)
            .await?;
    for (id, storage_filename) in rows {
        let dir: PathBuf = audio_dir.to_owned();
        let file = storage_filename.clone();
        let renderer = renderer.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            audio_format::detect(&dir.join(&file)).map(|detected| {
                detected.map(|format| (format, prepare(format, &dir, &file, renderer.as_deref())))
            })
        })
        .await?;
        match outcome {
            Ok(Ok((format, prepared))) => record_format(pool, id, format, &prepared).await?,
            Ok(Err(unsupported)) => tracing::warn!(
                clip_id = id,
                ?unsupported,
                "stored clip is in a format uploads no longer accept"
            ),
            Err(e) => tracing::warn!(clip_id = id, error = ?e, "could not read stored clip"),
        }
    }

    let Some(renderer) = renderer else {
        return Ok(());
    };
    let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, storage_filename, playback_filename FROM clips
         WHERE format = 'midi' AND (render_version IS NULL OR render_version != ?)",
    )
    .bind(renderer.version())
    .fetch_all(pool)
    .await?;
    for (id, storage_filename, old_copy) in rows {
        let dir: PathBuf = audio_dir.to_owned();
        let renderer = renderer.clone();
        let (prepared, waveform) = tokio::task::spawn_blocking(move || {
            let prepared = prepare(AudioFormat::Midi, &dir, &storage_filename, Some(&renderer));
            let waveform = prepared
                .copy
                .as_ref()
                .and_then(|c| crate::audio::compute(&dir.join(&c.filename)));
            (prepared, waveform)
        })
        .await?;
        if prepared.copy.is_some() {
            tracing::info!(clip_id = id, "rendered MIDI clip");
        }
        sqlx::query(
            "UPDATE clips
             SET playback_filename = ?, playback_content_type = ?, render_version = ?,
                 peaks = ?, duration_seconds = ?
             WHERE id = ?",
        )
        .bind(prepared.copy.as_ref().map(|c| c.filename.as_str()))
        .bind(prepared.copy.as_ref().map(|c| c.content_type))
        .bind(prepared.render_version.as_deref())
        .bind(
            waveform
                .as_ref()
                .map(|w| crate::audio::peaks_to_json(&w.peaks)),
        )
        .bind(waveform.as_ref().map(|w| w.duration_seconds))
        .bind(id)
        .execute(pool)
        .await?;
        if let Some(old) = old_copy {
            if prepared.copy.as_ref().map(|c| &c.filename) != Some(&old) {
                let _ = tokio::fs::remove_file(audio_dir.join(old)).await;
            }
        }
    }
    Ok(())
}

async fn record_format(
    pool: &SqlitePool,
    id: i64,
    format: AudioFormat,
    prepared: &Prepared,
) -> sqlx::Result<()> {
    if prepared.copy.is_some() {
        tracing::info!(clip_id = id, format = format.as_str(), "made playback copy");
    }
    sqlx::query(
        "UPDATE clips
         SET format = ?, content_type = ?, playback_filename = ?, playback_content_type = ?,
             render_version = ?
         WHERE id = ?",
    )
    .bind(format.as_str())
    .bind(format.content_type())
    .bind(prepared.copy.as_ref().map(|c| c.filename.as_str()))
    .bind(prepared.copy.as_ref().map(|c| c.content_type))
    .bind(prepared.render_version.as_deref())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
