pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod routes;

use crate::config::{request_body_limit, Config};
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: PathBuf,
    pub allow_signup: bool,
    pub max_epub_bytes: u64,
}

pub fn router(state: AppState) -> Router {
    let body_limit = request_body_limit(state.max_epub_bytes);
    Router::new()
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
        .route("/v1/books/{id}/cover", get(routes::download_cover))
        .route("/v1/books/{id}/likes", get(routes::list_likes))
        .route(
            "/v1/books/{id}/likes/{post_id}",
            post(routes::toggle_like),
        )
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
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
        allow_signup: config.allow_signup,
        max_epub_bytes: config.max_epub_bytes,
    }))
}
