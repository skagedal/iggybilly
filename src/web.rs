use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tower_sessions::{Expiry, Session, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

use crate::{
    config::Config,
    error::AppError,
    handlers::{account, auth as auth_handlers, clips, labels},
    models::SessionUser,
};

pub const SESSION_USER_KEY: &str = "user";

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
}

pub async fn serve(config: Config, pool: SqlitePool) -> Result<()> {
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

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(time::Duration::days(30)));

    let state = AppState { pool, config };

    Ok(Router::new()
        .route("/", get(clips::list))
        .route("/clips", post(clips::upload))
        .route("/clips/{id}", get(clips::detail))
        .route("/clips/{id}/audio", get(clips::audio))
        .route("/clips/{id}/name", post(clips::rename))
        .route("/clips/{id}/name/form", get(clips::rename_form))
        .route("/clips/{id}/name/display", get(clips::rename_display))
        .route("/clips/{id}/labels", post(labels::add))
        .route("/clips/{id}/labels/{label_id}", axum::routing::delete(labels::remove))
        .route("/labels/search", get(labels::search))
        .route("/login", get(auth_handlers::login_form).post(auth_handlers::login))
        .route("/logout", post(auth_handlers::logout))
        .route("/account", get(account::form).post(account::change_password))
        .nest_service("/static", ServeDir::new("static"))
        .layer(session_layer)
        .layer(TraceLayer::new_for_http())
        .with_state(state))
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

/// Like CurrentUser, but for handlers that should return 401 (HTMX
/// fragments) instead of redirecting.
pub struct CurrentUserApi(pub SessionUser);

impl<S> FromRequestParts<S> for CurrentUserApi
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Unauthorized)?;
        match session.get::<SessionUser>(SESSION_USER_KEY).await {
            Ok(Some(u)) => Ok(CurrentUserApi(u)),
            _ => Err(AppError::Unauthorized),
        }
    }
}
