use crate::auth::{check_password, new_token, normalize_username, Authed, ClientIp};
use crate::config::MAX_COVER_BYTES;
use crate::db::now_ms;
use crate::error::ApiError;
use crate::feed::{slug, Post, Posts};
use crate::library::{self, Staged};
use crate::models::{
    ActorRecord, AuthRequest, AuthResponse, BookRecord, BookRow, CommentIndex, CommentRecord,
    CommentRow, CommentThread, FeedItem, FeedPage, InstanceInfo, LibrarySnapshot, LikeSet,
    MeResponse, NewComment, PatchBook, ProfileBooks, ProfilePostRecord, ProfilePostsPage,
    ProfileRecord, ProfileReplies, ProfileReplyRecord, ProfileReplyRow, ProfileRepostRecord,
    ProfileRepostsPage, RepostState, SearchKind, SearchPage, MAX_COMMENT_CHARS, MAX_POST_ID_CHARS,
};
use crate::search::{self, PROFILE_POSTS_LIMIT};
use crate::version::VersionInfo;
use crate::visibility::{BOOK_SELECT, BOOK_VISIBLE_TO};
use crate::AppState;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

pub async fn version() -> Json<VersionInfo> {
    Json(crate::version::current())
}

pub async fn instance(State(state): State<AppState>) -> Result<Json<InstanceInfo>, ApiError> {
    let settings = crate::settings::load(&state.pool).await?;
    Ok(Json(InstanceInfo {
        allow_signup: settings.allow_signup,
    }))
}

pub async fn register(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let (username, norm) = normalize_username(&body.username)?;
    check_password(&body.password)?;
    let attempt = state.throttle.register(ip, &norm)?;
    let settings = crate::settings::load(&state.pool).await?;
    if !settings.allow_signup {
        return Err(ApiError::Forbidden("Signups are off.".into()));
    }
    // A taken name is rejected before Argon2. The UNIQUE index still decides a race.
    let taken = sqlx::query("SELECT id FROM users WHERE username_norm = ?")
        .bind(&norm)
        .fetch_optional(&state.pool)
        .await?;
    if taken.is_some() {
        return Err(ApiError::Conflict("That username is taken.".into()));
    }
    let password_hash = match state.passwords.hash(&body.password).await {
        Ok(hash) => hash,
        Err(ApiError::Busy) => {
            attempt.abandon();
            return Err(ApiError::Busy);
        }
        Err(err) => return Err(err),
    };
    let id = Uuid::new_v4().to_string();
    let created_at = now_ms();
    let inserted = sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&username)
    .bind(&norm)
    .bind(&password_hash)
    .bind(created_at)
    .execute(&state.pool)
    .await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            return Err(ApiError::Conflict("That username is taken.".into()));
        }
        Err(err) => return Err(err.into()),
    }
    let token = issue_session(&state, &id).await?;
    Ok(Json(AuthResponse { token, username }))
}

pub async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let (_, norm) = normalize_username(&body.username)?;
    check_password(&body.password)?;
    let attempt = state.throttle.login(ip, &norm)?;
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, username, password_hash FROM users WHERE username_norm = ?",
    )
    .bind(&norm)
    .fetch_optional(&state.pool)
    .await?;
    let ok = match state
        .passwords
        .verify_login(&body.password, row.as_ref().map(|row| row.2.as_str()))
        .await
    {
        Ok(ok) => ok,
        Err(ApiError::Busy) => {
            attempt.abandon();
            return Err(ApiError::Busy);
        }
        Err(err) => return Err(err),
    };
    let Some((id, username, _)) = row.filter(|_| ok) else {
        return Err(ApiError::Unauthorized("Wrong username or password.".into()));
    };
    attempt.succeeded();
    let token = issue_session(&state, &id).await?;
    Ok(Json(AuthResponse { token, username }))
}

