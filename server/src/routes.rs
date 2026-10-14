use crate::auth::{
    check_password, hash_password, new_token, normalize_username, verify_password, Authed,
};
use crate::config::MAX_COVER_BYTES;
use crate::db::now_ms;
use crate::error::ApiError;
use crate::feed::{slug, Posts};
use crate::models::{
    AuthRequest, AuthResponse, BookRecord, BookRow, CommentIndex, CommentRecord, CommentRow,
    CommentThread, LibrarySnapshot, LikeSet, MeResponse, NewComment, PatchBook, ProfileReplies,
    ProfileReplyRecord, ProfileReplyRow, MAX_COMMENT_CHARS, MAX_POST_ID_CHARS,
};
use crate::version::VersionInfo;
use crate::AppState;
use axum::extract::{Multipart, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::Json;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

pub async fn version() -> Json<VersionInfo> {
    Json(crate::version::current())
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    if !state.allow_signup {
        return Err(ApiError::Forbidden("Signups are off.".into()));
    }
    let (username, norm) = normalize_username(&body.username)?;
    check_password(&body.password)?;
    let password_hash = hash_password(&body.password)?;
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
    Json(body): Json<AuthRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let (_, norm) = normalize_username(&body.username)?;
    check_password(&body.password)?;
    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, username, password_hash FROM users WHERE username_norm = ?",
    )
    .bind(&norm)
    .fetch_optional(&state.pool)
    .await?;
    let Some((id, username, password_hash)) = row else {
        return Err(ApiError::Unauthorized("Wrong username or password.".into()));
    };
    if !verify_password(&body.password, &password_hash)? {
        return Err(ApiError::Unauthorized("Wrong username or password.".into()));
    }
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

pub async fn me(authed: Authed) -> Json<MeResponse> {
    Json(MeResponse {
        username: authed.username,
    })
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
    let parsed = read_upload(&state, multipart).await?;
    if parsed.is_sample {
        if let Some(existing) = existing_sample(&state, &authed.user_id).await? {
            let _ = tokio::fs::remove_dir_all(&parsed.temp_dir).await;
            return Ok(Json(existing));
        }
    }
    let id = Uuid::new_v4().to_string();
    let imported_at = now_ms();
    let dest = book_dir(&state.data_dir, &authed.user_id, &id);
    tokio::fs::create_dir_all(dest.parent().ok_or(ApiError::Internal)?).await?;
    // The complete EPUB, cover and posts cache become visible together.
    tokio::fs::rename(&parsed.temp_dir, &dest).await?;

    let inserted = sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)",
    )
    .bind(&id)
    .bind(&authed.user_id)
    .bind(&parsed.title)
    .bind(&parsed.author)
    .bind(&parsed.handle)
    .bind(imported_at)
    .bind(parsed.post_count)
    .bind(if parsed.is_sample { 1 } else { 0 })
    .execute(&state.pool)
    .await;

    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() && parsed.is_sample => {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            let existing = existing_sample(&state, &authed.user_id)
                .await?
                .ok_or(ApiError::NotFound)?;
            return Ok(Json(existing));
        }
        Err(err) => {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            return Err(err.into());
        }
    }

    sqlx::query("INSERT INTO book_progress (user_id, book_id, progress_index) VALUES (?, ?, 0)")
        .bind(&authed.user_id)
        .bind(&id)
        .execute(&state.pool)
        .await?;

    Ok(Json(BookRecord {
        id,
        title: parsed.title,
        author: parsed.author,
        handle: parsed.handle,
        imported_at,
        post_count: parsed.post_count,
        progress_index: 0,
        is_sample: parsed.is_sample,
        mine: true,
        shared: false,
        owner_username: authed.username,
    }))
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
    sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
        .bind(&row.id)
        .bind(&authed.user_id)
        .execute(&state.pool)
        .await?;
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
    let bytes = match read_posts(&dir).await? {
        Some(bytes) => bytes,
        None => fill_posts(&state, &authed.user_id, &row.id, &dir).await?,
    };
    Ok((
        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        bytes,
    ))
}

