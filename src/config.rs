use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub audio_dir: PathBuf,
    /// Where the frontend build output lives: served at /static, and
    /// read at startup for `dist/manifest.json`. Relative by default,
    /// so it depends on the working directory — hence the env var, and
    /// hence `serve` refusing to start if it isn't there.
    pub static_dir: PathBuf,
    /// Set the `Secure` flag on the session cookie. Default true so a
    /// misconfigured prod deploy fails closed; tests flip it off because
    /// they speak plain HTTP to 127.0.0.1.
    pub secure_cookies: bool,
    /// Discord incoming-webhook URL. When `None`, clip-upload and
    /// wiki-edit notifications are silently skipped — so local dev and
    /// tests need no Discord setup.
    pub discord_webhook_url: Option<String>,
    /// Public origin the app is served from, e.g.
    /// `https://iggybilly.skagedal.tech`, with no trailing slash. Used to
    /// turn clips and labels into clickable links in Discord posts. When
    /// `None`, posts carry plain names instead of links.
    pub base_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir: PathBuf = std::env::var("IGGYBILLY_DATA_DIR")
            .unwrap_or_else(|_| "./data".into())
            .into();
        let listen_addr =
            std::env::var("IGGYBILLY_LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:9020".into());
        let db_path = data_dir.join("iggybilly.db");
        let audio_dir = data_dir.join("audio");
        let static_dir: PathBuf = std::env::var("IGGYBILLY_STATIC_DIR")
            .unwrap_or_else(|_| "./static".into())
            .into();
        let secure_cookies = std::env::var("IGGYBILLY_SECURE_COOKIES")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "no"))
            .unwrap_or(true);
        let discord_webhook_url = non_empty_env("IGGYBILLY_DISCORD_WEBHOOK_URL");
        // Trim a trailing slash so we can build `${base}/clips/1` without
        // risking a double slash.
        let base_url =
            non_empty_env("IGGYBILLY_BASE_URL").map(|u| u.trim_end_matches('/').to_string());
        Self {
            listen_addr,
            data_dir,
            db_path,
            audio_dir,
            static_dir,
            secure_cookies,
            discord_webhook_url,
            base_url,
        }
    }
}

/// Read an env var, treating unset and empty/whitespace-only the same
/// (both yield `None`) so an env var set to "" disables the feature
/// rather than producing a useless empty URL.
fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
