use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::{auth, config::Config, db, web};

#[derive(Parser, Debug)]
#[command(name = "iggybilly", about = "Self-hosted audio clip sharing")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the HTTP server.
    Serve,
    /// Create a new user; prints a randomly generated password to stdout.
    CreateUser {
        username: String,
        #[arg(long)]
        admin: bool,
    },
    /// Reset a user's password to a new random value, printed to stdout.
    ResetPassword { username: String },
    /// Fill in recording_date for clips that don't have one yet, by
    /// re-reading each stored audio file's metadata. Useful after the
    /// extraction logic learns to read a new source (e.g. the mvhd
    /// fallback) or for clips uploaded before it existed.
    BackfillDates {
        /// Show what would change without writing to the database.
        #[arg(long)]
        dry_run: bool,
    },
    /// Compute and store waveform peaks for clips that don't have them
    /// yet, so the UI can draw waveforms without downloading the audio.
    /// Run this for clips uploaded before waveforms were precomputed.
    BackfillWaveforms {
        /// Show what would change without writing to the database.
        #[arg(long)]
        dry_run: bool,
    },
}

pub async fn run(cli: Cli) -> Result<()> {
    let config = Config::from_env();
    tokio::fs::create_dir_all(&config.data_dir).await.ok();
    tokio::fs::create_dir_all(&config.audio_dir).await.ok();
    let pool = db::connect(&config.db_path).await?;

    match cli.command {
        Command::Serve => web::serve(config, pool).await,
        Command::CreateUser { username, admin } => {
            let password = auth::random_password();
            let hash = auth::hash_password(&password)?;
            sqlx::query("INSERT INTO users (username, password_hash, is_admin) VALUES (?, ?, ?)")
                .bind(&username)
                .bind(&hash)
                .bind(admin as i64)
                .execute(&pool)
                .await
                .with_context(|| format!("creating user {username}"))?;
            println!("Created user {username}");
            println!("Password: {password}");
            Ok(())
        }
        Command::ResetPassword { username } => {
            let password = auth::random_password();
            let hash = auth::hash_password(&password)?;
            let result = sqlx::query("UPDATE users SET password_hash = ? WHERE username = ?")
                .bind(&hash)
                .bind(&username)
                .execute(&pool)
                .await?;
            if result.rows_affected() == 0 {
                anyhow::bail!("no such user: {username}");
            }
            println!("Password reset for {username}");
            println!("New password: {password}");
            Ok(())
        }
        Command::BackfillDates { dry_run } => {
            backfill_dates(&config, &pool, dry_run).await
        }
        Command::BackfillWaveforms { dry_run } => {
            backfill_waveforms(&config, &pool, dry_run).await
        }
    }
}

/// Re-derive `recording_date` for every clip that's currently missing
/// one. Only touches NULL rows, so it's safe to re-run and never
/// overwrites a date already on file.
async fn backfill_dates(
    config: &Config,
    pool: &sqlx::SqlitePool,
    dry_run: bool,
) -> Result<()> {
    let clips: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, storage_filename FROM clips WHERE recording_date IS NULL")
            .fetch_all(pool)
            .await
            .context("listing clips without a recording date")?;

    if clips.is_empty() {
        println!("No clips are missing a recording date.");
        return Ok(());
    }

    let (mut updated, mut not_found, mut no_date) = (0u32, 0u32, 0u32);
    for (id, storage_filename) in &clips {
        let path = config.audio_dir.join(storage_filename);
        if !path.exists() {
            eprintln!("clip {id}: audio file {storage_filename} is missing, skipping");
            not_found += 1;
            continue;
        }

        match crate::handlers::clips::extract_recording_date(&path) {
            Some(date) => {
                if dry_run {
                    println!("clip {id}: would set recording_date = {date}");
                } else {
                    sqlx::query("UPDATE clips SET recording_date = ? WHERE id = ?")
                        .bind(&date)
                        .bind(id)
                        .execute(pool)
                        .await
                        .with_context(|| format!("updating clip {id}"))?;
                    println!("clip {id}: set recording_date = {date}");
                }
                updated += 1;
            }
            None => {
                println!("clip {id}: no recording date found in metadata");
                no_date += 1;
            }
        }
    }

    let verb = if dry_run { "would update" } else { "updated" };
    println!(
        "Done: {verb} {updated}, no date in {no_date}, {not_found} file(s) missing \
         (of {} checked).",
        clips.len()
    );
    Ok(())
}

/// Compute and store waveform peaks for every clip that's currently
/// missing them. Only touches NULL rows, so it's safe to re-run.
async fn backfill_waveforms(
    config: &Config,
    pool: &sqlx::SqlitePool,
    dry_run: bool,
) -> Result<()> {
    let clips: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, storage_filename FROM clips WHERE peaks IS NULL")
            .fetch_all(pool)
            .await
            .context("listing clips without a waveform")?;

    if clips.is_empty() {
        println!("No clips are missing a waveform.");
        return Ok(());
    }

    let (mut updated, mut not_found, mut undecodable) = (0u32, 0u32, 0u32);
    for (id, storage_filename) in &clips {
        let path = config.audio_dir.join(storage_filename);
        if !path.exists() {
            eprintln!("clip {id}: audio file {storage_filename} is missing, skipping");
            not_found += 1;
            continue;
        }

        match crate::waveform::compute(&path) {
            Some(wf) => {
                let peaks = crate::waveform::peaks_to_json(&wf.peaks);
                if dry_run {
                    println!(
                        "clip {id}: would set waveform ({} peaks, {:.1}s)",
                        wf.peaks.len(),
                        wf.duration_seconds
                    );
                } else {
                    sqlx::query("UPDATE clips SET peaks = ?, duration_seconds = ? WHERE id = ?")
                        .bind(&peaks)
                        .bind(wf.duration_seconds)
                        .bind(id)
                        .execute(pool)
                        .await
                        .with_context(|| format!("updating clip {id}"))?;
                    println!(
                        "clip {id}: set waveform ({} peaks, {:.1}s)",
                        wf.peaks.len(),
                        wf.duration_seconds
                    );
                }
                updated += 1;
            }
            None => {
                println!("clip {id}: could not decode audio for a waveform");
                undecodable += 1;
            }
        }
    }

    let verb = if dry_run { "would update" } else { "updated" };
    println!(
        "Done: {verb} {updated}, undecodable {undecodable}, {not_found} file(s) missing \
         (of {} checked).",
        clips.len()
    );
    Ok(())
}
