//! Catalog search and book requests.
//!
//! Prowlarr and SABnzbd stay inside the worker. Handlers only see [`BookRequest`].

pub(crate) mod languages;
mod matching;
mod openlibrary;
mod pick;
mod prowlarr;
mod sabnzbd;
pub mod worker;

pub(crate) mod http;
pub(crate) use openlibrary::{Limiter, WorkCache};

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
    /// Leader's username. Set only on `GET /v1/requests?all=1`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,
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
    follows: Option<String>,
    lead_state: Option<String>,
    lead_progress: Option<i64>,
    lead_error: Option<String>,
    lead_requester: Option<String>,
}

impl RequestRow {
    /// A follower's wire state, progress, and error are the leader's. `show_leader`
    /// adds the leader's username for the admin list.
    fn into_wire(self, show_leader: bool) -> BookRequest {
        let following = self.lead_state.is_some();
        BookRequest {
            id: self.id,
            work_key: self.work_key,
            title: self.title,
            author: self.author,
            year: self.year,
            cover_id: self.cover_id,
            state: if following {
                self.lead_state.clone().unwrap_or(self.state)
            } else {
                self.state
            },
            progress: if following {
                self.lead_progress.unwrap_or(self.progress)
            } else {
                self.progress
            },
            error: if following {
                self.lead_error
            } else {
                self.error
            },
            book_id: self.book_id,
            requester: self.requester,
            created_at: self.created_at,
            updated_at: self.updated_at,
            follows: if show_leader {
                self.lead_requester
            } else {
                None
            },
        }
    }
}

const REQUEST_COLUMNS: &str =
    "SELECT r.id, r.user_id, r.work_key, r.title, r.author, r.year, r.cover_id,
        r.state, r.progress, r.error, r.book_id, r.download_id, u.username AS requester,
        r.created_at, r.updated_at, r.follows AS follows,
        lead.state AS lead_state, lead.progress AS lead_progress, lead.error AS lead_error,
        lead_user.username AS lead_requester
     FROM book_requests r
     JOIN users u ON u.id = r.user_id
     LEFT JOIN book_requests lead ON lead.id = r.follows
     LEFT JOIN users lead_user ON lead_user.id = lead.user_id";

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
        .map_err(|err| ApiError::BadGateway(err.to_string()))?;
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
    let asked = clean_work_key(&body.work_key)?;
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
    let cached = state.work_cache.cached_canonical(&asked);
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let admitted = admit(
        &mut tx,
        &authed.user_id,
        &asked,
        cached.as_deref(),
        &title,
        &author,
        body.year,
        body.cover_id,
    )
    .await?;
    tx.commit().await?;
    let (status, request) = match admitted {
        Admitted::Existing(request) => (StatusCode::OK, request),
        Admitted::Fresh { id, wake } => {
            if wake {
                state.worker.notify_one();
            }
            let row = fetch(&state.pool, &id).await?.ok_or(ApiError::Internal)?;
            (StatusCode::CREATED, row.into_wire(false))
        }
    };
    Ok((status, Json(request)))
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
        .map(|row| row.into_wire(everyone))
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
    // A follower has no SABnzbd job of its own. A leader's job is deleted
    // after the hand-off, so the unique index sees the leader leave first.
    let leading =
        row.follows.is_none() && matches!(row.state.as_str(), "requested" | "downloading");
    let now = now_ms();
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let changed = sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', error = NULL, download_id = NULL, updated_at = ?, next_check_at = ?
         WHERE id = ? AND user_id = ? AND state IN ('requested', 'downloading', 'failed')",
    )
    .bind(now)
    .bind(now)
    .bind(&row.id)
    .bind(&row.user_id)
    .execute(&mut *tx)
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
    if leading {
        hand_off(&mut tx, &row.id, now).await?;
    }
    tx.commit().await?;
    if leading {
        if let Some(nzo) = row.download_id {
            delete_job(&state, &nzo).await;
        }
        state.worker.notify_one();
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
    let cached = state.work_cache.cached_canonical(&row.work_key);
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let work_key = resolve_key(&mut *tx, &row.work_key, cached.as_deref()).await?;
    let wake = requeue(&mut tx, &row, &work_key).await?;
    tx.commit().await?;
    if wake {
        state.worker.notify_one();
    }
    let row = fetch(&state.pool, &row.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(row.into_wire(false)))
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

async fn ensure_under_cap<'a, E>(executor: E, user_id: &str) -> Result<(), ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    // Followers and instant reuses take no indexer or SABnzbd slot.
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND state IN ('requested', 'downloading') AND follows IS NULL",
    )
    .bind(user_id)
    .fetch_one(executor)
    .await?;
    if active >= ACTIVE_CAP {
        return Err(ApiError::Conflict(
            "You have 5 requests in progress.".into(),
        ));
    }
    Ok(())
}

