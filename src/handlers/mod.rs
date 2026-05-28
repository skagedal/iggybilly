pub mod account;
pub mod auth;
pub mod clips;
pub mod labels;

use askama::Template;
use axum::response::{Html, IntoResponse, Response};

use crate::error::AppError;

/// Render an Askama template into an HTML response. Centralised here so
/// every handler doesn't repeat the same three lines.
pub fn render<T: Template>(tpl: T) -> Result<Response, AppError> {
    let body = tpl.render()?;
    Ok(Html(body).into_response())
}