pub async fn logout(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<axum::http::StatusCode, ApiError> {
    sqlx::query("DELETE FROM sessions WHERE token = ?")
        .bind(&authed.token)
        .execute(&state.pool)
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn me(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<MeResponse>, ApiError> {
    let settings = crate::settings::load(&state.pool).await?;
    Ok(Json(MeResponse {
        username: authed.username,
        is_admin: authed.is_admin,
        can_request: crate::requests::can_request(&settings),
    }))
}

pub async fn library(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<LibrarySnapshot>, ApiError> {
    let rows = sqlx::query_as::<_, BookRow>(&format!(
        "{BOOK_SELECT} WHERE {BOOK_VISIBLE_TO} ORDER BY b.imported_at DESC"
    ))
    .bind(&authed.user_id)
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(LibrarySnapshot {
        books: rows
            .into_iter()
            .map(|row| row.into_record(&authed.user_id))
            .collect(),
    }))
}

pub async fn upload_book(
    State(state): State<AppState>,
    authed: Authed,
    multipart: Multipart,
) -> Result<Json<BookRecord>, ApiError> {
    let staged = read_upload(&state, multipart).await?;
    let book = library::import(
        &state,
        &library::Owner {
            user_id: &authed.user_id,
            username: &authed.username,
        },
        library::NewId::Random,
        staged,
    )
    .await?;
    Ok(Json(book))
}

pub async fn get_book(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<BookRecord>, ApiError> {
    Ok(Json(book_record(&state, &authed.user_id, &id).await?))
}

pub async fn delete_book(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let row = visible_book(&state, &authed.user_id, &id).await?;
    if row.owner_id != authed.user_id {
        return Err(ApiError::Forbidden(
            "Only the person who added this book can remove it.".into(),
        ));
    }
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    crate::requests::cancel_imported(&mut *tx, &authed.user_id, &row.id).await?;
    sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
        .bind(&row.id)
        .bind(&authed.user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let dest = book_dir(&state.data_dir, &row.owner_id, &row.id);
    let _ = tokio::fs::remove_dir_all(dest).await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn patch_book(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
    Json(body): Json<PatchBook>,
) -> Result<Json<BookRecord>, ApiError> {
    let row = visible_book(&state, &authed.user_id, &id).await?;
    if body.shared.is_some() && row.owner_id != authed.user_id {
        return Err(ApiError::Forbidden(
            "Only the person who added this book can change sharing.".into(),
        ));
    }
    if let Some(shared) = body.shared {
        sqlx::query("UPDATE books SET shared = ? WHERE id = ? AND user_id = ?")
            .bind(if shared { 1 } else { 0 })
            .bind(&row.id)
            .bind(&authed.user_id)
            .execute(&state.pool)
            .await?;
    }
    // Old phone builds still send postCount. Accept it, but never let a client
    // replace the count of the canonical server feed.
    if let Some(progress_index) = body.progress_index {
        let progress_index = progress_index.max(0);
        sqlx::query(
            "INSERT INTO book_progress (user_id, book_id, progress_index)
             VALUES (?, ?, ?)
             ON CONFLICT(user_id, book_id) DO UPDATE SET progress_index = excluded.progress_index",
        )
        .bind(&authed.user_id)
        .bind(&row.id)
        .bind(progress_index)
        .execute(&state.pool)
        .await?;
    }
    Ok(Json(book_record(&state, &authed.user_id, &row.id).await?))
}

pub async fn download_epub(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row = visible_book(&state, &authed.user_id, &id).await?;
    let path = book_dir(&state.data_dir, &row.owner_id, &row.id).join("book.epub");
    file_response(path, "application/epub+zip", "book.epub").await
}

pub async fn download_cover(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row = visible_book(&state, &authed.user_id, &id).await?;
    let path = book_dir(&state.data_dir, &row.owner_id, &row.id).join("cover.jpg");
    if !path.exists() {
        return Err(ApiError::NotFound);
    }
    let mut magic = Vec::new();
    tokio::fs::File::open(&path)
        .await?
        .take(12)
        .read_to_end(&mut magic)
        .await?;
    file_response(path, cover_content_type(&magic), "cover.jpg").await
}

fn cover_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        "image/webp"
    } else {
        // JPEG magic (ff d8 ff) and unknown legacy covers both use JPEG.
        "image/jpeg"
    }
}

/// Serves the cached posts.json bytes as-is; the cache is written once by
/// upload or by the legacy fill below, so hits never decode and re-encode.
pub async fn get_posts(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let row = visible_book(&state, &authed.user_id, &id).await?;
    let dir = book_dir(&state.data_dir, &row.owner_id, &row.id);
    let bytes = match search::read_posts(&dir).await? {
        Some(bytes) => bytes,
        None => fill_posts(&state, &row.id, &dir).await?,
    };
    Ok((
        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        bytes,
    ))
}

/// Parses a book uploaded before the server parsed books, then caches it.
async fn fill_posts(
    state: &AppState,
    book_id: &str,
    dir: &std::path::Path,
) -> Result<Vec<u8>, ApiError> {
    // Simple global lock for legacy fills; cache hits and new uploads bypass it.
    // Recheck after locking so concurrent requests don't duplicate the parse.
    let _guard = state.lazy_parse_lock.lock().await;
    if let Some(bytes) = search::read_posts(dir).await? {
        return Ok(bytes);
    }
    let parsed = library::parse_epub(dir.join("book.epub")).await?;
    search::store_feed(
        &state.pool,
        dir,
        book_id,
        &Posts {
            posts: parsed.posts,
        },
    )
    .await
}

pub async fn list_likes(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<LikeSet>, ApiError> {
    let _ = visible_book(&state, &authed.user_id, &id).await?;
    Ok(Json(likes_for(&state, &authed.user_id, &id).await?))
}

pub async fn toggle_like(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id)): Path<(String, String)>,
) -> Result<Json<LikeSet>, ApiError> {
    let _ = visible_book(&state, &authed.user_id, &id).await?;
    let inserted = sqlx::query(
        "INSERT INTO likes (user_id, book_id, post_id) VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(&authed.user_id)
    .bind(&id)
    .bind(&post_id)
    .execute(&state.pool)
    .await?;
    if inserted.rows_affected() == 0 {
        sqlx::query("DELETE FROM likes WHERE user_id = ? AND book_id = ? AND post_id = ?")
            .bind(&authed.user_id)
            .bind(&id)
            .bind(&post_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(Json(likes_for(&state, &authed.user_id, &id).await?))
}

pub async fn comment_index(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<CommentIndex>, ApiError> {
    let book = visible_book(&state, &authed.user_id, &id).await?;
    let counts = sqlx::query_as::<_, (String, i64)>(
        "SELECT post_id, COUNT(*) FROM comments WHERE book_id = ? GROUP BY post_id",
    )
    .bind(&book.id)
    .fetch_all(&state.pool)
    .await?;
    let mine = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT post_id FROM comments WHERE book_id = ? AND user_id = ?",
    )
    .bind(&book.id)
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    let reposts = sqlx::query_as::<_, (String, i64)>(
        "SELECT post_id, COUNT(*) FROM reposts WHERE book_id = ? GROUP BY post_id",
    )
    .bind(&book.id)
    .fetch_all(&state.pool)
    .await?;
    let reposted = sqlx::query_scalar::<_, String>(
        "SELECT post_id FROM reposts WHERE book_id = ? AND user_id = ?",
    )
    .bind(&book.id)
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(CommentIndex {
        counts: counts.into_iter().collect(),
        mine,
        reposts: reposts.into_iter().collect(),
        reposted,
    }))
}

pub async fn put_repost(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id)): Path<(String, String)>,
) -> Result<Json<RepostState>, ApiError> {
    let post_id = parse_post_id(&post_id)?;
    let book = visible_book(&state, &authed.user_id, &id).await?;
    require_post(&state, &book.id, &post_id).await?;
    sqlx::query(
        "INSERT INTO reposts (user_id, book_id, post_id, created_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(&authed.user_id)
    .bind(&book.id)
    .bind(&post_id)
    .bind(now_ms())
    .execute(&state.pool)
    .await?;
    Ok(Json(
        repost_state(&state, &authed.user_id, &book.id, &post_id).await?,
    ))
}

pub async fn delete_repost(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id)): Path<(String, String)>,
) -> Result<Json<RepostState>, ApiError> {
    let post_id = parse_post_id(&post_id)?;
    let book = match visible_book(&state, &authed.user_id, &id).await {
        Ok(book) => book,
        Err(ApiError::NotFound) => {
            // The caller may have reposted before the book was unshared.
            // Drop only their row. The count of everyone else's reposts stays hidden.
            let book_id = parse_book_id(&id)?.to_string();
            let deleted = sqlx::query(
                "DELETE FROM reposts WHERE user_id = ? AND book_id = ? AND post_id = ?",
            )
            .bind(&authed.user_id)
            .bind(&book_id)
            .bind(&post_id)
            .execute(&state.pool)
            .await?;
            if deleted.rows_affected() == 0 {
                return Err(ApiError::NotFound);
            }
            return Ok(Json(RepostState {
                reposts: 0,
                reposted: false,
            }));
        }
        Err(err) => return Err(err),
    };
    sqlx::query("DELETE FROM reposts WHERE user_id = ? AND book_id = ? AND post_id = ?")
        .bind(&authed.user_id)
        .bind(&book.id)
        .bind(&post_id)
        .execute(&state.pool)
        .await?;
    Ok(Json(
        repost_state(&state, &authed.user_id, &book.id, &post_id).await?,
    ))
}

async fn repost_state(
    state: &AppState,
    user_id: &str,
    book_id: &str,
    post_id: &str,
) -> Result<RepostState, ApiError> {
    let reposts = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM reposts WHERE book_id = ? AND post_id = ?",
    )
    .bind(book_id)
    .bind(post_id)
    .fetch_one(&state.pool)
    .await?;
    let mine = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM reposts WHERE user_id = ? AND book_id = ? AND post_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .bind(post_id)
    .fetch_one(&state.pool)
    .await?;
    Ok(RepostState {
        reposts,
        reposted: mine != 0,
    })
}

pub async fn list_comments(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id)): Path<(String, String)>,
) -> Result<Json<CommentThread>, ApiError> {
    let post_id = parse_post_id(&post_id)?;
    let _ = visible_book(&state, &authed.user_id, &id).await?;
    Ok(Json(
        thread_for(&state, &authed.user_id, &id, &post_id).await?,
    ))
}

