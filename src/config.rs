use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub audio_dir: PathBuf,
    /// Set the `Secure` flag on the session cookie. Default true so a
    /// misconfigured prod deploy fails closed; tests flip it off because
    /// they speak plain HTTP to 127.0.0.1.
    pub secure_cookies: bool,
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
        let secure_cookies = std::env::var("IGGYBILLY_SECURE_COOKIES")
            .map(|v| !matches!(v.as_str(), "0" | "false" | "no"))
            .unwrap_or(true);
        Self { listen_addr, data_dir, db_path, audio_dir, secure_cookies }
    }
}
