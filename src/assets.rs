//! Resolves logical frontend entry names to their hashed URLs.
//!
//! `web/build.mjs` bundles each page into `static/dist/<name>-<hash>.js`
//! and writes a `manifest.json` alongside them mapping `"index.js"` →
//! `"/static/dist/index-7LSHXFEU.js"`. The content hash is what lets us
//! serve the bundles with a long, immutable cache lifetime; it also
//! means the server can't construct the URL itself, hence this lookup.
//!
//! The manifest is read once at startup. Rebuilding the frontend while
//! the server runs therefore needs a server restart to pick up the new
//! hashes — `cargo watch`/a manual restart in dev, and in production the
//! image carries a matched pair of binary and bundles anyway.

use std::{collections::HashMap, path::Path};

#[derive(Clone, Debug, Default)]
pub struct Assets {
    entries: HashMap<String, String>,
}

impl Assets {
    /// Load `<static_dir>/dist/manifest.json`. A missing or unparseable
    /// manifest is not fatal: `url` falls back to the unhashed path, so
    /// `cargo test` and `cargo run` work without having run the
    /// frontend build first (the page just won't find its bundle until
    /// you do).
    pub fn load(static_dir: &Path) -> Self {
        let path = static_dir.join("dist/manifest.json");
        let entries = match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<HashMap<String, String>>(&raw) {
                Ok(map) => map,
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "ignoring unreadable asset manifest");
                    HashMap::new()
                }
            },
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "no asset manifest; run `npm run build` in web/ to generate one"
                );
                HashMap::new()
            }
        };
        Self { entries }
    }

    /// URL for a logical entry such as `"main.js"` or `"app.css"`.
    ///
    /// Falls back to the unhashed path when there's no manifest, so a
    /// checkout that hasn't run the frontend build still serves a
    /// coherent page shell.
    pub fn url(&self, entry: &str) -> String {
        self.entries
            .get(entry)
            .cloned()
            .unwrap_or_else(|| format!("/static/dist/{entry}"))
    }

    /// Like `url`, but `None` rather than a guess when the entry isn't
    /// in the manifest. Used for the page-chunk `modulepreload` hint,
    /// where a wrong URL would be a console error for no gain.
    pub fn lookup(&self, entry: &str) -> Option<&str> {
        self.entries.get(entry).map(String::as_str)
    }
}
