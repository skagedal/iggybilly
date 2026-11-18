use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::{auth, config::Config, db, queries, web};

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
    /// Set a user's password to a given value. For local development: the
    /// password is visible in shell history and in `ps`, so prefer
    /// reset-password anywhere that matters.
    SetPasswordExplicit { username: String, password: String },
    /// List all users.
    ListUsers,
    /// Create a band. The slug is lower-kebab-case and is what appears
    /// in every URL for the band's content.
    CreateBand { slug: String },
    /// Rename a band. Every link to the old slug stops working.
    RenameBand { old_slug: String, new_slug: String },
    /// List bands, with how many members and clips each has.
    ListBands,
    /// Put a user in a band.
    AssignToBand { username: String, band: String },
    /// Take a user out of a band. Their clips stay with the band.
    RemoveFromBand { username: String, band: String },
    /// Point a band's Discord posts at its own channel. With no URL, the
    /// band goes back to the instance-wide webhook.
    SetBandWebhook { band: String, url: Option<String> },
    /// Backfill recording_date for clips that don't have one yet.
    BackfillDates {
        /// Show what would change without writing to the database.
        #[arg(long)]
        dry_run: bool,
    },
    /// Backfill waveform peaks for clips that don't have them yet.
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
            set_password(&pool, &username, &password).await?;
            println!("Password reset for {username}");
            println!("New password: {password}");
            Ok(())
        }
        Command::SetPasswordExplicit { username, password } => {
            set_password(&pool, &username, &password).await?;
            println!("Password set for {username}");
            Ok(())
        }
        Command::ListUsers => list_users(&pool).await,
        Command::CreateBand { slug } => create_band(&pool, &slug).await,
        Command::RenameBand { old_slug, new_slug } => {
            rename_band(&pool, &old_slug, &new_slug).await
        }
        Command::ListBands => list_bands(&pool).await,
        Command::AssignToBand { username, band } => {
            let (user_id, band_row) = resolve_user_and_band(&pool, &username, &band).await?;
            queries::bands::add_member(&pool, band_row.id, user_id).await?;
            println!("{username} is now in {}", band_row.slug);
            Ok(())
        }
        Command::RemoveFromBand { username, band } => {
            let (user_id, band_row) = resolve_user_and_band(&pool, &username, &band).await?;
            if queries::bands::remove_member(&pool, band_row.id, user_id).await? {
                println!("{username} is no longer in {}", band_row.slug);
                println!("Their clips stay with the band.");
            } else {
                println!("{username} was not in {}", band_row.slug);
            }
            Ok(())
        }
        Command::SetBandWebhook { band, url } => {
            let band_row = find_band(&pool, &band).await?;
            queries::bands::set_webhook(&pool, band_row.id, url.as_deref()).await?;
            match url {
                Some(_) => println!("{} now posts to its own Discord channel", band_row.slug),
                None => println!(
                    "{} now posts to the instance-wide webhook, if one is set",
                    band_row.slug
                ),
            }
            Ok(())
        }
        Command::BackfillDates { dry_run } => backfill_dates(&config, &pool, dry_run).await,
        Command::BackfillWaveforms { dry_run } => backfill_waveforms(&config, &pool, dry_run).await,
    }
}

/// List every user, one per line, with their admin flag and creation time.
/// Store a new password for a user, and revoke their app tokens.
///
/// The tokens go because a password change is how access is cut off, and
/// a signed-in phone that kept working would defeat that. This matches
/// what the web and the API do.
async fn set_password(pool: &sqlx::SqlitePool, username: &str, password: &str) -> Result<()> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await?;
    let (user_id,) = row.with_context(|| format!("no such user: {username}"))?;

    let hash = auth::hash_password(password)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&hash)
        .bind(user_id)
        .execute(pool)
        .await?;
    crate::tokens::revoke_all(pool, user_id).await?;
    Ok(())
}

