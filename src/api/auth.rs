//! Tokens, the current user, and the password.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    handlers::{account, auth as auth_handlers},
    tokens,
    web::{ApiUser, AppState},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenRequest {
    pub username: String,
    pub password: String,
    /// What to call this install in the device list. The app sends the
    /// device's own name; anything blank becomes "Unnamed device".
    #[serde(default)]
    pub device_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateTokenResponse {
    /// The only time the plaintext token exists outside the client.
    /// Store it; it cannot be read back.
    token: String,
    token_id: i64,
    user: Me,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Me {
    id: i64,
    username: String,
    is_admin: bool,
}

/// POST /api/v1/tokens — sign in, and get a token for this device.
pub async fn create_token(
    State(state): State<AppState>,
    Json(req): Json<CreateTokenRequest>,
) -> AppResult<Response> {
    let user = auth_handlers::verify_credentials(&state, &req.username, &req.password).await?;
    let issued = tokens::issue(&state.pool, user.id, &req.device_name).await?;
    Ok(Json(CreateTokenResponse {
        token: issued.secret,
        token_id: issued.id,
        user: Me {
            id: user.id,
            username: user.username,
            is_admin: user.is_admin,
        },
    })
    .into_response())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceRow {
    id: i64,
    device_name: String,
    created_at: String,
    last_used_on: Option<String>,
    is_current: bool,
}

/// GET /api/v1/tokens — the caller's signed-in devices.
pub async fn list_tokens(
    State(state): State<AppState>,
    ApiUser { user, token_id }: ApiUser,
) -> AppResult<Response> {
    let rows: Vec<DeviceRow> = tokens::list(&state.pool, user.id, token_id)
        .await?
        .into_iter()
        .map(|d| DeviceRow {
            id: d.id,
            device_name: d.device_name,
            created_at: d.created_at,
            last_used_on: d.last_used_on,
            is_current: d.is_current,
        })
        .collect();
    Ok(Json(rows).into_response())
}

/// DELETE /api/v1/tokens/current — sign this device out.
///
/// A caller holding a session cookie rather than a token has nothing to
/// revoke here; it should use the web's own logout. Saying so beats
/// silently doing nothing.
pub async fn revoke_current_token(
    State(state): State<AppState>,
    ApiUser { user, token_id }: ApiUser,
) -> AppResult<Response> {
    let token_id = token_id.ok_or_else(|| {
        AppError::BadRequest("this request did not arrive with a token to revoke".into())
    })?;
    tokens::revoke(&state.pool, user.id, token_id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /api/v1/tokens/{id} — revoke one of the caller's devices,
/// which is how a lost phone is cut off from someone else's.
pub async fn revoke_token(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Path(id): Path<i64>,
) -> AppResult<Response> {
    if !tokens::revoke(&state.pool, user.id, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// GET /api/v1/me — who the caller is. The app uses it on launch to find
/// out whether its stored token is still good.
pub async fn me(ApiUser { user, .. }: ApiUser) -> AppResult<Response> {
    Ok(Json(Me {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
    })
    .into_response())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
    /// What to call the replacement token. The password change revokes
    /// every token the user has, this one included, so the app is handed
    /// a new one rather than being signed out by its own request.
    #[serde(default)]
    device_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordResponse {
    token: String,
    token_id: i64,
}

/// POST /api/v1/me/password.
pub async fn change_password(
    State(state): State<AppState>,
    ApiUser { user, .. }: ApiUser,
    Json(req): Json<ChangePasswordRequest>,
) -> AppResult<Response> {
    account::set_password(&state, user.id, &req.current_password, &req.new_password).await?;
    let issued = tokens::issue(&state.pool, user.id, &req.device_name).await?;
    Ok(Json(ChangePasswordResponse {
        token: issued.secret,
        token_id: issued.id,
    })
    .into_response())
}