pub async fn add_comment(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id)): Path<(String, String)>,
    Json(body): Json<NewComment>,
) -> Result<Json<CommentThread>, ApiError> {
    let post_id = parse_post_id(&post_id)?;
    let book = visible_book(&state, &authed.user_id, &id).await?;
    require_post(&state, &book.id, &post_id).await?;
    let text = parse_comment_text(&body.text)?;
    let comment_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO comments (id, user_id, book_id, post_id, text, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&comment_id)
    .bind(&authed.user_id)
    .bind(&book.id)
    .bind(&post_id)
    .bind(&text)
    .bind(now_ms())
    .execute(&state.pool)
    .await?;
    Ok(Json(
        thread_for(&state, &authed.user_id, &id, &post_id).await?,
    ))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(rename = "type")]
    kind: SearchKind,
    cursor: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    authed: Authed,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchPage>, ApiError> {
    Ok(Json(
        search::run(
            &state.pool,
            &authed,
            &params.q,
            params.kind,
            params.cursor.as_deref(),
        )
        .await?,
    ))
}

pub async fn profile(
    State(state): State<AppState>,
    authed: Authed,
    Path(username): Path<String>,
) -> Result<Json<ProfileRecord>, ApiError> {
    let (user_id, username, joined_at) = require_user(&state.pool, &username).await?;
    let post_count = visible_reply_count(&state, &authed.user_id, &user_id).await?;
    let repost_count = visible_repost_count(&state, &authed.user_id, &user_id).await?;
    Ok(Json(ProfileRecord {
        username,
        joined_at,
        post_count,
        repost_count,
        mine: user_id == authed.user_id,
    }))
}

#[derive(Deserialize)]
pub struct PageQuery {
    cursor: Option<String>,
}

