//! The only code that creates a book.
//!
//! Uploads and the request worker both finish here, so ownership, `shared = 0`,
//! the progress row, and the search feed stay on one path.

use crate::db::now_ms;
use crate::error::ApiError;
use crate::feed::{slug, Posts};
use crate::models::{BookRecord, BookRow};
use crate::search;
use crate::visibility::{BOOK_SELECT, BOOK_VISIBLE_TO};
use crate::AppState;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

/// An EPUB staged in `data_dir/tmp/<uuid>/` (`book.epub`, optional `cover.jpg`)
/// and already parsed. Dropping it without [`import`] leaves the tmp dir; both
/// callers remove it on error, as `read_upload` does today.
pub(crate) struct Staged {
    pub title: String,
    pub author: String,
    pub handle: String,
    pub is_sample: bool,
    pub post_count: i32,
    pub posts: Posts,
    pub temp_dir: PathBuf,
    pub meta: crate::epub::OpfMeta,
}

impl Staged {
    /// Catalog titles win over the release's OPF (release OPFs are often junk).
    pub fn with_titles(mut self, title: &str, author: &str) -> Self {
        self.title = title.to_string();
        self.author = author.to_string();
        self.handle = slug(author);
        self
    }
}

pub(crate) struct Owner<'a> {
    pub user_id: &'a str,
    pub username: &'a str,
}

pub(crate) enum NewId {
    Random,
    Fixed(String),
}

/// Copies `src` (a regular file, not a symlink) into a fresh tmp dir as
/// `book.epub`, stopping at `limit` bytes ([`ApiError::PayloadTooLarge`]), then
/// runs the same parse as uploads. A non-EPUB fails here with the parser's
/// 400 message. A full parse queue is [`ApiError::Busy`] before the copy.
pub(crate) async fn stage_file(
    state: &AppState,
    src: &Path,
    limit: u64,
) -> Result<Staged, ApiError> {
    let meta = tokio::fs::symlink_metadata(src)
        .await
        .map_err(|_| not_a_file())?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(not_a_file());
    }
    if meta.len() > limit {
        return Err(ApiError::PayloadTooLarge {
            asked: meta.len(),
            limit,
        });
    }
    // The worker retries this whole call. Take the permit first so a busy
    // parser does not copy the EPUB into tmp and delete it five minutes later.
    let permit = state.parsers.try_acquire()?;
    let temp_dir = state.data_dir.join("tmp").join(Uuid::new_v4().to_string());
    tokio::fs::create_dir_all(&temp_dir).await?;
    let dest = temp_dir.join("book.epub");
    let copied = copy_limited(src, &dest, limit).await;
    if let Err(err) = copied {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return Err(err);
    }
    let staged = staged_from_epub(&state.parsers, temp_dir.clone(), dest, false, permit).await;
    if staged.is_err() {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
    staged
}