enum Admitted {
    Existing(BookRequest),
    Fresh { id: String, wake: bool },
}

/// Reuse an imported copy, follow the in-flight leader, or become the leader.
/// The caller's own active row is returned as it stands.
async fn admit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    asked: &str,
    cached: Option<&str>,
    title: &str,
    author: &str,
    year: Option<i64>,
    cover_id: Option<i64>,
) -> Result<Admitted, ApiError> {
    let work_key = resolve_key(&mut **tx, asked, cached).await?;
    if let Some(existing) = active_for_work(&mut **tx, user_id, &work_key).await? {
        return Ok(Admitted::Existing(existing));
    }
    if work_key != asked {
        if let Some(existing) = active_for_work(&mut **tx, user_id, asked).await? {
            return Ok(Admitted::Existing(existing));
        }
    }
    if let Some(book_id) = reusable_book(&mut **tx, &work_key).await? {
        if let Some(existing) = own_imported(&mut **tx, user_id, &work_key, &book_id).await? {
            return Ok(Admitted::Existing(existing));
        }
        let id = insert_imported(
            tx, user_id, &work_key, title, author, year, cover_id, &book_id,
        )
        .await?;
        return Ok(Admitted::Fresh { id, wake: false });
    }
    if let Some(leader) = leader_id(&mut **tx, &work_key).await? {
        return follow(
            tx, user_id, &work_key, title, author, year, cover_id, &leader,
        )
        .await;
    }
    ensure_under_cap(&mut **tx, user_id).await?;
    match insert_leader(tx, user_id, &work_key, title, author, year, cover_id).await {
        Ok(id) => Ok(Admitted::Fresh { id, wake: true }),
        Err(err) if leader_conflict(&err) => {
            let Some(leader) = leader_id(&mut **tx, &work_key).await? else {
                return Err(err.into());
            };
            follow(
                tx, user_id, &work_key, title, author, year, cover_id, &leader,
            )
            .await
        }
        Err(err) if user_conflict(&err) => {
            if let Some(existing) = active_for_work(&mut **tx, user_id, &work_key).await? {
                return Ok(Admitted::Existing(existing));
            }
            Err(ApiError::Conflict(
                "You have 5 requests in progress.".into(),
            ))
        }
        Err(err) => Err(err.into()),
    }
}

/// A cached redirect is used only when nothing is already stored under the
/// asked key. The worker caches the key it was given, so rewriting that key
/// would miss the row and start a second download.
fn choose_key(asked: &str, cached: Option<&str>, asked_taken: bool) -> String {
    if asked_taken {
        return asked.to_string();
    }
    match cached {
        Some(key) if key != asked && clean_work_key(key).is_ok() => key.to_string(),
        _ => asked.to_string(),
    }
}

async fn resolve_key<'a, E>(
    executor: E,
    asked: &str,
    cached: Option<&str>,
) -> Result<String, ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let taken: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM book_requests
         WHERE work_key = ? AND state IN ('requested', 'downloading', 'imported')
         LIMIT 1",
    )
    .bind(asked)
    .fetch_optional(executor)
    .await?;
    Ok(choose_key(asked, cached, taken.is_some()))
}

async fn reusable_book<'a, E>(executor: E, work_key: &str) -> Result<Option<String>, ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let book_id: Option<String> = sqlx::query_scalar(
        "SELECT r.book_id FROM book_requests r
         JOIN books b ON b.id = r.book_id
         WHERE r.work_key = ? AND r.state = 'imported' AND r.book_id IS NOT NULL
         ORDER BY r.updated_at
         LIMIT 1",
    )
    .bind(work_key)
    .fetch_optional(executor)
    .await?;
    Ok(book_id)
}

