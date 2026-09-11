use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::{
    auth,
    error::{AppError, AppResult},
    handlers::{page, PageFormat},
    web::{AppState, CurrentUser},
};

#[derive(Serialize)]
struct AccountProps {
    username: String,
}

pub async fn form(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    format: PageFormat,
) -> AppResult<Response> {
    page(
        &state,
        format,
        "Account — iggybilly",
        "account",
        &AccountProps {
            username: user.username,
        },
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// POST /api/account/password. The "confirm" field never reaches the
/// server — matching the two entries is purely a client-side typo guard,
/// so the request carries just the one new password.
pub async fn change_password(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    session: Session,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<Response> {
    if req.new_password.len() < 10 {
        return Err(AppError::BadRequest(
            "New password must be at least 10 characters.".into(),
        ));
    }

    let stored: (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?;
    if !auth::verify_password(&req.current_password, &stored.0) {
        return Err(AppError::BadRequest(
            "Current password is incorrect.".into(),
        ));
    }

    let new_hash = auth::hash_password(&req.new_password).map_err(AppError::Other)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    // Rotate the session id: if the user is changing their password
    // because they fear compromise, the old cookie shouldn't keep
    // working.
    session.cycle_id().await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}
