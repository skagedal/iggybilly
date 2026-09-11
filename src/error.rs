use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    /// No usable session, or credentials that didn't check out. Carries
    /// a user-facing message because the login form shows it verbatim.
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden")]
    Forbidden,
    #[error("bad request: {0}")]
    BadRequest(String),
    /// Well-formed but conflicts with existing state — currently only a
    /// clip name that's already taken. The message is user-facing.
    #[error("conflict: {0}")]
    Conflict(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Render(#[from] askama::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Multipart(#[from] axum::extract::multipart::MultipartError),
    #[error(transparent)]
    Session(#[from] tower_sessions::session::Error),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Every error response is `{"error": "..."}`, so the frontend has one
/// shape to unwrap (see `web/src/api.ts`). Page routes fail into the
/// same shape — a bare JSON body in the browser rather than a styled
/// error page, which is a fair trade for a single error path.
#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg): (StatusCode, String) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            AppError::Multipart(e) => {
                // A malformed or interrupted multipart body is a client
                // problem (e.g. a flaky mobile upload), not a server
                // fault — return 400, but log it so failed uploads are
                // visible rather than a silent error.
                tracing::warn!(error = ?e, "rejected multipart upload");
                (
                    StatusCode::BAD_REQUEST,
                    "could not read the uploaded form data".into(),
                )
            }
            _ => {
                tracing::error!(error = ?self, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                )
            }
        };
        (status, Json(ErrorBody { error: msg })).into_response()
    }
}