/// Parses a book uploaded before the server parsed books, then caches it.
async fn fill_posts(
    state: &AppState,
    user_id: &str,
    book_id: &str,
    dir: &std::path::Path,
) -> Result<Vec<u8>, ApiError> {
    // Simple global lock for legacy fills; cache hits and new uploads bypass it.
    // Recheck after locking so concurrent requests don't duplicate the parse.
    let _guard = state.lazy_parse_lock.lock().await;
    if let Some(bytes) = read_posts(dir).await? {
        return Ok(bytes);
    }
    let parsed = parse_epub(dir.join("book.epub")).await?;
    let posts = Posts {
        posts: parsed.posts,
    };
    sqlx::query(&format!(
        "UPDATE books SET post_count = ? WHERE id IN (
            SELECT b.id FROM books b WHERE b.id = ? AND {BOOK_VISIBLE_TO}
        )"
    ))
    .bind(posts.posts.len() as i64)
    .bind(book_id)
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    write_posts(dir, &posts).await
}

async fn parse_epub(path: PathBuf) -> Result<crate::epub::ParsedEpub, ApiError> {
    tokio::task::spawn_blocking(move || crate::epub::parse_file(&path, MAX_COVER_BYTES))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "EPUB parser task failed");
            ApiError::Internal
        })?
        .map_err(|reason| ApiError::BadRequest(format!("Could not read that EPUB: {reason}")))
}

async fn read_posts(dir: &std::path::Path) -> Result<Option<Vec<u8>>, ApiError> {
    match tokio::fs::read(dir.join("posts.json")).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Atomically writes posts.json and returns the bytes written.
async fn write_posts(dir: &std::path::Path, posts: &Posts) -> Result<Vec<u8>, ApiError> {
    let bytes = serde_json::to_vec(posts).map_err(|_| ApiError::Internal)?;
    let temp = dir.join(format!("posts-{}.tmp", Uuid::new_v4()));
    tokio::fs::write(&temp, &bytes).await?;
    if let Err(err) = tokio::fs::rename(&temp, dir.join("posts.json")).await {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(err.into());
    }
    Ok(bytes)
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
    let _ = visible_book(&state, &authed.user_id, &id).await?;
    let counts = sqlx::query_as::<_, (String, i64)>(
        "SELECT post_id, COUNT(*) FROM comments WHERE book_id = ? GROUP BY post_id",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await?;
    let mine = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT post_id FROM comments WHERE book_id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(CommentIndex {
        counts: counts.into_iter().collect(),
        mine,
    }))
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
    let _ = visible_book(&state, &authed.user_id, &id).await?;
    let text = parse_comment_text(&body.text)?;
    let comment_id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO comments (id, user_id, book_id, post_id, text, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&comment_id)
    .bind(&authed.user_id)
    .bind(&id)
    .bind(&post_id)
    .bind(&text)
    .bind(now_ms())
    .execute(&state.pool)
    .await?;
    Ok(Json(
        thread_for(&state, &authed.user_id, &id, &post_id).await?,
    ))
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

const BOOK_SELECT: &str = "
SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
       COALESCE(p.progress_index, 0) as progress_index,
       b.is_sample, b.shared, b.user_id as owner_id, owner.username as owner_username
FROM books b
JOIN users owner ON owner.id = b.user_id
LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
";

/// Book alias `b` is visible to the next bound user id. A hidden book is 404.
/// Owned, shared by its owner, or the built-in sample.
const BOOK_VISIBLE_TO: &str = "(b.user_id = ? OR b.shared = 1 OR b.is_sample = 1)";

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

async fn existing_sample(state: &AppState, user_id: &str) -> Result<Option<BookRecord>, ApiError> {
    let row = sqlx::query_as::<_, BookRow>(&format!(
        "{BOOK_SELECT} WHERE b.is_sample = 1 AND {BOOK_VISIBLE_TO} ORDER BY b.imported_at ASC LIMIT 1"
    ))
    .bind(user_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row.map(|row| row.into_record(user_id)))
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
    Ok(post_id.to_string())
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

struct ParsedUpload {
    title: String,
    author: String,
    handle: String,
    is_sample: bool,
    post_count: i32,
    temp_dir: PathBuf,
}

async fn read_upload(state: &AppState, mut multipart: Multipart) -> Result<ParsedUpload, ApiError> {
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
                    write_limited(field, &dest, state.max_epub_bytes).await?;
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
        let parsed = parse_epub(epub_path).await?;
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
        write_posts(
            &temp_dir,
            &Posts {
                posts: parsed.posts,
            },
        )
        .await?;
        Ok(ParsedUpload {
            title,
            author,
            handle,
            is_sample,
            post_count,
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
    field: axum::extract::multipart::Field<'_>,
    max: usize,
) -> Result<String, ApiError> {
    let bytes = field.bytes().await?;
    if bytes.len() > max {
        return Err(ApiError::BadRequest(format!(
            "Field is longer than {max} bytes."
        )));
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
