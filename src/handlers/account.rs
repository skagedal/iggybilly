use askama::Template;
use axum::{Form, extract::State, response::Response};
use serde::Deserialize;

use crate::{
    auth,
    error::{AppError, AppResult},
    handlers::render,
    web::{AppState, CurrentUser},
};

#[derive(Template)]
#[template(path = "account.html")]
struct AccountPage {
    username: String,
    error: Option<String>,
    success: Option<&'static str>,
}

pub async fn form(CurrentUser(user): CurrentUser) -> AppResult<Response> {
    render(AccountPage { username: user.username, error: None, success: None })
}

#[derive(Deserialize)]
pub struct ChangeForm {
    current_password: String,
    new_password: String,
    new_password_confirm: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Form(form): Form<ChangeForm>,
) -> AppResult<Response> {
    if form.new_password != form.new_password_confirm {
        return render(AccountPage {
            username: user.username,
            error: Some("New passwords don't match.".into()),
            success: None,
        });
    }
    if form.new_password.len() < 10 {
        return render(AccountPage {
            username: user.username,
            error: Some("New password must be at least 10 characters.".into()),
            success: None,
        });
    }

    let stored: (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await?;
    if !auth::verify_password(&form.current_password, &stored.0) {
        return render(AccountPage {
            username: user.username,
            error: Some("Current password is incorrect.".into()),
            success: None,
        });
    }

    let new_hash = auth::hash_password(&form.new_password).map_err(AppError::Other)?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&new_hash)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    render(AccountPage {
        username: user.username,
        error: None,
        success: Some("Password updated."),
    })
}
