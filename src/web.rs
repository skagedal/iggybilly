use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, FromRequestParts, Path},
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
    midi_render::Renderer,
    models::{Band, SessionUser},
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
    /// Renders MIDI uploads to audio. None without a SoundFont.
    pub renderer: Option<Arc<Renderer>>,
}

/// Load the configured SoundFont. A SoundFont that is configured but
/// can't be loaded stops the server, rather than quietly leaving every
/// MIDI clip unplayable.
pub fn load_renderer(config: &Config) -> Result<Option<Arc<Renderer>>> {
    let Some(path) = &config.soundfont else {
        tracing::warn!("IGGYBILLY_SOUNDFONT is not set: MIDI clips won't be playable");
        return Ok(None);
    };
    let renderer = Renderer::load(path)?;
    tracing::info!(soundfont = %path.display(), version = renderer.version(), "loaded SoundFont");
    Ok(Some(Arc::new(renderer)))
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

    let renderer = load_renderer(&config)?;

    // Detect formats and make playback copies for clips stored before
    // either existed, or rendered by another version. In the background:
    // the server is usable meanwhile, those clips just play as they did
    // until it gets to them.
    {
        let pool = pool.clone();
        let audio_dir = config.audio_dir.clone();
        let renderer = renderer.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::playback::backfill(&pool, &audio_dir, renderer).await {
                tracing::error!(error = ?e, "playback backfill failed");
            }
        });
    }

    let listen = config.listen_addr.clone();
    let app = build_app_with(pool, Arc::new(config), renderer).await?;
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
    let renderer = load_renderer(&config)?;
    build_app_with(pool, config, renderer).await
}

/// [`build_app`] with the SoundFont already loaded, so `serve` can share
/// it with the startup backfill instead of loading it twice.
pub async fn build_app_with(
    pool: SqlitePool,
    config: Arc<Config>,
    renderer: Option<Arc<Renderer>>,
) -> Result<Router> {
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
        renderer,
    };

    // Two kinds of route. The handful under no prefix are pages: real
    // URLs that return an HTML shell plus that page's React bundle, so
    // the app stays multi-page and the server keeps owning routing.
    // Everything under /api is JSON, called by those bundles with fetch.
    //
    // Every page about a band's content sits under the band's slug;
    // signing in and the account screen do not, because they are about
    // the person rather than about a band. `/` is nobody's band, so it
    // sends you to yours.
    let pages = Router::new()
        .route("/", get(clips::landing))
        // Both spellings, because a band's list is the one URL people
        // type and shorten by hand, and `/bonband` meaning nothing while
        // `/bonband/` works would be a puzzle rather than a redirect.
        .route("/{band}", get(clips::list))
        .route("/{band}/", get(clips::list))
        .route("/{band}/clips/{id}", get(clips::detail))
        .route(
            "/{band}/labels/{id}/wiki/history",
            get(labels::wiki_history),
        )
        .route("/login", get(auth_handlers::login_form))
        .route("/account", get(account::form));

    // The band-scoped half of the web's own JSON API. Everything here
    // reads or writes one band's content, so the slug is in the path for
    // the same reason it is in the page URLs.
    let band_api = Router::new()
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
        .route("/labels/{id}/order", post(labels::order))
        .route(
            "/labels/{id}/wiki",
            get(labels::wiki_view).post(labels::wiki_save),
        )
        .route(
            "/labels/{id}/wiki/restore/{rev}",
            post(labels::wiki_restore),
        );

    // The rest of /api is about the session or the account, and belongs
    // to no band.
    let api = Router::new()
        .route("/login", post(auth_handlers::login))
        .route("/logout", post(auth_handlers::logout))
        .route("/account/password", post(account::change_password))
        .route("/account/band", post(account::switch_band));

    Ok(pages
        .merge(Router::new().nest("/api", api))
        .merge(Router::new().nest("/api/{band}", band_api))
        // And /api/v1 is the app's: the same data, shaped for a phone
        // rather than for the React pages. See `crate::api`.
        .merge(Router::new().nest("/api/v1", crate::api::band_router()))
        .merge(Router::new().nest("/api/v1/{band}", crate::api::band_router()))
        .merge(Router::new().nest("/api/v1", crate::api::account_router()))
        // Audio is none of those: the browser hits it directly as an
        // <audio> src and as a download link, and the app hands it to
        // the platform player, so it stays a plain URL — a plain URL
        // under the band, like every other link to a clip.
        .route("/{band}/clips/{id}/audio", get(clips::audio))
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

/// Which band a request is about, resolved from the slug in its path and
/// checked against the caller's membership.
///
/// A slug that names no band and a slug that names a band the caller is
/// not in both come back as `NotFound`, from one query, so the two cannot
/// be told apart — otherwise trying slugs would be a way for a member of
/// one band to learn who else is on the instance.
async fn band_from_path(
    parts: &mut Parts,
    state: &AppState,
    user: &SessionUser,
) -> Result<Band, AppError> {
    let slug = path_param(parts, state, "band")
        .await
        .ok_or(AppError::NotFound)?;
    crate::queries::bands::resolve_for_member(&state.pool, &slug, user.id)
        .await?
        .ok_or(AppError::NotFound)
}

/// One named path capture, as a string. Extracting `Path` here doesn't
/// consume it, so a handler can still take its own typed parameters.
async fn path_param(parts: &mut Parts, state: &AppState, name: &str) -> Option<String> {
    let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
        .await
        .ok()?;
    params.get(name).cloned()
}

/// A page about one band's content: the signed-in user, and the band.
///
/// Arriving here also makes this band the one the user was last in, so a
/// link someone sent takes precedence over whatever they were looking at
/// before — nobody has to switch bands before a link works. The write is
/// conditional on the value actually changing, so an ordinary page view
/// costs a statement that matches no rows.
pub struct BandUser {
    pub user: SessionUser,
    pub band: Band,
}

impl FromRequestParts<AppState> for BandUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        let band = band_from_path(parts, state, &user)
            .await
            .map_err(|e| e.into_response())?;
        crate::queries::bands::set_last_band(&state.pool, user.id, band.id)
            .await
            .map_err(|e| e.into_response())?;
        Ok(BandUser { user, band })
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

/// A JSON endpoint about one band's content, for either client.
///
/// The band comes from the path when the path carries one, and from the
/// caller's last band when it does not. That second case is only for the
/// `/api/v1` routes without a band segment, which every app already
/// installed still calls and will go on calling until each phone
/// updates. For a user in one band — every user on the day this ships —
/// it resolves to their band and the old app carries on working.
///
/// This is the one place the last-band preference scopes a request
/// rather than directing a person, and it is why that preference has to
/// be a column: a bearer token from an old app build has no session to
/// keep it in.
pub struct ApiBand {
    pub user: SessionUser,
    #[allow(dead_code)]
    pub token_id: Option<i64>,
    pub band: Band,
}

impl FromRequestParts<AppState> for ApiBand {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let ApiUser { user, token_id } = ApiUser::from_request_parts(parts, state).await?;
        let band = match path_param(parts, state, "band").await {
            Some(slug) => crate::queries::bands::resolve_for_member(&state.pool, &slug, user.id)
                .await?
                .ok_or(AppError::NotFound)?,
            None => crate::queries::bands::landing_band(&state.pool, user.id)
                .await?
                .ok_or(AppError::NoBand)?,
        };
        Ok(ApiBand {
            user,
            token_id,
            band,
        })
    }
}