async fn list_users(pool: &sqlx::SqlitePool) -> Result<()> {
    let users: Vec<(i64, String, i64, String)> = sqlx::query_as(
        "SELECT id, username, is_admin, created_at FROM users ORDER BY username COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .context("listing users")?;

    if users.is_empty() {
        println!("No users.");
        return Ok(());
    }

    for (id, username, is_admin, created_at) in &users {
        let admin = if *is_admin != 0 { " (admin)" } else { "" };
        // The bands each user is in, so that this and `list-bands`
        // together answer "who can see what" without opening the
        // database. A user in none is worth seeing as such: they can
        // sign in and reach nothing.
        let bands = queries::bands::slugs_for_user(pool, *id).await?;
        let bands = if bands.is_empty() {
            "no bands".to_string()
        } else {
            bands.join(", ")
        };
        println!("{id}\t{username}{admin}\t{bands}\tcreated {created_at}");
    }
    Ok(())
}

/// Re-derive `recording_date` for every clip that's currently missing
/// one. Only touches NULL rows, so it's safe to re-run and never
/// overwrites a date already on file.
async fn backfill_dates(config: &Config, pool: &sqlx::SqlitePool, dry_run: bool) -> Result<()> {
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
async fn backfill_waveforms(config: &Config, pool: &sqlx::SqlitePool, dry_run: bool) -> Result<()> {
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

        match crate::audio::compute(&path) {
            Some(wf) => {
                let peaks = crate::audio::peaks_to_json(&wf.peaks);
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

/// A band by slug, or an error naming it. Used by the commands that act
/// on an existing band, which at the command line is the site owner and
/// therefore not a question of membership.
async fn find_band(pool: &sqlx::SqlitePool, slug: &str) -> Result<crate::models::Band> {
    queries::bands::find_by_slug(pool, slug)
        .await?
        .with_context(|| format!("no such band: {slug}"))
}

async fn resolve_user_and_band(
    pool: &sqlx::SqlitePool,
    username: &str,
    slug: &str,
) -> Result<(i64, crate::models::Band)> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await?;
    let (user_id,) = row.with_context(|| format!("no such user: {username}"))?;
    let band = find_band(pool, slug).await?;
    Ok((user_id, band))
}

/// Create a band, refusing a slug that could not work as one.
///
/// Three ways a slug is refused, and each says which: it isn't
/// lower-kebab-case, it is already taken, or the server already answers
/// on that path and a band there would have a clip list nobody could
/// reach.
async fn create_band(pool: &sqlx::SqlitePool, slug: &str) -> Result<()> {
    if !queries::bands::is_valid_slug(slug) {
        anyhow::bail!(
            "{slug} is not a valid slug: lower-case letters, digits and single hyphens \
             between them, like `bonband` or `the-other-band`"
        );
    }
    if queries::bands::RESERVED_SLUGS.contains(&slug) {
        anyhow::bail!(
            "{slug} is a path the server already uses, so a band called that would be \
             unreachable; reserved: {}",
            queries::bands::RESERVED_SLUGS.join(", ")
        );
    }
    if queries::bands::find_by_slug(pool, slug).await?.is_some() {
        anyhow::bail!("there is already a band called {slug}");
    }
    queries::bands::create(pool, slug).await?;
    println!("Created band {slug}");
    println!("Add someone to it with: iggybilly assign-to-band <username> {slug}");
    Ok(())
}

/// Rename a band, which is the only way its slug changes.
///
/// It says what it costs as it goes: there is no redirect from the old
/// slug, so every link anyone has sent stops working. That is acceptable
/// precisely because this is a rare, deliberate command run by the person
/// who can warn the band.
async fn rename_band(pool: &sqlx::SqlitePool, old_slug: &str, new_slug: &str) -> Result<()> {
    let band = find_band(pool, old_slug).await?;
    if !queries::bands::is_valid_slug(new_slug) {
        anyhow::bail!("{new_slug} is not a valid slug");
    }
    if queries::bands::RESERVED_SLUGS.contains(&new_slug) {
        anyhow::bail!("{new_slug} is a path the server already uses");
    }
    if let Some(existing) = queries::bands::find_by_slug(pool, new_slug).await? {
        if existing.id != band.id {
            anyhow::bail!("there is already a band called {new_slug}");
        }
    }
    queries::bands::rename(pool, band.id, new_slug).await?;
    println!("Renamed {old_slug} to {new_slug}");
    println!("Every link to /{old_slug}/… now 404s. There is no redirect from it.");
    Ok(())
}

async fn list_bands(pool: &sqlx::SqlitePool) -> Result<()> {
    let bands = queries::bands::list_with_counts(pool).await?;
    if bands.is_empty() {
        println!("No bands.");
        return Ok(());
    }
    for (band, members, clips) in &bands {
        let webhook = match &band.discord_webhook_url {
            Some(_) => "own Discord webhook",
            None => "instance Discord webhook",
        };
        println!(
            "{}\t{}\t{members} member(s), {clips} clip(s)\t{webhook}",
            band.id, band.slug
        );
    }
    Ok(())
}