pub async fn profile_posts(
    State(state): State<AppState>,
    authed: Authed,
    Path(username): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ProfilePostsPage>, ApiError> {
    let (user_id, _, _) = require_user(&state.pool, &username).await?;
    let cursor = match query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
    {
        Some(cursor) => Some(search::decode_keyset(cursor)?),
        None => None,
    };
    // Keyset on (created_at, id) matches comments_user plus the id tiebreak.
    // Newest first. The extra row is how we know there is a next page.
    let (has_cursor, created_at, comment_id) = match cursor {
        Some((created_at, id)) => (1i64, created_at, id),
        None => (0, 0, String::new()),
    };
    let comment_posts = posts_for("c");
    let sql = format!(
        "SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
                COALESCE(p.progress_index, 0) AS progress_index,
                b.is_sample, b.shared, b.user_id AS owner_id,
                owner.username AS owner_username,
                posts.idx AS post_idx, posts.kind AS post_kind,
                posts.chapter AS post_chapter, posts.text AS post_text,
                c.id AS comment_id, u.username AS comment_username,
                c.text AS comment_text, c.created_at AS comment_created_at,
                c.user_id AS comment_user_id
         FROM comments c
         JOIN users u ON u.id = c.user_id
         JOIN books b ON b.id = c.book_id
         JOIN users owner ON owner.id = b.user_id
         LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
         {comment_posts}
         WHERE c.user_id = ? AND {BOOK_VISIBLE_TO}
           AND (
             ? = 0
             OR c.created_at < ?
             OR (c.created_at = ? AND c.id < ?)
           )
         ORDER BY c.created_at DESC, c.id DESC
         LIMIT ?"
    );
    let mut rows = sqlx::query_as::<_, ProfilePostRow>(&sql)
        .bind(&authed.user_id)
        .bind(&user_id)
        .bind(&authed.user_id)
        .bind(has_cursor)
        .bind(created_at)
        .bind(created_at)
        .bind(&comment_id)
        .bind(PROFILE_POSTS_LIMIT + 1)
        .fetch_all(&state.pool)
        .await?;
    let next_cursor = if rows.len() > PROFILE_POSTS_LIMIT as usize {
        rows.truncate(PROFILE_POSTS_LIMIT as usize);
        rows.last()
            .map(|row| search::encode_keyset(row.comment_created_at, &row.comment_id))
    } else {
        None
    };
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.into_record(&authed.user_id)?);
    }
    Ok(Json(ProfilePostsPage { items, next_cursor }))
}

const PROFILE_REPOSTS_LIMIT: i64 = 30;
const FEED_LIMIT: i64 = 20;

pub async fn profile_reposts(
    State(state): State<AppState>,
    authed: Authed,
    Path(username): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ProfileRepostsPage>, ApiError> {
    let (user_id, _, _) = require_user(&state.pool, &username).await?;
    let (has_cursor, created_at, cursor_book, cursor_post) = keyset3(&query, false)?;
    let repost_posts = posts_for("r");
    let sql = format!(
        "SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
                COALESCE(p.progress_index, 0) AS progress_index,
                b.is_sample, b.shared, b.user_id AS owner_id,
                owner.username AS owner_username,
                posts.idx AS post_idx, posts.kind AS post_kind,
                posts.chapter AS post_chapter, posts.text AS post_text,
                r.created_at AS reposted_at, r.book_id AS repost_book_id,
                r.post_id AS repost_post_id
         FROM reposts r
         JOIN books b ON b.id = r.book_id
         JOIN users owner ON owner.id = b.user_id
         LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
         {repost_posts}
         WHERE r.user_id = ? AND {BOOK_VISIBLE_TO}
           AND (
             ? = 0
             OR r.created_at < ?
             OR (r.created_at = ? AND r.book_id < ?)
             OR (r.created_at = ? AND r.book_id = ? AND r.post_id < ?)
           )
         ORDER BY r.created_at DESC, r.book_id DESC, r.post_id DESC
         LIMIT ?"
    );
    let mut rows = sqlx::query_as::<_, ProfileRepostRow>(&sql)
        .bind(&authed.user_id)
        .bind(&user_id)
        .bind(&authed.user_id)
        .bind(has_cursor)
        .bind(created_at)
        .bind(created_at)
        .bind(&cursor_book)
        .bind(created_at)
        .bind(&cursor_book)
        .bind(&cursor_post)
        .bind(PROFILE_REPOSTS_LIMIT + 1)
        .fetch_all(&state.pool)
        .await?;
    let next_cursor = if rows.len() > PROFILE_REPOSTS_LIMIT as usize {
        rows.truncate(PROFILE_REPOSTS_LIMIT as usize);
        rows.last().map(|row| {
            search::encode_keyset3(row.reposted_at, &row.repost_book_id, &row.repost_post_id)
        })
    } else {
        None
    };
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.into_record(&authed.user_id)?);
    }
    Ok(Json(ProfileRepostsPage { items, next_cursor }))
}

pub async fn profile_books(
    State(state): State<AppState>,
    authed: Authed,
    Path(username): Path<String>,
) -> Result<Json<ProfileBooks>, ApiError> {
    let (user_id, _, _) = require_user(&state.pool, &username).await?;
    let rows = sqlx::query_as::<_, BookRow>(&format!(
        "{BOOK_SELECT} WHERE b.user_id = ? AND b.shared = 1 AND b.is_sample = 0 AND {BOOK_VISIBLE_TO}
         ORDER BY b.imported_at DESC, b.id ASC"
    ))
    .bind(&authed.user_id)
    .bind(&user_id)
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(ProfileBooks {
        items: rows
            .into_iter()
            .map(|row| row.into_record(&authed.user_id))
            .collect(),
    }))
}

