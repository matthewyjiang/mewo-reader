pub mod auth;
pub mod config;
pub mod db;
pub mod epub;
pub mod error;
pub mod feed;
pub mod models;
pub mod routes;

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_epub;

use crate::config::{request_body_limit, Config};
use axum::extract::DefaultBodyLimit;
use axum::routing::{any, get, post};
use axum::Router;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: PathBuf,
    pub web_dir: Option<PathBuf>,
    /// Serializes legacy cache fills. Normal reads/uploads do not take this lock.
    pub lazy_parse_lock: Arc<Mutex<()>>,
    pub allow_signup: bool,
    pub max_epub_bytes: u64,
}

pub fn router(state: AppState) -> Router {
    let body_limit = request_body_limit(state.max_epub_bytes);
    let web_dir = state.web_dir.clone().filter(|dir| dir.is_dir());
    let api = Router::new()
        .route("/v1/auth/register", post(routes::register))
        .route("/v1/auth/login", post(routes::login))
        .route("/v1/auth/logout", post(routes::logout))
        .route("/v1/me", get(routes::me))
        .route("/v1/library", get(routes::library))
        .route("/v1/books", post(routes::upload_book))
        .route(
            "/v1/books/{id}",
            get(routes::get_book)
                .delete(routes::delete_book)
                .patch(routes::patch_book),
        )
        .route("/v1/books/{id}/epub", get(routes::download_epub))
        .route("/v1/books/{id}/posts", get(routes::get_posts))
        .route("/v1/books/{id}/cover", get(routes::download_cover))
        .route("/v1/books/{id}/likes", get(routes::list_likes))
        .route("/v1/books/{id}/likes/{post_id}", post(routes::toggle_like))
        .route("/v1/books/{id}/comments", get(routes::comment_index))
        .route(
            "/v1/books/{id}/comments/{post_id}",
            get(routes::list_comments).post(routes::add_comment),
        )
        .route(
            "/v1/books/{id}/comments/{post_id}/{comment_id}",
            axum::routing::delete(routes::delete_comment),
        )
        .route(
            "/v1/profiles/{username}/replies",
            get(routes::profile_replies),
        )
        // Never let history-routing fallback swallow unknown API routes.
        .route("/v1", any(api_not_found))
        .route("/v1/", any(api_not_found))
        .route("/v1/{*path}", any(api_not_found))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let app = if let Some(dir) = web_dir {
        api.fallback_service(ServeDir::new(&dir).fallback(ServeFile::new(dir.join("index.html"))))
    } else {
        api.fallback(api_not_found)
    };
    // War and Peace posts are 4_022_093 bytes of JSON and 1_263_141 gzipped.
    // tower-http skips images and bodies under 32 bytes on its own.
    app.layer(CompressionLayer::new())
}

async fn api_not_found() -> error::ApiError {
    error::ApiError::NotFound
}

pub async fn build(config: Config) -> Result<Router, sqlx::Error> {
    tokio::fs::create_dir_all(&config.data_dir)
        .await
        .map_err(sqlx::Error::Io)?;
    let data_dir = config.data_dir.canonicalize().map_err(sqlx::Error::Io)?;
    let pool = db::connect(&config.database_url(), &data_dir).await?;
    Ok(router(AppState {
        pool,
        data_dir,
        web_dir: config.web_dir,
        lazy_parse_lock: Arc::new(Mutex::new(())),
        allow_signup: config.allow_signup,
        max_epub_bytes: config.max_epub_bytes,
    }))
}