/// Today's upload tail: rename into the book dir, insert the book, the progress
/// row, and the search feed, and clean up on failure. With [`NewId::Fixed`], an
/// existing book with that id and owner is returned and the stage is dropped, so
/// a crash between the insert and the request update can't make a second copy.
pub(crate) async fn import(
    state: &AppState,
    owner: &Owner<'_>,
    id: NewId,
    staged: Staged,
) -> Result<BookRecord, ApiError> {
    if let NewId::Fixed(ref fixed) = id {
        if let Some(existing) = owned_book(state, owner, fixed).await? {
            let _ = tokio::fs::remove_dir_all(&staged.temp_dir).await;
            return Ok(existing);
        }
    }
    if matches!(id, NewId::Random) && staged.is_sample {
        if let Some(existing) = existing_sample(state, owner.user_id).await? {
            let _ = tokio::fs::remove_dir_all(&staged.temp_dir).await;
            return Ok(existing);
        }
    }

    let fixed_id = matches!(id, NewId::Fixed(_));
    let id = match id {
        NewId::Random => Uuid::new_v4().to_string(),
        NewId::Fixed(id) => id,
    };
    let imported_at = now_ms();
    let dest = book_dir(&state.data_dir, owner.user_id, &id);
    // A crash after the rename and before the books insert leaves the directory
    // with no row. The next Fixed import clears it. A row for this id already
    // returned above.
    if fixed_id {
        match tokio::fs::symlink_metadata(&dest).await {
            Ok(meta) if meta.file_type().is_symlink() || meta.is_dir() => {
                tokio::fs::remove_dir_all(&dest).await?;
            }
            Ok(_) => tokio::fs::remove_file(&dest).await?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    tokio::fs::create_dir_all(dest.parent().ok_or(ApiError::Internal)?).await?;
    tokio::fs::rename(&staged.temp_dir, &dest).await?;

    let inserted = sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)",
    )
    .bind(&id)
    .bind(owner.user_id)
    .bind(&staged.title)
    .bind(&staged.author)
    .bind(&staged.handle)
    .bind(imported_at)
    .bind(staged.post_count)
    .bind(if staged.is_sample { 1 } else { 0 })
    .execute(&state.pool)
    .await;

    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() && staged.is_sample => {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            let existing = existing_sample(state, owner.user_id)
                .await?
                .ok_or(ApiError::NotFound)?;
            return Ok(existing);
        }
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            if let Some(existing) = owned_book(state, owner, &id).await? {
                return Ok(existing);
            }
            return Err(sqlx::Error::Database(err).into());
        }
        Err(err) => {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            return Err(err.into());
        }
    }

    if let Err(err) =
        sqlx::query("INSERT INTO book_progress (user_id, book_id, progress_index) VALUES (?, ?, 0)")
            .bind(owner.user_id)
            .bind(&id)
            .execute(&state.pool)
            .await
    {
        return Err(err.into());
    }

    if let Err(err) = search::store_feed(&state.pool, &dest, &id, &staged.posts).await {
        let _ = sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
            .bind(&id)
            .bind(owner.user_id)
            .execute(&state.pool)
            .await;
        let _ = tokio::fs::remove_dir_all(&dest).await;
        return Err(err);
    }

    Ok(BookRecord {
        id,
        title: staged.title,
        author: staged.author,
        handle: staged.handle,
        imported_at,
        post_count: staged.post_count,
        progress_index: 0,
        is_sample: staged.is_sample,
        mine: true,
        shared: false,
        owner_username: owner.username.to_string(),
        held: false,
    })
}

/// Deletes a book this process just inserted. Used when a cancel wins the
/// compare-and-set after import. `files_user_id` is still null, so the
/// directory is under `user_id`.
pub(crate) async fn remove(state: &AppState, user_id: &str, book_id: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
        .bind(book_id)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    let _ = tokio::fs::remove_dir_all(book_dir(&state.data_dir, user_id, book_id)).await;
    Ok(())
}

/// The epub directory. `files_owner` is `books.files_user_id`, or `user_id`
/// when that column is null. Transfers change the owner row and leave the
/// directory here.
pub(crate) fn book_dir(data_dir: &Path, files_owner: &str, book_id: &str) -> PathBuf {
    data_dir.join("books").join(files_owner).join(book_id)
}

/// The owner is deleting a book someone else holds. The earliest holder
/// becomes the owner, sharing goes back to private, and the previous owner's
/// rows on this book go away. The file stays put.
///
/// Account deletion does not exist (`admin` only grants and revokes). If it
/// is added, it must call this before the `users` cascade, or a held book
/// disappears with the account that first imported it.
pub(crate) async fn pass_ownership(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
    from_user: &str,
    to_user: &str,
) -> Result<(), ApiError> {
    let updated = sqlx::query(
        "UPDATE books
         SET user_id = ?, files_user_id = COALESCE(files_user_id, user_id), shared = 0
         WHERE id = ? AND user_id = ?",
    )
    .bind(to_user)
    .bind(book_id)
    .bind(from_user)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Err(ApiError::NotFound);
    }
    for table in ["comments", "reposts", "likes", "book_progress"] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE book_id = ? AND user_id = ?"
        ))
        .bind(book_id)
        .bind(from_user)
        .execute(&mut **tx)
        .await?;
    }
    crate::requests::cancel_imported(&mut **tx, from_user, book_id).await?;
    Ok(())
}

