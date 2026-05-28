use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        // Block briefly on SQLITE_BUSY instead of bubbling a 500 to the
        // user — WAL plus this is enough for a handful of writers.
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePool::connect_with(opts)
        .await
        .with_context(|| format!("opening sqlite at {}", path.display()))?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