pub async fn feed(
    State(state): State<AppState>,
    authed: Authed,
    Query(query): Query<PageQuery>,
) -> Result<Json<FeedPage>, ApiError> {
    let (has_cursor, at, kind, id) = keyset3(&query, true)?;
    let comment_posts = posts_for("c");
    let repost_posts = posts_for("r");
    // Each branch keeps its own cursor and limit so the feed does not sort the
    // whole history before discarding it. The outer query merges those pages.
    let sql = format!(
        "SELECT kind, id, at, actor_username, book_id, title, author, handle, imported_at,
                post_count, progress_index, is_sample, shared, owner_id, owner_username,
                post_idx, post_kind, post_chapter, post_text, comment_id, comment_username,
                comment_text, comment_created_at, comment_user_id
         FROM (
             SELECT * FROM (
                 SELECT 'comment' AS kind, c.id AS id, c.created_at AS at,
                        u.username AS actor_username,
                        b.id AS book_id, b.title AS title, b.author AS author, b.handle AS handle,
                        b.imported_at AS imported_at, b.post_count AS post_count,
                        COALESCE(p.progress_index, 0) AS progress_index,
                        b.is_sample AS is_sample, b.shared AS shared, b.user_id AS owner_id,
                        owner.username AS owner_username,
                        posts.idx AS post_idx, posts.kind AS post_kind,
                        posts.chapter AS post_chapter, posts.text AS post_text,
                        c.id AS comment_id, u.username AS comment_username,
                        c.text AS comment_text, c.created_at AS comment_created_at,
                        c.user_id AS comment_user_id
                 FROM comments c
                 JOIN users u ON u.id = c.user_id
                 JOIN books b ON b.id = c.book_id
                 JOIN users owner ON owner.id = b.user_id
                 LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
                 {comment_posts}
                 WHERE {BOOK_VISIBLE_TO}
                   AND (
                     ? = 0
                     OR c.created_at < ?
                     OR (c.created_at = ? AND 'comment' < ?)
                     OR (c.created_at = ? AND 'comment' = ? AND c.id < ?)
                   )
                 ORDER BY c.created_at DESC, c.id DESC
                 LIMIT ?
             )
             UNION ALL
             SELECT * FROM (
                 SELECT 'repost' AS kind,
                        r.user_id || ':' || r.book_id || ':' || r.post_id AS id,
                        r.created_at AS at, u.username AS actor_username,
                        b.id AS book_id, b.title AS title, b.author AS author, b.handle AS handle,
                        b.imported_at AS imported_at, b.post_count AS post_count,
                        COALESCE(p.progress_index, 0) AS progress_index,
                        b.is_sample AS is_sample, b.shared AS shared, b.user_id AS owner_id,
                        owner.username AS owner_username,
                        posts.idx AS post_idx, posts.kind AS post_kind,
                        posts.chapter AS post_chapter, posts.text AS post_text,
                        CAST(NULL AS TEXT) AS comment_id,
                        CAST(NULL AS TEXT) AS comment_username,
                        CAST(NULL AS TEXT) AS comment_text,
                        CAST(NULL AS INTEGER) AS comment_created_at,
                        CAST(NULL AS TEXT) AS comment_user_id
                 FROM reposts r
                 JOIN users u ON u.id = r.user_id
                 JOIN books b ON b.id = r.book_id
                 JOIN users owner ON owner.id = b.user_id
                 LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
                 {repost_posts}
                 WHERE {BOOK_VISIBLE_TO}
                   AND (
                     ? = 0
                     OR r.created_at < ?
                     OR (r.created_at = ? AND 'repost' < ?)
                     OR (r.created_at = ? AND 'repost' = ? AND (r.user_id || ':' || r.book_id || ':' || r.post_id) < ?)
                   )
                 ORDER BY r.created_at DESC, r.user_id || ':' || r.book_id || ':' || r.post_id DESC
                 LIMIT ?
             )
         )
         ORDER BY at DESC, kind DESC, id DESC
         LIMIT ?"
    );
    let limit = FEED_LIMIT + 1;
    let mut query = sqlx::query_as::<_, FeedRow>(&sql);
    for _ in 0..2 {
        query = query
            .bind(&authed.user_id)
            .bind(&authed.user_id)
            .bind(has_cursor)
            .bind(at)
            .bind(at)
            .bind(&kind)
            .bind(at)
            .bind(&kind)
            .bind(&id)
            .bind(limit);
    }
    let mut rows = query.bind(limit).fetch_all(&state.pool).await?;
    let next_cursor = if rows.len() > FEED_LIMIT as usize {
        rows.truncate(FEED_LIMIT as usize);
        rows.last()
            .map(|row| search::encode_keyset3(row.at, &row.kind, &row.id))
    } else {
        None
    };
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.into_item(&authed.user_id)?);
    }
    Ok(Json(FeedPage { items, next_cursor }))
}

fn keyset3(query: &PageQuery, feed: bool) -> Result<(i64, i64, String, String), ApiError> {
    let cursor = query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty());
    let Some(cursor) = cursor else {
        return Ok((0, 0, String::new(), String::new()));
    };
    let (at, second, third) = if feed {
        search::decode_feed_cursor(cursor)?
    } else {
        search::decode_keyset3(cursor)?
    };
    Ok((1, at, second, third))
}

#[derive(Debug, sqlx::FromRow)]
struct ProfileRepostRow {
    id: String,
    title: String,
    author: String,
    handle: String,
    imported_at: i64,
    post_count: i64,
    progress_index: i64,
    is_sample: i64,
    shared: i64,
    owner_id: String,
    owner_username: String,
    post_idx: i64,
    post_kind: String,
    post_chapter: String,
    post_text: String,
    reposted_at: i64,
    repost_book_id: String,
    repost_post_id: String,
}

