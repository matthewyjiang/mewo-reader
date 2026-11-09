//! A reader says the imported file is the wrong book.
//!
//! The release goes on the blocklist, every hold of that copy is canceled, and
//! the same people get one new request: the reporter leads, the others follow.
//! The new copy keeps the sharing the old one had. Nothing is deleted when
//! requests are off, the caller cannot act, or the new request cannot start.

use crate::auth::Authed;
use crate::db::now_ms;
use crate::error::ApiError;
use crate::library;
use crate::requests::pick::title_norm;
use crate::AppState;
use sqlx::{Connection, SqlitePool};
use uuid::Uuid;

use super::{aliases, BookRequest};

const ACTIVE_CAP: i64 = 5;

pub(crate) async fn wrong_file(
    state: &AppState,
    authed: &Authed,
    book_id: &str,
) -> Result<BookRequest, ApiError> {
    let settings = crate::settings::load(&state.pool).await?;
    if !super::can_request(&settings) {
        return Err(ApiError::Forbidden("Requests are off.".into()));
    }
    let Some(book) = load_book(&state.pool, book_id).await? else {
        return Err(ApiError::NotFound);
    };
    if book.owner_id != authed.user_id && !authed.is_admin {
        return Err(ApiError::Forbidden(
            "Only the person who added this book can report a wrong file.".into(),
        ));
    }
    let Some(source) = load_source(&state.pool, book_id).await? else {
        return Err(ApiError::Conflict(
            "This book has no release to block.".into(),
        ));
    };
    let Some(guid) = last_guid(&source.tried_guids) else {
        return Err(ApiError::Conflict(
            "This book has no release to block.".into(),
        ));
    };
    let Some(release_title) = source.release_title.clone() else {
        return Err(ApiError::Conflict(
            "This book has no release to block.".into(),
        ));
    };
    let Some(release_size) = source.release_size else {
        return Err(ApiError::Conflict(
            "This book has no release to block.".into(),
        ));
    };
    let canonical = aliases::canonical(state, &source.work_key).await;
    let mut conn = state.pool.acquire().await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let keys = aliases::group(&mut *tx, &canonical).await?;
    if active_for(&mut tx, &authed.user_id, &keys.all).await? {
        return Err(ApiError::Conflict(
            "You already have a request for this book.".into(),
        ));
    }
    let existing = leader_of(&mut tx, &keys.all).await?;
    let leading = existing.is_none();
    if leading && at_cap(&mut tx, &authed.user_id).await? {
        return Err(ApiError::Conflict(
            "You have 5 requests in progress.".into(),
        ));
    }
    insert_block(
        &mut tx,
        &guid,
        source.indexer.as_deref().unwrap_or(""),
        &release_title,
        release_size,
        &source.work_key,
        &book.title,
        &book.author,
        &authed.user_id,
    )
    .await?;
    let holders = holders_of(&mut tx, book_id, &authed.user_id, &book.owner_id).await?;
    let now = now_ms();
    sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', book_id = NULL, error = NULL, download_id = NULL,
             updated_at = ?, next_check_at = ?
         WHERE book_id = ? AND state = 'imported'",
    )
    .bind(now)
    .bind(now)
    .bind(book_id)
    .execute(&mut *tx)
    .await?;
    let (leader_id, leader_key) = if let Some((id, key)) = existing {
        let id = insert_follower(
            &mut tx,
            &authed.user_id,
            &key,
            &id,
            &source,
            book.shared,
            now,
        )
        .await?;
        (id, key)
    } else {
        let id = insert_leader(
            &mut tx,
            &authed.user_id,
            &keys.canonical,
            &source,
            book.shared,
            now,
        )
        .await?;
        (id, keys.canonical)
    };
    for user_id in holders {
        if user_id == authed.user_id {
            continue;
        }
        if active_for(&mut tx, &user_id, &keys.all).await? {
            continue;
        }
        match insert_follower(
            &mut tx,
            &user_id,
            &leader_key,
            &leader_id,
            &source,
            book.shared,
            now,
        )
        .await
        {
            Ok(_) => {}
            Err(err) if user_conflict(&err) => {}
            Err(err) => return Err(err.into()),
        }
    }
    sqlx::query("DELETE FROM books WHERE id = ?")
        .bind(book_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let _ = tokio::fs::remove_dir_all(library::book_dir(
        &state.data_dir,
        &book.files_owner,
        book_id,
    ))
    .await;
    if leading {
        state.worker.notify_one();
    }
    load_request(&state.pool, &leader_id).await
}

struct BookMeta {
    owner_id: String,
    files_owner: String,
    title: String,
    author: String,
    shared: bool,
}

async fn load_book(pool: &SqlitePool, book_id: &str) -> Result<Option<BookMeta>, ApiError> {
    let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT user_id, COALESCE(files_user_id, user_id), title, author, shared
         FROM books WHERE id = ?",
    )
    .bind(book_id)
    .fetch_optional(pool)
    .await?;
    Ok(
        row.map(|(owner_id, files_owner, title, author, shared)| BookMeta {
            owner_id,
            files_owner,
            title,
            author,
            shared: shared != 0,
        }),
    )
}

struct Source {
    work_key: String,
    title: String,
    author: String,
    year: Option<i64>,
    cover_id: Option<i64>,
    release_title: Option<String>,
    release_size: Option<i64>,
    indexer: Option<String>,
    tried_guids: String,
}