/// The audio route, which is neither a page nor a JSON endpoint: the
/// browser reaches it as an `<audio>` source and the app hands it to the
/// platform player, so it takes either credential and fails the way the
/// caller can act on — 401 for a request that presented a token, a
/// redirect to the login page for a browser that presented nothing.
/// It carries the band from its path as well, so the bytes of a clip are
/// reachable only under the band that owns it — the audio URL is a link
/// like any other and is checked like any other.
pub struct MediaUser {
    #[allow(dead_code)]
    pub user: SessionUser,
    pub band: Band,
}

impl FromRequestParts<AppState> for MediaUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let user = if let Some(secret) = bearer_token(parts) {
            match crate::tokens::authenticate(&state.pool, &secret).await {
                Ok(Some((user, _))) => user,
                Ok(None) => {
                    return Err(
                        AppError::Unauthorized("invalid or revoked token".into()).into_response()
                    )
                }
                Err(e) => return Err(e.into_response()),
            }
        } else {
            match session_user(parts, state).await {
                Ok(user) => user,
                Err(_) => return Err(Redirect::to("/login").into_response()),
            }
        };

        let band = band_from_path(parts, state, &user)
            .await
            .map_err(|e| e.into_response())?;
        Ok(MediaUser { user, band })
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

/// The path parameters of a route about one clip or one label.
///
/// `band` is an `Option` because the `/api/v1` routes exist twice: with
/// a band segment, which is what an app that knows about bands calls,
/// and without one, which is what every app already installed calls. One
/// set of handlers serves both, and `ApiBand` is what turns the absence
/// into the caller's last band. Handlers destructure the id out and
/// never read `band` themselves — the extractor has already resolved it
/// into a `Band` they cannot forget to check.
#[derive(serde::Deserialize)]
pub struct IdPath {
    #[serde(default)]
    #[allow(dead_code)]
    pub band: Option<String>,
    pub id: i64,
}

/// `/clips/{id}/labels/{label_id}`.
#[derive(serde::Deserialize)]
pub struct ClipLabelPath {
    #[serde(default)]
    #[allow(dead_code)]
    pub band: Option<String>,
    pub id: i64,
    pub label_id: i64,
}

/// `/labels/{id}/wiki/restore/{rev}`.
#[derive(serde::Deserialize)]
pub struct LabelRevPath {
    #[serde(default)]
    #[allow(dead_code)]
    pub band: Option<String>,
    pub id: i64,
    pub rev: i64,
}
