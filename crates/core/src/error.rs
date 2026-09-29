use thiserror::Error;

use axum::Json;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("database migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("storage error: {0}")]
    Storage(#[from] object_store::Error),

    #[error("render error: {0}")]
    Render(String),

    #[error("sanitization error: {0}")]
    Sanitization(String),

    #[error("QR generation error: {0}")]
    QrGeneration(String),

    #[error("email error: {0}")]
    Email(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("configuration error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("argon2 error: {0}")]
    Argon2(#[from] argon2::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("environment variable error: {0}")]
    EnvVar(#[from] std::env::VarError),

    #[error("base62 encoding error: {0}")]
    Base62(String),

    #[error("template validation error: {0}")]
    TemplateValidation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("gone: {0}")]
    Gone(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("short code error: {0}")]
    ShortCode(#[from] crate::short_code::ShortCodeError),

    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("password hashing error: {0}")]
    PasswordHash(#[from] argon2::password_hash::Error),

    #[error("multipart error: {0}")]
    Multipart(#[from] axum::extract::multipart::MultipartError),
}

pub type Result<T> = std::result::Result<T, CoreError>;

impl From<CoreError> for axum::http::StatusCode {
    fn from(err: CoreError) -> Self {
        match err {
            CoreError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
            CoreError::Conflict(_) => axum::http::StatusCode::CONFLICT,
            CoreError::Unauthorized(_) => axum::http::StatusCode::UNAUTHORIZED,
            CoreError::Forbidden(_) => axum::http::StatusCode::FORBIDDEN,
            CoreError::Gone(_) => axum::http::StatusCode::GONE,
            CoreError::TemplateValidation(_) => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl axum::response::IntoResponse for CoreError {
    fn into_response(self) -> axum::response::Response {
        let message = self.to_string();
        let status = axum::http::StatusCode::from(self);
        let body = serde_json::json!({ "error": message });
        (status, Json(body)).into_response()
    }
}