impl ProfileRepostRow {
    fn into_record(self, viewer: &str) -> Result<ProfileRepostRecord, ApiError> {
        let book = BookRow {
            id: self.id,
            title: self.title,
            author: self.author,
            handle: self.handle,
            imported_at: self.imported_at,
            post_count: self.post_count,
            progress_index: self.progress_index,
            is_sample: self.is_sample,
            shared: self.shared,
            owner_id: self.owner_id,
            owner_username: self.owner_username,
        }
        .into_record(viewer);
        Ok(ProfileRepostRecord {
            book,
            post: stored_post(
                self.post_idx,
                &self.post_kind,
                self.post_chapter,
                self.post_text,
            )?,
            reposted_at: self.reposted_at,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct FeedRow {
    kind: String,
    id: String,
    at: i64,
    actor_username: String,
    book_id: String,
    title: String,
    author: String,
    handle: String,
    imported_at: i64,
    post_count: i64,
    progress_index: i64,
    is_sample: i64,
    shared: i64,
    owner_id: String,
    owner_username: String,
    post_idx: i64,
    post_kind: String,
    post_chapter: String,
    post_text: String,
    comment_id: Option<String>,
    comment_username: Option<String>,
    comment_text: Option<String>,
    comment_created_at: Option<i64>,
    comment_user_id: Option<String>,
}

impl FeedRow {
    fn into_item(self, viewer: &str) -> Result<FeedItem, ApiError> {
        let book = BookRow {
            id: self.book_id,
            title: self.title,
            author: self.author,
            handle: self.handle,
            imported_at: self.imported_at,
            post_count: self.post_count,
            progress_index: self.progress_index,
            is_sample: self.is_sample,
            shared: self.shared,
            owner_id: self.owner_id,
            owner_username: self.owner_username,
        }
        .into_record(viewer);
        let post = stored_post(
            self.post_idx,
            &self.post_kind,
            self.post_chapter,
            self.post_text,
        )?;
        let actor = ActorRecord {
            username: self.actor_username,
        };
        match self.kind.as_str() {
            "comment" => {
                let (Some(id), Some(username), Some(text), Some(created_at), Some(user_id)) = (
                    self.comment_id,
                    self.comment_username,
                    self.comment_text,
                    self.comment_created_at,
                    self.comment_user_id,
                ) else {
                    tracing::error!("comment activity is missing its comment");
                    return Err(ApiError::Internal);
                };
                Ok(FeedItem::Comment {
                    actor,
                    book,
                    post,
                    comment: CommentRecord {
                        id,
                        username,
                        text,
                        created_at,
                        mine: user_id == viewer,
                    },
                    at: self.at,
                })
            }
            "repost" => Ok(FeedItem::Repost {
                actor,
                book,
                post,
                at: self.at,
            }),
            _ => {
                tracing::error!(kind = %self.kind, "activity kind is not comment or repost");
                Err(ApiError::Internal)
            }
        }
    }
}

fn stored_post(idx: i64, kind: &str, chapter: String, text: String) -> Result<Post, ApiError> {
    Ok(Post {
        id: idx.to_string(),
        kind: search::parse_kind(kind)?,
        text,
        chapter,
    })
}

#[derive(Debug, sqlx::FromRow)]
struct ProfilePostRow {
    id: String,
    title: String,
    author: String,
    handle: String,
    imported_at: i64,
    post_count: i64,
    progress_index: i64,
    is_sample: i64,
    shared: i64,
    owner_id: String,
    owner_username: String,
    post_idx: i64,
    post_kind: String,
    post_chapter: String,
    post_text: String,
    comment_id: String,
    comment_username: String,
    comment_text: String,
    comment_created_at: i64,
    comment_user_id: String,
}

impl ProfilePostRow {
    fn into_record(self, viewer: &str) -> Result<ProfilePostRecord, ApiError> {
        let book = BookRow {
            id: self.id,
            title: self.title,
            author: self.author,
            handle: self.handle,
            imported_at: self.imported_at,
            post_count: self.post_count,
            progress_index: self.progress_index,
            is_sample: self.is_sample,
            shared: self.shared,
            owner_id: self.owner_id,
            owner_username: self.owner_username,
        }
        .into_record(viewer);
        Ok(ProfilePostRecord {
            book,
            post: Post {
                id: self.post_idx.to_string(),
                kind: search::parse_kind(&self.post_kind)?,
                text: self.post_text,
                chapter: self.post_chapter,
            },
            comment: CommentRecord {
                id: self.comment_id,
                username: self.comment_username,
                text: self.comment_text,
                created_at: self.comment_created_at,
                mine: self.comment_user_id == viewer,
            },
        })
    }
}

async fn require_user(
    pool: &sqlx::SqlitePool,
    username: &str,
) -> Result<(String, String, i64), ApiError> {
    let (_, norm) = normalize_username(username)?;
    let row = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, username, created_at FROM users WHERE username_norm = ?",
    )
    .bind(&norm)
    .fetch_optional(pool)
    .await?;
    row.ok_or(ApiError::NotFound)
}

async fn visible_reply_count(
    state: &AppState,
    viewer: &str,
    user_id: &str,
) -> Result<i64, ApiError> {
    let comment_posts = posts_for("c");
    let count = sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*)
         FROM comments c
         JOIN books b ON b.id = c.book_id
         {comment_posts}
         WHERE c.user_id = ? AND {BOOK_VISIBLE_TO}"
    ))
    .bind(user_id)
    .bind(viewer)
    .fetch_one(&state.pool)
    .await?;
    Ok(count)
}

async fn visible_repost_count(
    state: &AppState,
    viewer: &str,
    user_id: &str,
) -> Result<i64, ApiError> {
    let repost_posts = posts_for("r");
    let count = sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*)
         FROM reposts r
         JOIN books b ON b.id = r.book_id
         {repost_posts}
         WHERE r.user_id = ? AND {BOOK_VISIBLE_TO}"
    ))
    .bind(user_id)
    .bind(viewer)
    .fetch_one(&state.pool)
    .await?;
    Ok(count)
}

