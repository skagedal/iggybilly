use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
    Form,
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::{
    auth,
    error::AppResult,
    handlers::render,
    models::SessionUser,
    web::{AppState, SESSION_USER_KEY},
};

#[derive(Template)]
#[template(path = "login.html")]
struct LoginPage {
    error: Option<&'static str>,
}

pub async fn login_form() -> AppResult<Response> {
    render(LoginPage { error: None })
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let row: Option<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT id, username, password_hash, is_admin FROM users WHERE username = ?",
    )
    .bind(&form.username)
    .fetch_optional(&state.pool)
    .await?;

    // Verify against a real (dummy) hash even when the user is missing,
    // so wrong-password and unknown-username take the same wall-clock
    // time. Without this an attacker can enumerate usernames by timing.
    let (user_record, ok) = match row {
        Some((id, username, hash, is_admin)) => {
            let ok = auth::verify_password(&form.password, &hash);
            (Some((id, username, is_admin)), ok)
        }
        None => {
            let _ = auth::verify_password(&form.password, auth::dummy_hash());
            (None, false)
        }
    };

    if !ok {
        auth::failed_login_penalty().await;
        return render(LoginPage {
            error: Some("Invalid username or password."),
        });
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
    Ok(Redirect::to("/").into_response())
}

pub async fn logout(session: Session) -> AppResult<Response> {
    session.flush().await?;
    Ok(Redirect::to("/login").into_response())
}
