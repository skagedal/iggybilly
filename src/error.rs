use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Render(#[from] askama::Error),
    #[error(transparent)]
    Multipart(#[from] axum::extract::multipart::MultipartError),
    #[error(transparent)]
    Session(#[from] tower_sessions::session::Error),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg): (StatusCode, String) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
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
        (status, msg).into_response()
    }
}
