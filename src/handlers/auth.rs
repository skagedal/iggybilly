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

    let Some((id, username, hash, is_admin)) = row else {
        return render(LoginPage { error: Some("Invalid username or password.") });
    };
    if !auth::verify_password(&form.password, &hash) {
        return render(LoginPage { error: Some("Invalid username or password.") });
    }

    let user = SessionUser { id, username, is_admin: is_admin != 0 };
    session.insert(SESSION_USER_KEY, &user).await?;
    Ok(Redirect::to("/").into_response())
}

pub async fn logout(session: Session) -> AppResult<Response> {
    session.flush().await?;
    Ok(Redirect::to("/login").into_response())
}