pub async fn profile_replies(
    State(state): State<AppState>,
    authed: Authed,
    Path(username): Path<String>,
) -> Result<Json<ProfileReplies>, ApiError> {
    let (display, norm) = normalize_username(&username)?;
    let user = sqlx::query_as::<_, (String, String)>(
        "SELECT id, username FROM users WHERE username_norm = ?",
    )
    .bind(&norm)
    .fetch_optional(&state.pool)
    .await?;
    let Some((user_id, stored_name)) = user else {
        return Ok(Json(ProfileReplies {
            username: display,
            replies: vec![],
        }));
    };
    let rows = sqlx::query_as::<_, ProfileReplyRow>(&format!(
        "SELECT c.id, c.book_id, c.post_id, u.username, c.text, c.created_at, c.user_id
         FROM comments c
         JOIN users u ON u.id = c.user_id
         JOIN books b ON b.id = c.book_id
         WHERE c.user_id = ? AND {BOOK_VISIBLE_TO}
         ORDER BY c.created_at DESC, c.id DESC"
    ))
    .bind(&user_id)
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(ProfileReplies {
        username: stored_name,
        replies: rows
            .into_iter()
            .map(|row| ProfileReplyRecord {
                book_id: row.book_id,
                post_id: row.post_id,
                comment: CommentRecord {
                    id: row.id,
                    username: row.username,
                    text: row.text,
                    created_at: row.created_at,
                    mine: row.user_id == authed.user_id,
                },
            })
            .collect(),
    }))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id, comment_id)): Path<(String, String, String)>,
) -> Result<Json<CommentThread>, ApiError> {
    let post_id = parse_post_id(&post_id)?;
    let _ = visible_book(&state, &authed.user_id, &id).await?;
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM comments WHERE id = ? AND book_id = ? AND post_id = ?",
    )
    .bind(&comment_id)
    .bind(&id)
    .bind(&post_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(owner) = owner else {
        return Err(ApiError::NotFound);
    };
    if owner != authed.user_id {
        return Err(ApiError::Forbidden(
            "You can only delete your own comment.".into(),
        ));
    }
    sqlx::query("DELETE FROM comments WHERE id = ?")
        .bind(&comment_id)
        .execute(&state.pool)
        .await?;
    Ok(Json(
        thread_for(&state, &authed.user_id, &id, &post_id).await?,
    ))
}

async fn issue_session(state: &AppState, user_id: &str) -> Result<String, ApiError> {
    let token = new_token();
    sqlx::query("INSERT INTO sessions (token, user_id, created_at) VALUES (?, ?, ?)")
        .bind(&token)
        .bind(user_id)
        .bind(now_ms())
        .execute(&state.pool)
        .await?;
    Ok(token)
}

fn parse_book_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| ApiError::NotFound)
}

async fn book_record(state: &AppState, user_id: &str, id: &str) -> Result<BookRecord, ApiError> {
    Ok(visible_book(state, user_id, id).await?.into_record(user_id))
}

async fn visible_book(state: &AppState, user_id: &str, id: &str) -> Result<BookRow, ApiError> {
    let id = parse_book_id(id)?.to_string();
    let row = sqlx::query_as::<_, BookRow>(&format!(
        "{BOOK_SELECT} WHERE b.id = ? AND {BOOK_VISIBLE_TO}"
    ))
    .bind(user_id)
    .bind(&id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    row.ok_or(ApiError::NotFound)
}

fn parse_post_id(post_id: &str) -> Result<String, ApiError> {
    if post_id.is_empty() {
        return Err(ApiError::BadRequest("Post id is required.".into()));
    }
    if post_id.chars().count() > MAX_POST_ID_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Post id is longer than {MAX_POST_ID_CHARS} characters (asked {}).",
            post_id.chars().count()
        )));
    }
    // `00`, `+0`, and `abc` must not collapse onto post 0 through CAST.
    let Ok(idx) = post_id.parse::<u32>() else {
        return Err(ApiError::BadRequest(
            "Post id is not a canonical index.".into(),
        ));
    };
    let canonical = idx.to_string();
    if canonical != post_id {
        return Err(ApiError::BadRequest(
            "Post id is not a canonical index.".into(),
        ));
    }
    Ok(canonical)
}

async fn require_post(state: &AppState, book_id: &str, post_id: &str) -> Result<(), ApiError> {
    let idx: i64 = post_id.parse().map_err(|_| ApiError::NotFound)?;
    let found = sqlx::query_scalar::<_, i64>("SELECT 1 FROM posts WHERE book_id = ? AND idx = ?")
        .bind(book_id)
        .bind(idx)
        .fetch_optional(&state.pool)
        .await?;
    found.map(|_| ()).ok_or(ApiError::NotFound)
}

/// `alias` is `c` or `r`. The idx comparison uses UNIQUE (book_id, idx).
/// The text comparison rejects a non-canonical id that CASTs to that idx.
fn posts_for(alias: &str) -> String {
    format!(
        "JOIN posts ON posts.book_id = {alias}.book_id \
         AND posts.idx = CAST({alias}.post_id AS INTEGER) \
         AND {alias}.post_id = CAST(posts.idx AS TEXT)"
    )
}

fn parse_comment_text(raw: &str) -> Result<String, ApiError> {
    let text = raw.trim().to_string();
    if text.is_empty() {
        return Err(ApiError::BadRequest("Write something first.".into()));
    }
    let asked = text.chars().count();
    if asked > MAX_COMMENT_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Comment is longer than {MAX_COMMENT_CHARS} characters (asked {asked})."
        )));
    }
    Ok(text)
}

