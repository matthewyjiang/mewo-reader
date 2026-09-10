use crate::error::ApiError;
use crate::AppState;
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rand::RngCore;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Authed {
    pub user_id: String,
    pub username: String,
    pub token: String,
}

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| {
            tracing::error!(error = %err, "password hash failed");
            ApiError::Internal
        })
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, ApiError> {
    let parsed = PasswordHash::new(hash).map_err(|err| {
        tracing::error!(error = %err, "stored hash is not parseable");
        ApiError::Internal
    })?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn normalize_username(raw: &str) -> Result<(String, String), ApiError> {
    let username = raw.trim().to_string();
    if username.len() < 3 || username.len() > 32 {
        return Err(ApiError::BadRequest(
            "Username must be 3 to 32 characters.".into(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(ApiError::BadRequest(
            "Username can only use letters, digits, and underscore.".into(),
        ));
    }
    let norm = username.to_ascii_lowercase();
    Ok((username, norm))
}

pub fn check_password(raw: &str) -> Result<(), ApiError> {
    if raw.len() < 8 {
        return Err(ApiError::BadRequest(
            "Password must be at least 8 characters.".into(),
        ));
    }
    if raw.len() > 256 {
        return Err(ApiError::BadRequest("Password is too long.".into()));
    }
    Ok(())
}

impl FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("Sign in first.".into()))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("Sign in first.".into()))?;
        let row = sqlx::query_as::<_, Authed>(
            "SELECT users.id AS user_id, users.username, sessions.token AS token
             FROM sessions
             JOIN users ON users.id = sessions.user_id
             WHERE sessions.token = ?",
        )
        .bind(token)
        .fetch_optional(&state.pool)
        .await?;
        row.ok_or_else(|| ApiError::Unauthorized("Sign in first.".into()))
    }
}