/// A holder is leaving the book. Their access and the rows they wrote while
/// the book was private go away. Rows written while it was shared stay, and
/// the file is untouched. Returns false when this account does not hold it.
pub(crate) async fn release_hold(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: &str,
    book_id: &str,
) -> Result<bool, ApiError> {
    let now = now_ms();
    let changed = sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', book_id = NULL, error = NULL, download_id = NULL,
             updated_at = ?, next_check_at = ?
         WHERE book_id = ? AND user_id = ? AND state = 'imported'",
    )
    .bind(now)
    .bind(now)
    .bind(book_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Ok(false);
    }
    for table in ["book_progress", "likes"] {
        sqlx::query(&format!(
            "DELETE FROM {table} WHERE user_id = ? AND book_id = ?"
        ))
        .bind(user_id)
        .bind(book_id)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "DELETE FROM comments WHERE user_id = ? AND book_id = ? AND private = 1 -- own rows only",
    )
    .bind(user_id)
    .bind(book_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "DELETE FROM reposts WHERE user_id = ? AND book_id = ? AND private = 1 -- own rows only",
    )
    .bind(user_id)
    .bind(book_id)
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

async fn staged_from_epub(
    parsers: &crate::parse_pool::ParsePool,
    temp_dir: PathBuf,
    epub_path: PathBuf,
    is_sample: bool,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Staged, ApiError> {
    let parsed = parsers.parse_owned(epub_path, permit).await?;
    let title = parsed
        .title
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled".into());
    let author = parsed
        .author
        .filter(|author| !author.is_empty())
        .unwrap_or_else(|| "Unknown".into());
    if let Some(cover) = parsed.cover {
        tokio::fs::write(temp_dir.join("cover.jpg"), cover).await?;
    }
    finish(
        temp_dir,
        title,
        author,
        is_sample,
        parsed.posts,
        parsed.meta,
    )
    .await
}

async fn finish(
    temp_dir: PathBuf,
    title: String,
    author: String,
    is_sample: bool,
    posts: Vec<crate::feed::Post>,
    meta: crate::epub::OpfMeta,
) -> Result<Staged, ApiError> {
    let post_count = i32::try_from(posts.len()).map_err(|_| {
        ApiError::BadRequest(format!(
            "Post count exceeds BookRecord's signed 32-bit representation (asked {}).",
            posts.len()
        ))
    })?;
    Ok(Staged {
        handle: slug(&author),
        title,
        author,
        is_sample,
        post_count,
        posts: Posts { posts },
        temp_dir,
        meta,
    })
}

async fn copy_limited(src: &Path, dest: &Path, limit: u64) -> Result<(), ApiError> {
    let mut input = tokio::fs::File::open(src).await?;
    let mut output = tokio::fs::File::create(dest).await?;
    let mut written = 0u64;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        written += n as u64;
        if written > limit {
            drop(output);
            let _ = tokio::fs::remove_file(dest).await;
            return Err(ApiError::PayloadTooLarge {
                asked: written,
                limit,
            });
        }
        output.write_all(&buf[..n]).await?;
    }
    output.flush().await?;
    Ok(())
}

async fn owned_book(
    state: &AppState,
    owner: &Owner<'_>,
    id: &str,
) -> Result<Option<BookRecord>, ApiError> {
    let row =
        sqlx::query_as::<_, BookRow>(&format!("{BOOK_SELECT} WHERE b.id = ? AND b.user_id = ?"))
            .bind(owner.user_id)
            .bind(owner.user_id)
            .bind(id)
            .bind(owner.user_id)
            .fetch_optional(&state.pool)
            .await?;
    Ok(row.map(|row| row.into_record(owner.user_id)))
}

async fn existing_sample(state: &AppState, user_id: &str) -> Result<Option<BookRecord>, ApiError> {
    let row = sqlx::query_as::<_, BookRow>(&format!(
        "{BOOK_SELECT} WHERE b.is_sample = 1 AND {BOOK_VISIBLE_TO} ORDER BY b.imported_at ASC LIMIT 1"
    ))
    .bind(user_id)
    .bind(user_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    Ok(row.map(|row| row.into_record(user_id)))
}

fn not_a_file() -> ApiError {
    ApiError::BadRequest("Could not read that EPUB: not a file.".into())
}
