use crate::auth::{check_password, hash_password, normalize_username, Authed};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::models::{AdminUser, AdminUsers, PatchUser, ServerInfo};
use crate::settings::{self, AdminSettings};
use crate::AppState;
use axum::extract::{Extension, FromRequestParts, Path, Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::Json;
use axum::Router;
use serde_json::Value;
use sqlx::{FromRow, SqlitePool};
use std::path::Path as FsPath;
use uuid::Uuid;

/// A signed-in account with `users.is_admin = 1`. Only the guard below builds one.
#[derive(Clone, Debug)]
pub struct Admin {
    pub user_id: String,
    pub username: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdminChange {
    Changed,
    Unchanged,
}

/// Every `/v1/admin` route. The guard is a `route_layer` on this router, so a
/// handler added here is protected even if it never names `Admin`. A handler
/// that takes `Extension<Admin>` outside this router fails closed with 500.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/admin/settings",
            get(get_settings).patch(patch_settings),
        )
        .route("/v1/admin/users", get(list_users))
        .route("/v1/admin/users/{username}", patch(patch_user))
        .route("/v1/admin/server", get(server_info))
        .route("/v1/admin/requests/test", post(test_requests))
        .route_layer(middleware::from_fn(require_admin))
}

/// Runs `Authed`: missing or bad token → 401. Not an admin → 403 "Admins only.".
/// Inserts `Admin` into the request extensions. Reads `is_admin` on every request,
/// so a revoke (web or CLI) takes effect on the next call.
///
/// `from_fn` cannot extract [`State`], and this router is built before the pool
/// exists. `crate::router` puts [`AppState`] in the request extensions first.
async fn require_admin(request: Request, next: Next) -> Result<Response, ApiError> {
    let (mut parts, body) = request.into_parts();
    let state = parts
        .extensions
        .get::<AppState>()
        .cloned()
        .ok_or(ApiError::Internal)?;
    let authed = Authed::from_request_parts(&mut parts, &state).await?;
    if !authed.is_admin {
        return Err(ApiError::Forbidden("Admins only.".into()));
    }
    parts.extensions.insert(Admin {
        user_id: authed.user_id,
        username: authed.username,
    });
    Ok(next.run(Request::from_parts(parts, body)).await)
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<AdminSettings>, ApiError> {
    Ok(Json(with_downloads(
        settings::load_admin(&state.pool).await?,
        &state,
    )))
}

async fn patch_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<Admin>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<AdminSettings>, ApiError> {
    let Json(value) =
        body.map_err(|_| ApiError::BadRequest("Settings patch must be a JSON object.".into()))?;
    let Some(patch) = value.as_object().cloned() else {
        return Err(ApiError::BadRequest(
            "Settings patch must be a JSON object.".into(),
        ));
    };
    Ok(Json(with_downloads(
        settings::update(&state.pool, patch, &admin.user_id).await?,
        &state,
    )))
}

fn with_downloads(mut view: AdminSettings, state: &AppState) -> AdminSettings {
    view.downloads_dir = state
        .downloads_dir
        .as_ref()
        .map(|path| path.display().to_string());
    view
}

async fn test_requests(
    State(state): State<AppState>,
) -> Result<Json<crate::requests::Checks>, ApiError> {
    crate::requests::test_connection(&state).await
}

async fn list_users(
    State(state): State<AppState>,
    Extension(admin): Extension<Admin>,
) -> Result<Json<AdminUsers>, ApiError> {
    let rows = account_rows(&state.pool).await?;
    Ok(Json(AdminUsers {
        items: rows
            .into_iter()
            .map(|row| row.into_user(&admin.user_id))
            .collect(),
    }))
}

async fn patch_user(
    State(state): State<AppState>,
    Extension(admin): Extension<Admin>,
    Path(username): Path<String>,
    body: Result<Json<PatchUser>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<AdminUser>, ApiError> {
    let Json(body) =
        body.map_err(|_| ApiError::BadRequest("User patch must be a JSON object.".into()))?;
    let row = find_account(&state.pool, &username).await?;
    if row.id == admin.user_id {
        return Err(ApiError::Conflict(
            "You can't change your own admin access.".into(),
        ));
    }
    set_admin(&state.pool, &row.username, body.is_admin).await?;
    let row = find_account(&state.pool, &row.username).await?;
    Ok(Json(row.into_user(&admin.user_id)))
}

async fn server_info(State(state): State<AppState>) -> Result<Json<ServerInfo>, ApiError> {
    let counts = sqlx::query_as::<_, Counts>(
        "SELECT
            (SELECT COUNT(*) FROM users) AS users,
            (SELECT COUNT(*) FROM books) AS books,
            (SELECT COUNT(*) FROM comments) AS comments,
            (SELECT COUNT(*) FROM reposts) AS reposts",
    )
    .fetch_one(&state.pool)
    .await?;
    let data_dir = state.data_dir.clone();
    let storage_bytes = tokio::task::spawn_blocking(move || storage_bytes(&data_dir))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "storage walk failed");
            ApiError::Internal
        })?;
    Ok(Json(ServerInfo {
        users: counts.users,
        books: counts.books,
        comments: counts.comments,
        reposts: counts.reposts,
        storage_bytes,
    }))
}

