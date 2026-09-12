use std::{sync::Arc, time::Duration};

use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, FromRequestParts},
    http::request::Parts,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::{
    services::ServeDir,
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tower_sessions::{cookie::SameSite, ExpiredDeletion, Expiry, Session, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;
use tracing::Level;

use crate::{
    assets::Assets,
    config::Config,
    discord::Discord,
    error::AppError,
    handlers::{account, auth as auth_handlers, clips, labels},
    models::SessionUser,
};

pub const SESSION_USER_KEY: &str = "user";

/// Hard cap on a single audio file, in bytes. Audio clips for a band app
/// comfortably fit under this; anything larger is almost certainly not
/// the file the user meant to upload. Enforced per file as we stream.
pub const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

/// Hard cap on the whole multipart upload request. A single POST can now
/// carry several files (multi-select / drag-and-drop), so the request
/// body limit is a generous multiple of the per-file cap. We still never
/// buffer a whole file — each is streamed to disk — so this only bounds
/// total bytes read, not memory.
pub const MAX_UPLOAD_REQUEST_BYTES: usize = 100 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
    /// Posts clip-upload and wiki-edit notifications to Discord. Disabled
    /// (a no-op) when no webhook URL is configured.
    pub discord: Discord,
    /// Maps frontend entry names to their content-hashed URLs.
    pub assets: Arc<Assets>,
}

pub async fn serve(config: Config, pool: SqlitePool) -> Result<()> {
    // Without the manifest every page would still render its shell, but
    // pointing at bundle URLs that 404 — a blank screen with nothing in
    // the browser console to explain it. Refuse to start instead: the
    // app cannot work without the frontend build, and a startup error
    // naming the missing file is a much shorter debugging session.
    let manifest = config.static_dir.join("dist/manifest.json");
    if !manifest.exists() {
        anyhow::bail!(
            "no frontend build found at {} — start the app with `./local/run serve`, \
             which builds the frontend first, or run `pnpm install && pnpm run build` \
             in web/ yourself; set IGGYBILLY_STATIC_DIR if the bundles live elsewhere \
             (the default is ./static, relative to the working directory)",
            manifest.display()
        );
    }

    let listen = config.listen_addr.clone();
    let app = build_app(pool, Arc::new(config)).await?;
    let listener = TcpListener::bind(&listen).await?;
    tracing::info!("listening on {listen}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the full axum Router with session middleware and shared state.
/// Split out from `serve` so integration tests can mount the router on
/// a random-port `TcpListener` without going through the CLI/config
/// env-var path.
pub async fn build_app(pool: SqlitePool, config: Arc<Config>) -> Result<Router> {
    let session_store = SqliteStore::new(pool.clone());
    session_store.migrate().await?;

    // Reap expired sessions hourly so the tower_sessions table doesn't
    // grow unbounded. The task lives for the process lifetime.
    tokio::spawn(
        session_store
            .clone()
            .continuously_delete_expired(Duration::from_secs(3600)),
    );

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(config.secure_cookies)
        // Strict means a cross-site request won't carry the cookie —
        // our CSRF mitigation for the state-changing /api endpoints,
        // which is why they need no separate token. The only UX cost is
        // that following an external link to the app doesn't carry the
        // session on that first navigation.
        .with_same_site(SameSite::Strict)
        .with_expiry(Expiry::OnInactivity(time::Duration::days(30)));

    let discord = Discord::new(config.discord_webhook_url.clone(), config.base_url.clone());
    let static_dir = config.static_dir.clone();
    let state = AppState {
        pool,
        assets: Arc::new(Assets::load(&static_dir)),
        config,
        discord,
    };

    // Two kinds of route. The handful under no prefix are pages: real
    // URLs that return an HTML shell plus that page's React bundle, so
    // the app stays multi-page and the server keeps owning routing.
    // Everything under /api is JSON, called by those bundles with fetch.
    let pages = Router::new()
        .route("/", get(clips::list))
        .route("/clips/{id}", get(clips::detail))
        .route("/labels/{id}/wiki/history", get(labels::wiki_history))
        .route("/login", get(auth_handlers::login_form))
        .route("/account", get(account::form));

    let api = Router::new()
        .route(
            "/clips",
            post(clips::upload).layer(DefaultBodyLimit::max(MAX_UPLOAD_REQUEST_BYTES)),
        )
        .route("/clips/{id}", axum::routing::delete(clips::delete))
        .route("/clips/{id}/name", post(clips::rename))
        .route("/clips/{id}/labels", post(labels::add))
        .route(
            "/clips/{id}/labels/{label_id}",
            axum::routing::delete(labels::remove),
        )
        .route("/labels/search", get(labels::search))
        .route(
            "/labels/{id}/wiki",
            get(labels::wiki_view).post(labels::wiki_save),
        )
        .route(
            "/labels/{id}/wiki/restore/{rev}",
            post(labels::wiki_restore),
        )
        .route("/login", post(auth_handlers::login))
        .route("/logout", post(auth_handlers::logout))
        .route("/account/password", post(account::change_password));

    Ok(pages
        .merge(Router::new().nest("/api", api))
        // And /api/v1 is the app's: the same data, shaped for a phone
        // rather than for the React pages. See `crate::api`.
        .merge(Router::new().nest("/api/v1", crate::api::router()))
        // Audio is none of those: the browser hits it directly as an
        // <audio> src and as a download link, and the app hands it to
        // the platform player, so it stays a plain URL.
        .route("/clips/{id}/audio", get(clips::audio))
        .route("/healthz", get(healthz))
        .nest_service("/static", ServeDir::new(&static_dir))
        .layer(session_layer)
        // Log each request and its response (method, path, status,
        // latency) at INFO. The span carries method/path, so any
        // tracing event emitted while handling the request — including
        // the 500 error log in AppError — is tagged with them.
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::extract::Request| {
                    tracing::info_span!(
                        "request",
                        method = %req.method(),
                        path = %req.uri().path(),
                    )
                })
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(
                    DefaultOnResponse::new()
                        .level(Level::INFO)
                        .latency_unit(LatencyUnit::Millis),
                ),
        )
        .with_state(state))
}

async fn healthz() -> &'static str {
    "ok"
}

