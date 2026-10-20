//! Catalog search and book requests.
//!
//! Prowlarr and SABnzbd stay inside the worker. Handlers only see [`BookRequest`].

mod openlibrary;
mod pick;
mod prowlarr;
mod sabnzbd;
pub mod worker;

pub(crate) mod http;
pub(crate) use openlibrary::Limiter;

use crate::auth::Authed;
use crate::db::now_ms;
use crate::error::ApiError;
use crate::settings::{self, Settings};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use uuid::Uuid;

const ACTIVE_CAP: i64 = 5;
const MAX_QUERY_CHARS: usize = 100;
const MAX_TITLE: usize = 500;
const MAX_AUTHOR: usize = 300;
const MAX_WORK_KEY: usize = 200;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/catalog/search", get(catalog_search))
        .route("/v1/requests", get(list_requests).post(create_request))
        .route("/v1/requests/{id}", delete(cancel_request))
        .route("/v1/requests/{id}/retry", post(retry_request))
}

/// Catalog and requests are on. This does not look at secrets: `/v1/me` only
/// knows the master switch.
pub(crate) fn can_request(settings: &Settings) -> bool {
    settings.requests_enabled
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookRequest {
    pub id: String,
    pub work_key: String,
    pub title: String,
    pub author: String,
    pub year: Option<i64>,
    pub cover_id: Option<i64>,
    pub state: String,
    pub progress: i64,
    pub error: Option<String>,
    pub book_id: Option<String>,
    pub requester: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogWork {
    pub work_key: String,
    pub title: String,
    pub author: String,
    pub year: Option<i64>,
    pub cover_id: Option<i64>,
    pub request: Option<BookRequest>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPage {
    items: Vec<CatalogWork>,
    next_page: Option<i64>,
}

#[derive(Serialize)]
struct RequestList {
    items: Vec<BookRequest>,
}

#[derive(Debug, sqlx::FromRow)]
struct RequestRow {
    id: String,
    user_id: String,
    work_key: String,
    title: String,
    author: String,
    year: Option<i64>,
    cover_id: Option<i64>,
    state: String,
    progress: i64,
    error: Option<String>,
    book_id: Option<String>,
    download_id: Option<String>,
    requester: String,
    created_at: i64,
    updated_at: i64,
}

impl RequestRow {
    fn into_wire(self) -> BookRequest {
        BookRequest {
            id: self.id,
            work_key: self.work_key,
            title: self.title,
            author: self.author,
            year: self.year,
            cover_id: self.cover_id,
            state: self.state,
            progress: self.progress,
            error: self.error,
            book_id: self.book_id,
            requester: self.requester,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

const REQUEST_COLUMNS: &str =
    "SELECT r.id, r.user_id, r.work_key, r.title, r.author, r.year, r.cover_id,
        r.state, r.progress, r.error, r.book_id, r.download_id, u.username AS requester,
        r.created_at, r.updated_at
     FROM book_requests r
     JOIN users u ON u.id = r.user_id";

#[derive(Deserialize)]
struct CatalogQuery {
    q: String,
    #[serde(default = "first_page")]
    page: i64,
}

fn first_page() -> i64 {
    1
}

async fn catalog_search(
    State(state): State<AppState>,
    authed: Authed,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<CatalogPage>, ApiError> {
    ensure_on(&state).await?;
    let q = query.q.trim();
    if q.is_empty() {
        return Err(ApiError::BadRequest("Enter a search.".into()));
    }
    let asked = q.chars().count();
    if asked > MAX_QUERY_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Search is longer than {MAX_QUERY_CHARS} characters (asked {asked})."
        )));
    }
    let page = query.page.max(1);
    let (works, next_page) = openlibrary::search(&state, q, page)
        .await
        .map_err(|err| ApiError::BadRequest(err.to_string()))?;
    let mine = mine_by_work(&state.pool, &authed.user_id).await?;
    let items = works
        .into_iter()
        .map(|work| CatalogWork {
            request: mine.get(&work.work_key).cloned(),
            work_key: work.work_key,
            title: work.title,
            author: work.author,
            year: work.year,
            cover_id: work.cover_id,
        })
        .collect();
    Ok(Json(CatalogPage { items, next_page }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBody {
    work_key: String,
    title: String,
    author: String,
    year: Option<i64>,
    cover_id: Option<i64>,
}

async fn create_request(
    State(state): State<AppState>,
    authed: Authed,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<BookRequest>), ApiError> {
    ensure_on(&state).await?;
    let work_key = clean_work_key(&body.work_key)?;
    let title = required(
        &body.title,
        MAX_TITLE,
        "Enter a title.",
        "Title is too long.",
    )?;
    let author = required(
        &body.author,
        MAX_AUTHOR,
        "Enter an author.",
        "Author is too long.",
    )?;
    if body.cover_id.is_some_and(|id| id < 0) {
        return Err(ApiError::BadRequest("coverId must be 0 or more.".into()));
    }
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    if let Some(existing) = active_for_work(&mut *tx, &authed.user_id, &work_key).await? {
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(existing)));
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND state IN ('requested', 'downloading')",
    )
    .bind(&authed.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if active >= ACTIVE_CAP {
        return Err(ApiError::Conflict(
            "You have 5 requests in progress.".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    let inserted = sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, year, cover_id, state, next_check_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'requested', ?, ?, ?)",
    )
    .bind(&id)
    .bind(&authed.user_id)
    .bind(&work_key)
    .bind(&title)
    .bind(&author)
    .bind(body.year)
    .bind(body.cover_id)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            drop(tx);
            let Some(existing) = active_for_work(&state.pool, &authed.user_id, &work_key).await?
            else {
                return Err(ApiError::Conflict(
                    "You have 5 requests in progress.".into(),
                ));
            };
            return Ok((StatusCode::OK, Json(existing)));
        }
        Err(err) => return Err(err.into()),
    }
    tx.commit().await?;
    state.worker.notify_one();
    let row = fetch(&state.pool, &id).await?.ok_or(ApiError::Internal)?;
    Ok((StatusCode::CREATED, Json(row.into_wire())))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    all: i64,
}

async fn list_requests(
    State(state): State<AppState>,
    authed: Authed,
    Query(query): Query<ListQuery>,
) -> Result<Json<RequestList>, ApiError> {
    ensure_on(&state).await?;
    let everyone = query.all == 1;
    if everyone && !authed.is_admin {
        return Err(ApiError::Forbidden("Admins only.".into()));
    }
    let sql = if everyone {
        format!("{REQUEST_COLUMNS} ORDER BY r.created_at DESC, r.id DESC")
    } else {
        format!("{REQUEST_COLUMNS} WHERE r.user_id = ? ORDER BY r.created_at DESC, r.id DESC")
    };
    let mut rows = sqlx::query_as::<_, RequestRow>(&sql);
    if !everyone {
        rows = rows.bind(&authed.user_id);
    }
    let items = rows
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(RequestRow::into_wire)
        .collect();
    Ok(Json(RequestList { items }))
}

async fn cancel_request(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    ensure_on(&state).await?;
    let row = owned_row(&state, &authed, &id).await?;
    if row.state == "canceled" {
        return Ok(StatusCode::NO_CONTENT);
    }
    if row.state == "imported" {
        return Err(ApiError::Conflict("That book is on your shelf.".into()));
    }
    let now = now_ms();
    let changed = sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', error = NULL, download_id = NULL, updated_at = ?, next_check_at = ?
         WHERE id = ? AND user_id = ? AND state IN ('requested', 'downloading', 'failed')",
    )
    .bind(now)
    .bind(now)
    .bind(&row.id)
    .bind(&row.user_id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if changed == 0 {
        let again = fetch(&state.pool, &row.id).await?;
        if again.as_ref().is_some_and(|row| row.state == "canceled") {
            return Ok(StatusCode::NO_CONTENT);
        }
        if again.as_ref().is_some_and(|row| row.state == "imported") {
            return Err(ApiError::Conflict("That book is on your shelf.".into()));
        }
        return Err(ApiError::NotFound);
    }
    if let Some(nzo) = row.download_id {
        delete_job(&state, &nzo).await;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn retry_request(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<BookRequest>, ApiError> {
    ensure_on(&state).await?;
    let row = owned_row(&state, &authed, &id).await?;
    if row.state != "failed" && row.state != "canceled" {
        return Err(ApiError::Conflict(
            "Only a failed or canceled request can be retried.".into(),
        ));
    }
    let now = now_ms();
    let changed = sqlx::query(
        "UPDATE book_requests
         SET state = 'requested', error = NULL, next_check_at = ?, attempts = 0,
             tried_guids = '[]', download_id = NULL, progress = 0,
             release_title = NULL, release_size = NULL, indexer = NULL, updated_at = ?
         WHERE id = ? AND user_id = ? AND state IN ('failed', 'canceled')",
    )
    .bind(now)
    .bind(now)
    .bind(&row.id)
    .bind(&row.user_id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(ApiError::Conflict(
            "Only a failed or canceled request can be retried.".into(),
        ));
    }
    state.worker.notify_one();
    let row = fetch(&state.pool, &row.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(row.into_wire()))
}

async fn owned_row(state: &AppState, authed: &Authed, id: &str) -> Result<RequestRow, ApiError> {
    let Some(row) = fetch(&state.pool, id).await? else {
        return Err(ApiError::NotFound);
    };
    if row.user_id != authed.user_id && !authed.is_admin {
        return Err(ApiError::NotFound);
    }
    Ok(row)
}

async fn ensure_on(state: &AppState) -> Result<(), ApiError> {
    let settings = settings::load(&state.pool).await?;
    if can_request(&settings) {
        Ok(())
    } else {
        Err(ApiError::Forbidden("Requests are off.".into()))
    }
}

fn clean_work_key(raw: &str) -> Result<String, ApiError> {
    let key = raw.trim();
    let ok = key.starts_with("/works/")
        && key.len() > "/works/".len()
        && key.chars().count() <= MAX_WORK_KEY
        && !key.chars().any(char::is_whitespace);
    if ok {
        Ok(key.to_string())
    } else {
        Err(ApiError::BadRequest(
            "workKey must start with \"/works/\".".into(),
        ))
    }
}

fn required(raw: &str, max: usize, empty: &str, long: &str) -> Result<String, ApiError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(ApiError::BadRequest(empty.into()));
    }
    if value.chars().count() > max {
        return Err(ApiError::BadRequest(long.into()));
    }
    Ok(value.to_string())
}

async fn active_for_work<'a, E>(
    executor: E,
    user_id: &str,
    work_key: &str,
) -> Result<Option<BookRequest>, ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let sql = format!(
        "{REQUEST_COLUMNS}
         WHERE r.user_id = ? AND r.work_key = ? AND r.state IN ('requested', 'downloading')
         ORDER BY r.created_at DESC
         LIMIT 1"
    );
    let row = sqlx::query_as::<_, RequestRow>(&sql)
        .bind(user_id)
        .bind(work_key)
        .fetch_optional(executor)
        .await?;
    Ok(row.map(RequestRow::into_wire))
}

async fn fetch(pool: &SqlitePool, id: &str) -> Result<Option<RequestRow>, ApiError> {
    let sql = format!("{REQUEST_COLUMNS} WHERE r.id = ?");
    Ok(sqlx::query_as::<_, RequestRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?)
}

async fn mine_by_work(
    pool: &SqlitePool,
    user_id: &str,
) -> Result<HashMap<String, BookRequest>, ApiError> {
    let sql =
        format!("{REQUEST_COLUMNS} WHERE r.user_id = ? ORDER BY r.created_at DESC, r.id DESC");
    let rows = sqlx::query_as::<_, RequestRow>(&sql)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let mut by_work = HashMap::new();
    for row in rows {
        let key = row.work_key.clone();
        by_work.entry(key).or_insert_with(|| row.into_wire());
    }
    Ok(by_work)
}

async fn delete_job(state: &AppState, nzo: &str) {
    let Ok(settings) = settings::load(&state.pool).await else {
        return;
    };
    let Ok(Some(key)) = settings::secret(&state.pool, settings::Secret::SabnzbdApiKey).await else {
        return;
    };
    let Ok(base) = http::base_url(&settings.sabnzbd_url) else {
        return;
    };
    let sab = sabnzbd::Sabnzbd {
        up: &state.upstream,
        base,
        key,
        category: settings.sabnzbd_category.clone(),
    };
    if let Err(err) = sab.delete(nzo).await {
        tracing::error!(error = %err, "sab delete on cancel failed");
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Checks {
    pub checks: Vec<Check>,
}

/// Three checks. Always 200; a failed check is `ok: false` and `detail` is the
/// safe error text, never a body or a URL.
pub(crate) async fn test_connection(state: &AppState) -> Result<Json<Checks>, ApiError> {
    let settings = settings::load(&state.pool).await?;
    let prowlarr_key = settings::secret(&state.pool, settings::Secret::ProwlarrApiKey).await?;
    let sab_key = settings::secret(&state.pool, settings::Secret::SabnzbdApiKey).await?;
    let prowlarr = match (http::base_url(&settings.prowlarr_url), prowlarr_key) {
        (Ok(base), Some(key)) => {
            let client = prowlarr::Prowlarr {
                up: &state.upstream,
                base,
                key,
            };
            match client.status().await {
                Ok(detail) => Check {
                    name: "prowlarr".into(),
                    ok: true,
                    detail,
                },
                Err(err) => failed_check("prowlarr", &err.to_string()),
            }
        }
        (Err(err), _) => failed_check("prowlarr", &err.to_string()),
        (_, None) => failed_check("prowlarr", "not configured"),
    };
    let sabnzbd = match (http::base_url(&settings.sabnzbd_url), sab_key) {
        (Ok(base), Some(key)) => {
            let client = sabnzbd::Sabnzbd {
                up: &state.upstream,
                base,
                key,
                category: settings.sabnzbd_category.clone(),
            };
            match client.status().await {
                Ok(detail) => Check {
                    name: "sabnzbd".into(),
                    ok: true,
                    detail,
                },
                Err(err) => failed_check("sabnzbd", &err.to_string()),
            }
        }
        (Err(err), _) => failed_check("sabnzbd", &err.to_string()),
        (_, None) => failed_check("sabnzbd", "not configured"),
    };
    let downloads = downloads_check(state.downloads_dir.as_deref()).await;
    Ok(Json(Checks {
        checks: vec![prowlarr, sabnzbd, downloads],
    }))
}

fn failed_check(name: &str, detail: &str) -> Check {
    Check {
        name: name.into(),
        ok: false,
        detail: detail.into(),
    }
}

async fn downloads_check(dir: Option<&std::path::Path>) -> Check {
    let Some(dir) = dir else {
        return failed_check("downloads", "Not set");
    };
    match tokio::fs::read_dir(dir).await {
        Ok(_) => Check {
            name: "downloads".into(),
            ok: true,
            detail: "readable".into(),
        },
        Err(_) => Check {
            name: "downloads".into(),
            ok: false,
            detail: format!("{} is not readable", dir.display()),
        },
    }
}
