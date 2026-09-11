//! `/api/v1` — the interface the mobile app talks to.
//!
//! Why a second surface at all, when the web already has `/api` and the
//! page routes already answer JSON? Because those two are shaped for the
//! browser. A page route returns `{entry, title, props}`, where `entry`
//! names a React module; the label lists it carries are full of `href`s
//! into the web UI; the dates in it are pre-formatted Stockholm strings,
//! because that is what the page prints. None of that survives contact
//! with a phone, and none of it should have to: an app that fed on it
//! would break every time a page was restyled.
//!
//! So this module is a second *presentation* of the same data, not a
//! second implementation. Every read goes through `crate::queries`, the
//! same functions the page handlers use, and every write goes through
//! the same shared helper the web's own endpoint calls. What differs is
//! only the wire shape:
//!
//! - timestamps are the raw RFC 3339 the database holds, so the device
//!   can format them in its own locale and time zone;
//! - wiki pages carry Markdown source, not server-rendered HTML, because
//!   Flutter renders Markdown itself;
//! - there are no `href`s, only ids and names;
//! - URLs that the client needs but cannot derive — a clip's audio — are
//!   named explicitly, relative to the server's origin.
//!
//! Authentication is `Authorization: Bearer` (see `crate::tokens`),
//! though the `ApiUser` extractor will equally accept a session cookie,
//! which is what lets these routes be exercised from a browser.

pub mod auth;
pub mod clips;
pub mod labels;

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post},
    Router,
};

use crate::web::{AppState, MAX_UPLOAD_REQUEST_BYTES};

/// The whole v1 surface, to be nested under `/api/v1`.
pub fn router() -> Router<AppState> {
    Router::new()
        // Sessions, as the app sees them: a token per install.
        .route("/tokens", post(auth::create_token).get(auth::list_tokens))
        .route("/tokens/current", delete(auth::revoke_current_token))
        .route("/tokens/{id}", delete(auth::revoke_token))
        .route("/me", get(auth::me))
        .route("/me/password", post(auth::change_password))
        // Clips.
        .route(
            "/clips",
            get(clips::list).post(clips::upload).layer(
                // Same cap as the web's upload route: the app can send a
                // batch too, and the per-file limit still applies inside.
                DefaultBodyLimit::max(MAX_UPLOAD_REQUEST_BYTES),
            ),
        )
        .route("/clips/{id}", get(clips::detail).delete(clips::delete))
        .route("/clips/{id}/name", post(clips::rename))
        .route("/clips/{id}/labels", post(labels::add))
        .route("/clips/{id}/labels/{label_id}", delete(labels::remove))
        // Labels and their wiki pages.
        .route("/labels/search", get(labels::search))
        .route(
            "/labels/{id}/wiki",
            get(labels::wiki).post(labels::save_wiki),
        )
        .route("/labels/{id}/wiki/history", get(labels::wiki_history))
        .route(
            "/labels/{id}/wiki/restore/{rev}",
            post(labels::restore_wiki),
        )
}
