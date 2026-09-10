use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("Not found.")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("EPUB too large: asked {asked} bytes, limit is MEWO_MAX_EPUB_BYTES={limit}")]
    PayloadTooLarge { asked: u64, limit: u64 },
    #[error("Request body is too large.")]
    RequestTooLarge,
    #[error("Internal error.")]
    Internal,
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(error = %err, "database error");
        ApiError::Internal
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        tracing::error!(error = %err, "io error");
        ApiError::Internal
    }
}

impl From<axum::extract::multipart::MultipartError> for ApiError {
    fn from(err: axum::extract::multipart::MultipartError) -> Self {
        if err.status() == StatusCode::PAYLOAD_TOO_LARGE {
            return ApiError::RequestTooLarge;
        }
        ApiError::BadRequest(err.body_text())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::PayloadTooLarge { .. } | ApiError::RequestTooLarge => {
                StatusCode::PAYLOAD_TOO_LARGE
            }
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(ErrorBody {
            error: self.to_string(),
        });
        (status, body).into_response()
    }
}