async fn thread_for(
    state: &AppState,
    user_id: &str,
    book_id: &str,
    post_id: &str,
) -> Result<CommentThread, ApiError> {
    let rows = sqlx::query_as::<_, CommentRow>(
        "SELECT c.id, u.username, c.text, c.created_at, c.user_id
         FROM comments c
         JOIN users u ON u.id = c.user_id
         WHERE c.book_id = ? AND c.post_id = ?
         ORDER BY c.created_at ASC",
    )
    .bind(book_id)
    .bind(post_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(CommentThread {
        comments: rows
            .into_iter()
            .map(|row| row.into_record(user_id))
            .collect(),
    })
}

async fn likes_for(state: &AppState, user_id: &str, book_id: &str) -> Result<LikeSet, ApiError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT post_id FROM likes WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(LikeSet { ids })
}

fn book_dir(data_dir: &std::path::Path, user_id: &str, book_id: &str) -> PathBuf {
    data_dir.join("books").join(user_id).join(book_id)
}

async fn read_upload(state: &AppState, mut multipart: Multipart) -> Result<Staged, ApiError> {
    let max_epub_bytes = crate::settings::load(&state.pool).await?.max_epub_bytes;
    let temp_dir = state.data_dir.join("tmp").join(Uuid::new_v4().to_string());
    tokio::fs::create_dir_all(&temp_dir).await?;
    let mut title = None;
    let mut author = None;
    let mut handle = None;
    let mut is_sample = false;
    let mut epub_path = None;
    let mut cover_path = None;

    let result: Result<(), ApiError> = async {
        while let Some(field) = multipart.next_field().await? {
            let name = field.name().unwrap_or("").to_string();
            match name.as_str() {
                "title" => title = Some(field_text(field, 512).await?),
                "author" => author = Some(field_text(field, 256).await?),
                "handle" => handle = Some(field_text(field, 32).await?),
                "isSample" | "is_sample" => {
                    let value = field_text(field, 16).await?;
                    is_sample = matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
                }
                "epub" => {
                    let dest = temp_dir.join("book.epub");
                    write_limited(field, &dest, max_epub_bytes).await?;
                    epub_path = Some(dest);
                }
                "cover" => {
                    let dest = temp_dir.join("cover.jpg");
                    write_limited(field, &dest, MAX_COVER_BYTES).await?;
                    cover_path = Some(dest);
                }
                _ => {
                    skip_field(field, 8 * 1024).await?;
                }
            }
        }
        Ok(())
    }
    .await;

    if let Err(err) = result {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Err(err);
    }

    let Some(epub_path) = epub_path else {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Err(ApiError::BadRequest("EPUB file is required.".into()));
    };
    let result = async {
        let parsed = library::parse_epub(epub_path).await?;
        let title = title
            .filter(|s| !s.is_empty())
            .or(parsed.title)
            .unwrap_or_else(|| "Untitled".into());
        let author = author
            .filter(|s| !s.is_empty())
            .or(parsed.author)
            .unwrap_or_else(|| "Unknown".into());
        let handle = handle
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| slug(&author));
        if cover_path.is_none() {
            if let Some(cover) = parsed.cover {
                tokio::fs::write(temp_dir.join("cover.jpg"), cover).await?;
            }
        }
        let post_count = i32::try_from(parsed.posts.len()).map_err(|_| {
            ApiError::BadRequest(format!(
                "Post count exceeds BookRecord's signed 32-bit representation (asked {}).",
                parsed.posts.len()
            ))
        })?;
        Ok(Staged {
            title,
            author,
            handle,
            is_sample,
            post_count,
            posts: Posts {
                posts: parsed.posts,
            },
            temp_dir: temp_dir.clone(),
        })
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
    result
}

async fn field_text(
    mut field: axum::extract::multipart::Field<'_>,
    max: usize,
) -> Result<String, ApiError> {
    let mut written = 0usize;
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await? {
        written = written.saturating_add(chunk.len());
        if written > max {
            return Err(ApiError::BadRequest(format!(
                "Field is longer than {max} bytes."
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

async fn skip_field(
    mut field: axum::extract::multipart::Field<'_>,
    max: u64,
) -> Result<(), ApiError> {
    let mut written = 0u64;
    while let Some(chunk) = field.chunk().await? {
        written += chunk.len() as u64;
        if written > max {
            return Err(ApiError::BadRequest(
                "Unknown upload field is too large.".into(),
            ));
        }
    }
    Ok(())
}

async fn write_limited(
    mut field: axum::extract::multipart::Field<'_>,
    dest: &std::path::Path,
    limit: u64,
) -> Result<(), ApiError> {
    if let Some(len) = field.headers().typed_get_content_length() {
        if len > limit {
            return Err(ApiError::PayloadTooLarge { asked: len, limit });
        }
    }
    let mut file = tokio::fs::File::create(dest).await?;
    let mut written: u64 = 0;
    while let Some(chunk) = field.chunk().await? {
        written += chunk.len() as u64;
        if written > limit {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            return Err(ApiError::PayloadTooLarge {
                asked: written,
                limit,
            });
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(())
}

trait ContentLengthHeader {
    fn typed_get_content_length(&self) -> Option<u64>;
}

impl ContentLengthHeader for axum::http::HeaderMap {
    fn typed_get_content_length(&self) -> Option<u64> {
        self.get(axum::http::header::CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }
}

async fn file_response(
    path: PathBuf,
    content_type: &'static str,
    filename: &'static str,
) -> Result<impl IntoResponse, ApiError> {
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| ApiError::NotFound)?;
    let stream = ReaderStream::new(file);
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    Ok((headers, axum::body::Body::from_stream(stream)))
}
