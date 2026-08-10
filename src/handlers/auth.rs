use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    auth,
    error::{AppError, AppResult},
    handlers::{page, PageFormat},
    models::SessionUser,
    web::{AppState, SESSION_USER_KEY},
};

/// GET /login — the shell for the login bundle. It needs no props: the
/// form posts to /api/login and navigates to / on success.
pub async fn login_form(State(state): State<AppState>, format: PageFormat) -> AppResult<Response> {
    page(
        &state,
        format,
        "Sign in — iggybilly",
        "login",
        &serde_json::json!({}),
    )
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(req): Json<LoginRequest>,
) -> AppResult<Response> {
    let row: Option<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, is_admin FROM users WHERE username = ?",
    )
    .bind(&req.username)
    .fetch_optional(&state.pool)
    .await?;

    // Verify against a real (dummy) hash even when the user is missing,
    // so wrong-password and unknown-username take the same wall-clock
    // time. Without this an attacker can enumerate usernames by timing.
    let (user_record, ok) = match row {
        Some((id, username, hash, is_admin)) => {
            let ok = auth::verify_password(&req.password, &hash);
            (Some((id, username, is_admin)), ok)
        }
        None => {
            let _ = auth::verify_password(&req.password, auth::dummy_hash());
            (None, false)
        }
    };

    if !ok {
        auth::failed_login_penalty().await;
        // One message for both failure modes, for the same reason the
        // timing is levelled: don't confirm which usernames exist.
        return Err(AppError::Unauthorized(
            "Invalid username or password.".into(),
        ));
    }
    let (id, username, is_admin) = user_record.expect("ok implies a user row");

    // Rotate the session id on auth boundary so any pre-login session
    // fixation attempt is invalidated.
    session.cycle_id().await?;
    let user = SessionUser {
        id,
        username,
        is_admin: is_admin != 0,
    };
    session.insert(SESSION_USER_KEY, &user).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn logout(session: Session) -> AppResult<Response> {
    session.flush().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