async fn own_imported<'a, E>(
    executor: E,
    user_id: &str,
    work_key: &str,
    book_id: &str,
) -> Result<Option<BookRequest>, ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let sql = format!(
        "{REQUEST_COLUMNS}
         WHERE r.user_id = ? AND r.state = 'imported'
           AND (r.work_key = ? OR r.book_id = ?)
         ORDER BY r.updated_at DESC
         LIMIT 1"
    );
    let row = sqlx::query_as::<_, RequestRow>(&sql)
        .bind(user_id)
        .bind(work_key)
        .bind(book_id)
        .fetch_optional(executor)
        .await?;
    Ok(row.map(|row| row.into_wire(false)))
}

async fn leader_id<'a, E>(executor: E, work_key: &str) -> Result<Option<String>, ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM book_requests
         WHERE work_key = ? AND follows IS NULL AND state IN ('requested', 'downloading')
         LIMIT 1",
    )
    .bind(work_key)
    .fetch_optional(executor)
    .await?;
    Ok(id)
}

async fn insert_imported(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    work_key: &str,
    title: &str,
    author: &str,
    year: Option<i64>,
    cover_id: Option<i64>,
    book_id: &str,
) -> Result<String, ApiError> {
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, year, cover_id, state, progress, book_id,
          next_check_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'imported', 100, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(work_key)
    .bind(title)
    .bind(author)
    .bind(year)
    .bind(cover_id)
    .bind(book_id)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn follow(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    work_key: &str,
    title: &str,
    author: &str,
    year: Option<i64>,
    cover_id: Option<i64>,
    leader: &str,
) -> Result<Admitted, ApiError> {
    match insert_follower(tx, user_id, work_key, title, author, year, cover_id, leader).await {
        Ok(id) => Ok(Admitted::Fresh { id, wake: false }),
        Err(err) if user_conflict(&err) => {
            if let Some(existing) = active_for_work(&mut **tx, user_id, work_key).await? {
                return Ok(Admitted::Existing(existing));
            }
            Err(err.into())
        }
        Err(err) => Err(err.into()),
    }
}

async fn insert_follower(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    work_key: &str,
    title: &str,
    author: &str,
    year: Option<i64>,
    cover_id: Option<i64>,
    leader: &str,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, year, cover_id, state, follows,
          next_check_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'requested', ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(work_key)
    .bind(title)
    .bind(author)
    .bind(year)
    .bind(cover_id)
    .bind(leader)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn insert_leader(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    work_key: &str,
    title: &str,
    author: &str,
    year: Option<i64>,
    cover_id: Option<i64>,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, year, cover_id, state, next_check_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'requested', ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(work_key)
    .bind(title)
    .bind(author)
    .bind(year)
    .bind(cover_id)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

/// Failed or canceled row becomes a reuse, a follower, or a leader.
/// `true` when the worker should wake (this row is now the leader).
async fn requeue(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &RequestRow,
    work_key: &str,
) -> Result<bool, ApiError> {
    let now = now_ms();
    if let Some(book_id) = reusable_book(&mut **tx, work_key).await? {
        let changed = sqlx::query(
            "UPDATE book_requests
             SET state = 'imported', book_id = ?, progress = 100, error = NULL, follows = NULL,
                 download_id = NULL, work_key = ?, next_check_at = ?, updated_at = ?
             WHERE id = ? AND user_id = ? AND state IN ('failed', 'canceled')",
        )
        .bind(&book_id)
        .bind(work_key)
        .bind(now)
        .bind(now)
        .bind(&row.id)
        .bind(&row.user_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if changed == 0 {
            return Err(ApiError::Conflict(
                "Only a failed or canceled request can be retried.".into(),
            ));
        }
        return Ok(false);
    }
    if let Some(leader) = leader_id(&mut **tx, work_key).await? {
        if leader != row.id {
            become_follower(tx, row, work_key, &leader, now).await?;
            return Ok(false);
        }
    }
    // The owner's cap, even when an admin presses retry. A follower does not
    // spend a slot, and neither does a reuse.
    ensure_under_cap(&mut **tx, &row.user_id).await?;
    match become_leader(tx, row, work_key, now).await {
        Ok(true) => Ok(true),
        Ok(false) => Err(ApiError::Conflict(
            "Only a failed or canceled request can be retried.".into(),
        )),
        Err(err) if leader_conflict(&err) => {
            let Some(leader) = leader_id(&mut **tx, work_key).await? else {
                return Err(err.into());
            };
            become_follower(tx, row, work_key, &leader, now).await?;
            Ok(false)
        }
        Err(err) if user_conflict(&err) => Err(ApiError::Conflict(
            "You already have a request for this book.".into(),
        )),
        Err(err) => Err(err.into()),
    }
}

async fn become_follower(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &RequestRow,
    work_key: &str,
    leader: &str,
    now: i64,
) -> Result<(), ApiError> {
    let changed = sqlx::query(
        "UPDATE book_requests
         SET state = 'requested', follows = ?, error = NULL, progress = 0, attempts = 0,
             tried_guids = '[]', download_id = NULL, release_title = NULL, release_size = NULL,
             indexer = NULL, work_key = ?, next_check_at = ?, updated_at = ?
         WHERE id = ? AND user_id = ? AND state IN ('failed', 'canceled')",
    )
    .bind(leader)
    .bind(work_key)
    .bind(now)
    .bind(now)
    .bind(&row.id)
    .bind(&row.user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(ApiError::Conflict(
            "Only a failed or canceled request can be retried.".into(),
        ));
    }
    Ok(())
}

async fn become_leader(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    row: &RequestRow,
    work_key: &str,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let changed = sqlx::query(
        "UPDATE book_requests
         SET state = 'requested', follows = NULL, error = NULL, next_check_at = ?, attempts = 0,
             tried_guids = '[]', download_id = NULL, progress = 0,
             release_title = NULL, release_size = NULL, indexer = NULL, work_key = ?, updated_at = ?
         WHERE id = ? AND user_id = ? AND state IN ('failed', 'canceled')",
    )
    .bind(now)
    .bind(work_key)
    .bind(now)
    .bind(&row.id)
    .bind(&row.user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(changed == 1)
}

fn leader_conflict(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            let msg = db.message();
            msg.contains("work_key") && !msg.contains("user_id")
        }
        _ => false,
    }
}

fn user_conflict(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) if db.is_unique_violation() => db.message().contains("user_id"),
        _ => false,
    }
}

/// The oldest follower becomes the leader. The caller's UPDATE of `leader_id`
/// has to have left the active-leader index already; this fails if it has not.
pub(crate) async fn hand_off(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    leader_id: &str,
    now: i64,
) -> Result<(), ApiError> {
    let promoted: Option<String> = sqlx::query_scalar(
        "SELECT id FROM book_requests
         WHERE follows = ? AND state = 'requested'
         ORDER BY created_at ASC, id ASC
         LIMIT 1",
    )
    .bind(leader_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(promoted) = promoted else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE book_requests
         SET follows = NULL, state = 'requested', attempts = 0, error = NULL, progress = 0,
             tried_guids = (SELECT tried_guids FROM book_requests WHERE id = ?),
             next_check_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(leader_id)
    .bind(now)
    .bind(now)
    .bind(&promoted)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE book_requests SET follows = ? WHERE follows = ? AND state = 'requested'")
        .bind(&promoted)
        .bind(leader_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Called in the same transaction that deletes the book. Compare-and-set so
/// only an `imported` request moves to `canceled`; the foreign key does not
/// change state.
pub(crate) async fn cancel_imported<'a, E>(
    executor: E,
    user_id: &str,
    book_id: &str,
) -> Result<(), ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let now = now_ms();
    sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', book_id = NULL, error = NULL, download_id = NULL,
             updated_at = ?, next_check_at = ?
         WHERE book_id = ? AND user_id = ? AND state = 'imported'",
    )
    .bind(now)
    .bind(now)
    .bind(book_id)
    .bind(user_id)
    .execute(executor)
    .await?;
    Ok(())
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
    Ok(row.map(|row| row.into_wire(false)))
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
        by_work.entry(key).or_insert_with(|| row.into_wire(false));
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

#[cfg(test)]
mod tests {
    use super::choose_key;

    #[test]
    fn a_cached_redirect_is_kept_when_the_asked_key_is_already_stored() {
        assert_eq!(
            choose_key("/works/OL893415W", Some("/works/OL893414W"), true),
            "/works/OL893415W"
        );
        assert_eq!(
            choose_key("/works/OL893415W", Some("/works/OL893414W"), false),
            "/works/OL893414W"
        );
        assert_eq!(
            choose_key("/works/OL893415W", None, false),
            "/works/OL893415W"
        );
        assert_eq!(
            choose_key("/works/OL893415W", Some("not a key"), false),
            "/works/OL893415W"
        );
    }
}
