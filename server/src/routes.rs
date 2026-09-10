use crate::auth::{check_password, hash_password, new_token, normalize_username, verify_password, Authed};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::models::{
    AuthRequest, AuthResponse, BookRecord, BookRow, LibrarySnapshot, LikeSet, MeResponse, PatchBook,
};
use crate::AppState;
use axum::extract::{Multipart, Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use axum::Json;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

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
    headers: axum::http::HeaderMap,
) -> Result<axum::http::StatusCode, ApiError> {
    let _ = authed;
    if let Some(token) = bearer_token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token)
            .execute(&state.pool)
            .await?;
    }
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
    let rows = sqlx::query_as::<_, BookRow>(
        "SELECT id, title, author, handle, imported_at, post_count, progress_index, is_sample
         FROM books
         WHERE user_id = ?
         ORDER BY imported_at DESC",
    )
    .bind(&authed.user_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(LibrarySnapshot {
        books: rows.into_iter().map(BookRow::into_record).collect(),
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
    tokio::fs::create_dir_all(&dest).await?;
    tokio::fs::rename(&parsed.epub_path, dest.join("book.epub")).await?;
    if let Some(cover) = parsed.cover_path {
        tokio::fs::rename(&cover, dest.join("cover.jpg")).await?;
    }
    let _ = tokio::fs::remove_dir_all(&parsed.temp_dir).await;

    let inserted = sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample)
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?)",
    )
    .bind(&id)
    .bind(&authed.user_id)
    .bind(&parsed.title)
    .bind(&parsed.author)
    .bind(&parsed.handle)
    .bind(imported_at)
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

    Ok(Json(BookRecord {
        id,
        title: parsed.title,
        author: parsed.author,
        handle: parsed.handle,
        imported_at,
        post_count: 0,
        progress_index: 0,
        is_sample: parsed.is_sample,
    }))
}

pub async fn get_book(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<BookRecord>, ApiError> {
    Ok(Json(owned_book(&state, &authed.user_id, &id).await?))
}

pub async fn delete_book(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let _ = owned_book(&state, &authed.user_id, &id).await?;
    sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&authed.user_id)
        .execute(&state.pool)
        .await?;
    let dest = book_dir(&state.data_dir, &authed.user_id, &id);
    let _ = tokio::fs::remove_dir_all(dest).await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn patch_book(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
    Json(body): Json<PatchBook>,
) -> Result<Json<BookRecord>, ApiError> {
    let current = owned_book(&state, &authed.user_id, &id).await?;
    let post_count = body.post_count.unwrap_or(current.post_count);
    let progress_index = body.progress_index.unwrap_or(current.progress_index).max(0);
    sqlx::query(
        "UPDATE books SET post_count = ?, progress_index = ? WHERE id = ? AND user_id = ?",
    )
    .bind(post_count)
    .bind(progress_index)
    .bind(&id)
    .bind(&authed.user_id)
    .execute(&state.pool)
    .await?;
    Ok(Json(owned_book(&state, &authed.user_id, &id).await?))
}

pub async fn download_epub(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let _ = owned_book(&state, &authed.user_id, &id).await?;
    let path = book_dir(&state.data_dir, &authed.user_id, &id).join("book.epub");
    file_response(path, "application/epub+zip", "book.epub").await
}

pub async fn download_cover(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let _ = owned_book(&state, &authed.user_id, &id).await?;
    let path = book_dir(&state.data_dir, &authed.user_id, &id).join("cover.jpg");
    if !path.exists() {
        return Err(ApiError::NotFound);
    }
    file_response(path, "image/jpeg", "cover.jpg").await
}

pub async fn list_likes(
    State(state): State<AppState>,
    authed: Authed,
    Path(id): Path<String>,
) -> Result<Json<LikeSet>, ApiError> {
    let _ = owned_book(&state, &authed.user_id, &id).await?;
    Ok(Json(likes_for(&state, &authed.user_id, &id).await?))
}

pub async fn toggle_like(
    State(state): State<AppState>,
    authed: Authed,
    Path((id, post_id)): Path<(String, String)>,
) -> Result<Json<LikeSet>, ApiError> {
    let _ = owned_book(&state, &authed.user_id, &id).await?;
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

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

async fn owned_book(state: &AppState, user_id: &str, id: &str) -> Result<BookRecord, ApiError> {
    let row = sqlx::query_as::<_, BookRow>(
        "SELECT id, title, author, handle, imported_at, post_count, progress_index, is_sample
         FROM books
         WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(BookRow::into_record).ok_or(ApiError::NotFound)
}

async fn existing_sample(state: &AppState, user_id: &str) -> Result<Option<BookRecord>, ApiError> {
    let row = sqlx::query_as::<_, BookRow>(
        "SELECT id, title, author, handle, imported_at, post_count, progress_index, is_sample
         FROM books
         WHERE user_id = ? AND is_sample = 1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row.map(BookRow::into_record))
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
    epub_path: PathBuf,
    cover_path: Option<PathBuf>,
    temp_dir: PathBuf,
}

async fn read_upload(state: &AppState, mut multipart: Multipart) -> Result<ParsedUpload, ApiError> {
    let temp_dir = state
        .data_dir
        .join("tmp")
        .join(Uuid::new_v4().to_string());
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
                "title" => title = Some(field.text().await?.trim().to_string()),
                "author" => author = Some(field.text().await?.trim().to_string()),
                "handle" => handle = Some(field.text().await?.trim().to_string()),
                "isSample" | "is_sample" => {
                    let value = field.text().await?;
                    is_sample = matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes"
                    );
                }
                "epub" => {
                    let dest = temp_dir.join("book.epub");
                    write_limited(field, &dest, state.max_epub_bytes).await?;
                    epub_path = Some(dest);
                }
                "cover" => {
                    let dest = temp_dir.join("cover.jpg");
                    write_limited(field, &dest, state.max_epub_bytes).await?;
                    cover_path = Some(dest);
                }
                _ => {
                    let _ = field.bytes().await;
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

    let title = title.filter(|s| !s.is_empty()).ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        ApiError::BadRequest("Title is required.".into())
    })?;
    let author = author.filter(|s| !s.is_empty()).ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        ApiError::BadRequest("Author is required.".into())
    })?;
    let handle = handle.filter(|s| !s.is_empty()).unwrap_or_else(|| "author".into());
    let Some(epub_path) = epub_path else {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(ApiError::BadRequest("EPUB file is required.".into()));
    };

    Ok(ParsedUpload {
        title,
        author,
        handle,
        is_sample,
        epub_path,
        cover_path,
        temp_dir,
    })
}

async fn write_limited(
    mut field: axum::extract::multipart::Field<'_>,
    dest: &std::path::Path,
    limit: u64,
) -> Result<(), ApiError> {
    if let Some(len) = field.headers().typed_get_content_length() {
        if len > limit {
            return Err(ApiError::PayloadTooLarge {
                asked: len,
                limit,
            });
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
    let file = tokio::fs::File::open(&path).await.map_err(|_| ApiError::NotFound)?;
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