/// The only writer of `users.is_admin`. Used by `patch_user` and the CLI.
/// Unknown username → [`ApiError::NotFound`]. Idempotent.
pub(crate) async fn set_admin(
    pool: &SqlitePool,
    username: &str,
    admin: bool,
) -> Result<AdminChange, ApiError> {
    let (_, norm) = normalize_username(username)?;
    let current = sqlx::query_as::<_, (String, i64)>(
        "SELECT username, is_admin FROM users WHERE username_norm = ?",
    )
    .bind(&norm)
    .fetch_optional(pool)
    .await?;
    let Some((_, is_admin)) = current else {
        return Err(ApiError::NotFound);
    };
    let next = i64::from(admin);
    if is_admin == next {
        return Ok(AdminChange::Unchanged);
    }
    sqlx::query("UPDATE users SET is_admin = ? WHERE username_norm = ?")
        .bind(next)
        .bind(&norm)
        .execute(pool)
        .await?;
    Ok(AdminChange::Changed)
}

/// Creates an account with `is_admin = 1`, using the same `normalize_username`,
/// `check_password` and `hash_password` as register. Ignores the signup setting.
/// A taken name → Conflict.
pub(crate) async fn create_admin(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> Result<(), ApiError> {
    let (username, norm) = normalize_username(username)?;
    check_password(password)?;
    let password_hash = hash_password(password)?;
    let id = Uuid::new_v4().to_string();
    let inserted = sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at, is_admin)
         VALUES (?, ?, ?, ?, ?, 1)",
    )
    .bind(&id)
    .bind(&username)
    .bind(&norm)
    .bind(&password_hash)
    .bind(now_ms())
    .execute(pool)
    .await;
    match inserted {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            Err(ApiError::Conflict("That username is taken.".into()))
        }
        Err(err) => Err(err.into()),
    }
}

/// `(username, is_admin)` for every account, by `created_at`.
pub(crate) async fn accounts(pool: &SqlitePool) -> Result<Vec<(String, bool)>, ApiError> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT username, is_admin FROM users ORDER BY created_at ASC, username ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(username, is_admin)| (username, is_admin != 0))
        .collect())
}

#[derive(Debug, FromRow)]
struct AccountRow {
    id: String,
    username: String,
    joined_at: i64,
    is_admin: bool,
    books: i64,
}

impl AccountRow {
    fn into_user(self, viewer_id: &str) -> AdminUser {
        AdminUser {
            username: self.username,
            joined_at: self.joined_at,
            is_admin: self.is_admin,
            books: self.books,
            mine: self.id == viewer_id,
        }
    }
}

#[derive(Debug, FromRow)]
struct Counts {
    users: i64,
    books: i64,
    comments: i64,
    reposts: i64,
}

async fn account_rows(pool: &SqlitePool) -> Result<Vec<AccountRow>, ApiError> {
    let rows = sqlx::query_as::<_, AccountRow>(
        "SELECT u.id AS id, u.username AS username, u.created_at AS joined_at,
                u.is_admin AS is_admin,
                (SELECT COUNT(*) FROM books b WHERE b.user_id = u.id) AS books
         FROM users u
         ORDER BY u.created_at ASC, u.username ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn find_account(pool: &SqlitePool, username: &str) -> Result<AccountRow, ApiError> {
    let (_, norm) = normalize_username(username)?;
    let row = sqlx::query_as::<_, AccountRow>(
        "SELECT u.id AS id, u.username AS username, u.created_at AS joined_at,
                u.is_admin AS is_admin,
                (SELECT COUNT(*) FROM books b WHERE b.user_id = u.id) AS books
         FROM users u
         WHERE u.username_norm = ?",
    )
    .bind(&norm)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ApiError::NotFound)
}

fn storage_bytes(data_dir: &FsPath) -> u64 {
    let mut total = file_size(&data_dir.join("mewo.db"));
    total = total.saturating_add(file_size(&data_dir.join("mewo.db-wal")));
    total.saturating_add(dir_size(&data_dir.join("books")))
}

fn file_size(path: &FsPath) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => meta.len(),
        _ => 0,
    }
}

fn dir_size(path: &FsPath) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path()));
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}
