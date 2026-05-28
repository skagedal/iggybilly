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
    }
}