/// Extractor that resolves the logged-in user from the session, or
/// redirects to /login if there is none. Use this in handlers that
/// require auth; for unauthenticated routes (login form) don't add it.
pub struct CurrentUser(pub SessionUser);

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|e| e.into_response())?;
        match session.get::<SessionUser>(SESSION_USER_KEY).await {
            Ok(Some(u)) => Ok(CurrentUser(u)),
            _ => Err(Redirect::to("/login").into_response()),
        }
    }
}

/// The caller of a JSON endpoint, however they identified themselves.
///
/// Two clients, two credentials: the browser sends its session cookie,
/// the app sends `Authorization: Bearer`. The endpoints don't care which
/// — they want a user — so the difference is resolved once, here. A
/// bearer token is tried first: a request that presents one has told us
/// what it is, and falling back to a cookie after a bad token would be
/// how you accidentally act as the wrong user on a shared device.
///
/// These endpoints answer 401 rather than redirecting. A redirect to the
/// login page is something `fetch` follows and the app cannot use, so
/// either client would end up parsing HTML where it expected data.
pub struct ApiUser {
    pub user: SessionUser,
    /// The token this request arrived with, when it arrived with one.
    /// The token endpoints use it to mark the caller's own device.
    pub token_id: Option<i64>,
}

impl FromRequestParts<AppState> for ApiUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        if let Some(secret) = bearer_token(parts) {
            return match crate::tokens::authenticate(&state.pool, &secret).await? {
                Some((user, token_id)) => Ok(ApiUser {
                    user,
                    token_id: Some(token_id),
                }),
                None => Err(AppError::Unauthorized("invalid or revoked token".into())),
            };
        }
        Ok(ApiUser {
            user: session_user(parts, state).await?,
            token_id: None,
        })
    }
}

/// The audio route, which is neither a page nor a JSON endpoint: the
/// browser reaches it as an `<audio>` source and the app hands it to the
/// platform player, so it takes either credential and fails the way the
/// caller can act on — 401 for a request that presented a token, a
/// redirect to the login page for a browser that presented nothing.
pub struct MediaUser(#[allow(dead_code)] pub SessionUser);

impl FromRequestParts<AppState> for MediaUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        if let Some(secret) = bearer_token(parts) {
            return match crate::tokens::authenticate(&state.pool, &secret).await {
                Ok(Some((user, _))) => Ok(MediaUser(user)),
                Ok(None) => {
                    Err(AppError::Unauthorized("invalid or revoked token".into()).into_response())
                }
                Err(e) => Err(e.into_response()),
            };
        }
        match session_user(parts, state).await {
            Ok(user) => Ok(MediaUser(user)),
            Err(_) => Err(Redirect::to("/login").into_response()),
        }
    }
}

/// The `Authorization: Bearer <token>` value, if the request carries a
/// well-formed one. The scheme is matched case-insensitively, as RFC
/// 7235 requires.
fn bearer_token(parts: &Parts) -> Option<String> {
    let raw = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, value) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The signed-in user from the session cookie, or an Unauthorized error.
async fn session_user(parts: &mut Parts, state: &AppState) -> Result<SessionUser, AppError> {
    let session = Session::from_request_parts(parts, state)
        .await
        .map_err(|_| AppError::Unauthorized("not signed in".into()))?;
    match session.get::<SessionUser>(SESSION_USER_KEY).await {
        Ok(Some(u)) => Ok(u),
        _ => Err(AppError::Unauthorized("not signed in".into())),
    }
}