async fn load_source(pool: &SqlitePool, book_id: &str) -> Result<Option<Source>, ApiError> {
    let row: Option<(
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
        String,
    )> = sqlx::query_as(
        "SELECT work_key, title, author, year, cover_id, release_title, release_size,
                indexer, tried_guids
         FROM book_requests WHERE id = ?",
    )
    .bind(book_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(
            work_key,
            title,
            author,
            year,
            cover_id,
            release_title,
            release_size,
            indexer,
            tried_guids,
        )| Source {
            work_key,
            title,
            author,
            year,
            cover_id,
            release_title,
            release_size,
            indexer,
            tried_guids,
        },
    ))
}

fn last_guid(raw: &str) -> Option<String> {
    let guids: Vec<String> = serde_json::from_str(raw).ok()?;
    guids
        .into_iter()
        .next_back()
        .filter(|guid| !guid.is_empty())
}

async fn active_for(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    keys: &[String],
) -> Result<bool, ApiError> {
    let listed = serde_json::to_string(keys).unwrap_or_else(|_| "[]".into());
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM book_requests
         WHERE user_id = ? AND state IN ('requested', 'downloading')
           AND work_key IN (SELECT value FROM json_each(?))
         LIMIT 1",
    )
    .bind(user_id)
    .bind(listed)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(found.is_some())
}

async fn at_cap(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
) -> Result<bool, ApiError> {
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND state IN ('requested', 'downloading') AND follows IS NULL",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(active >= ACTIVE_CAP)
}

async fn leader_of(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    keys: &[String],
) -> Result<Option<(String, String)>, ApiError> {
    let listed = serde_json::to_string(keys).unwrap_or_else(|_| "[]".into());
    let row = sqlx::query_as(
        "SELECT id, work_key FROM book_requests
         WHERE work_key IN (SELECT value FROM json_each(?))
           AND state IN ('requested', 'downloading') AND follows IS NULL
         LIMIT 1",
    )
    .bind(listed)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

async fn insert_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    guid: &str,
    indexer: &str,
    title: &str,
    size: i64,
    work_key: &str,
    book_title: &str,
    book_author: &str,
    blocked_by: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO release_blocklist (
            id, guid, indexer, title, title_norm, size, work_key, book_title, book_author,
            blocked_by, created_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(guid)
    .bind(indexer)
    .bind(title)
    .bind(title_norm(title))
    .bind(size)
    .bind(work_key)
    .bind(book_title)
    .bind(book_author)
    .bind(blocked_by)
    .bind(now_ms())
    .execute(&mut **tx)
    .await
    .map_err(|err| {
        if guid_conflict(&err) {
            ApiError::Conflict("That release is already blocked.".into())
        } else {
            err.into()
        }
    })?;
    Ok(())
}

fn guid_conflict(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) if db.is_unique_violation() => db.message().contains("guid"),
        _ => false,
    }
}

fn user_conflict(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) if db.is_unique_violation() => db.message().contains("user_id"),
        _ => false,
    }
}

async fn holders_of(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
    reporter: &str,
    owner_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut users: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM book_requests
         WHERE book_id = ? AND state = 'imported' AND user_id <> ?",
    )
    .bind(book_id)
    .bind(reporter)
    .fetch_all(&mut **tx)
    .await?;
    if owner_id != reporter && !users.iter().any(|id| id == owner_id) {
        users.push(owner_id.to_string());
    }
    Ok(users)
}

async fn insert_leader(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    work_key: &str,
    source: &Source,
    shared: bool,
    now: i64,
) -> Result<String, ApiError> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, year, cover_id, state,
            next_check_at, created_at, updated_at, want_shared
         ) VALUES (?, ?, ?, ?, ?, ?, ?, 'requested', ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(work_key)
    .bind(&source.title)
    .bind(&source.author)
    .bind(source.year)
    .bind(source.cover_id)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(if shared { 1 } else { 0 })
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn insert_follower(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    work_key: &str,
    leader: &str,
    source: &Source,
    shared: bool,
    now: i64,
) -> Result<String, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, year, cover_id, state, follows,
            next_check_at, created_at, updated_at, want_shared
         ) VALUES (?, ?, ?, ?, ?, ?, ?, 'requested', ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(work_key)
    .bind(&source.title)
    .bind(&source.author)
    .bind(source.year)
    .bind(source.cover_id)
    .bind(leader)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(if shared { 1 } else { 0 })
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn load_request(pool: &SqlitePool, id: &str) -> Result<BookRequest, ApiError> {
    let row: Option<(
        String,
        String,
        String,
        String,
        Option<i64>,
        Option<i64>,
        String,
        i64,
        Option<String>,
        Option<String>,
        String,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT r.id, r.work_key, r.title, r.author, r.year, r.cover_id, r.state, r.progress,
                r.error, r.book_id, u.username, r.created_at, r.updated_at
         FROM book_requests r
         JOIN users u ON u.id = r.user_id
         WHERE r.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((
        id,
        work_key,
        title,
        author,
        year,
        cover_id,
        state,
        progress,
        error,
        book_id,
        requester,
        created_at,
        updated_at,
    )) = row
    else {
        return Err(ApiError::NotFound);
    };
    Ok(BookRequest {
        id,
        work_key,
        title,
        author,
        year,
        cover_id,
        state,
        progress,
        error,
        book_id,
        requester,
        created_at,
        updated_at,
        follows: None,
    })
}
