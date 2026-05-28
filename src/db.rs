use std::path::Path;

use anyhow::{Context, Result};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);
    let pool = SqlitePool::connect_with(opts)
        .await
        .with_context(|| format!("opening sqlite at {}", path.display()))?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
