//! Account, book, and post search, plus the only writer of a book's feed.
//!
//! `store_feed` writes `posts.json`, the `posts` rows, and `books.posts_indexed`
//! together. Upload and the legacy cache fill both call it. Startup backfill
//! calls it for every book still marked unindexed.

use crate::auth::Authed;
use crate::config::MAX_COVER_BYTES;
use crate::error::ApiError;
use crate::feed::{Post, PostKind, Posts};
use crate::models::{AccountRecord, BookRow, PostHitRecord, SearchHits, SearchKind, SearchPage};
use crate::visibility::{BOOK_SELECT, BOOK_VISIBLE_TO};
use crate::AppState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sqlx::{FromRow, SqlitePool};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const SEARCH_LIMIT: i64 = 20;
pub(crate) const PROFILE_POSTS_LIMIT: i64 = 30;
const MAX_QUERY_CHARS: usize = 100;

pub(crate) async fn run(
    pool: &SqlitePool,
    viewer: &Authed,
    q: &str,
    kind: SearchKind,
    cursor: Option<&str>,
) -> Result<SearchPage, ApiError> {
    let q = q.trim();
    if q.is_empty() {
        return Err(ApiError::BadRequest("Enter a search.".into()));
    }
    let asked = q.chars().count();
    if asked > MAX_QUERY_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Search is longer than {MAX_QUERY_CHARS} characters (asked {asked})."
        )));
    }
    let offset = match cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) {
        Some(cursor) => decode_offset(cursor)?,
        None => 0,
    };
    match kind {
        SearchKind::Accounts => accounts(pool, viewer, q, offset).await,
        SearchKind::Books => books(pool, viewer, q, offset).await,
        SearchKind::Posts => posts(pool, viewer, q, offset).await,
    }
}

/// Words become a quoted FTS5 query. The last word is a prefix. Operators and
/// punctuation never reach MATCH. `None` means the query has no word characters,
/// so the caller returns an empty page instead of handing FTS an empty match.
pub(crate) fn fts_match(q: &str) -> Option<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in q.chars() {
        if c.is_alphanumeric() {
            word.push(c);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    if words.is_empty() {
        return None;
    }
    let last = words.len() - 1;
    let mut out = String::new();
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        out.push_str(word);
        out.push('"');
        if i == last {
            out.push('*');
        }
    }
    Some(out)
}

/// Escape `\`, `%`, and `_` so the query is a literal LIKE pattern.
pub(crate) fn like_pattern(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

async fn accounts(
    pool: &SqlitePool,
    viewer: &Authed,
    q: &str,
    offset: i64,
) -> Result<SearchPage, ApiError> {
    // username_norm is the ASCII-lowercase username, so fold the query the same way.
    let folded = q.to_ascii_lowercase();
    let infix = format!("%{}%", like_pattern(&folded));
    let prefix = format!("{}%", like_pattern(&folded));
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, username FROM users
         WHERE username_norm LIKE ? ESCAPE '\\'
         ORDER BY CASE WHEN username_norm LIKE ? ESCAPE '\\' THEN 0 ELSE 1 END,
                  length(username_norm),
                  username_norm
         LIMIT ? OFFSET ?",
    )
    .bind(&infix)
    .bind(&prefix)
    .bind(SEARCH_LIMIT + 1)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let (rows, next_cursor) = page(rows, offset);
    Ok(SearchPage {
        hits: SearchHits::Accounts {
            items: rows
                .into_iter()
                .map(|(id, username)| AccountRecord {
                    mine: id == viewer.user_id,
                    username,
                })
                .collect(),
        },
        next_cursor,
    })
}

async fn books(
    pool: &SqlitePool,
    viewer: &Authed,
    q: &str,
    offset: i64,
) -> Result<SearchPage, ApiError> {
    let infix = format!("%{}%", like_pattern(q));
    let prefix = format!("{}%", like_pattern(q));
    let sql = format!(
        "{BOOK_SELECT}
         WHERE {BOOK_VISIBLE_TO}
           AND (
             b.title LIKE ? ESCAPE '\\'
             OR b.author LIKE ? ESCAPE '\\'
             OR b.handle LIKE ? ESCAPE '\\'
             OR owner.username LIKE ? ESCAPE '\\'
           )
         ORDER BY CASE WHEN b.title LIKE ? ESCAPE '\\' THEN 0 ELSE 1 END,
                  b.imported_at DESC,
                  b.id ASC
         LIMIT ? OFFSET ?"
    );
    let rows = sqlx::query_as::<_, BookRow>(&sql)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(&infix)
        .bind(&infix)
        .bind(&infix)
        .bind(&infix)
        .bind(&prefix)
        .bind(SEARCH_LIMIT + 1)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    let (rows, next_cursor) = page(rows, offset);
    Ok(SearchPage {
        hits: SearchHits::Books {
            items: rows
                .into_iter()
                .map(|row| row.into_record(&viewer.user_id))
                .collect(),
        },
        next_cursor,
    })
}

async fn posts(
    pool: &SqlitePool,
    viewer: &Authed,
    q: &str,
    offset: i64,
) -> Result<SearchPage, ApiError> {
    let Some(match_query) = fts_match(q) else {
        return Ok(SearchPage {
            hits: SearchHits::Posts { items: vec![] },
            next_cursor: None,
        });
    };
    // The visible-books CTE is the only visibility check in this statement.
    // bm25 is the relevance order; id breaks ties so offset pages stay stable.
    let sql = format!(
        "WITH visible_books AS (
             SELECT b.id AS id
             FROM books b
             WHERE {BOOK_VISIBLE_TO}
         )
         SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
                COALESCE(p.progress_index, 0) AS progress_index,
                b.is_sample, b.shared, b.user_id AS owner_id,
                owner.username AS owner_username,
                posts.idx AS post_idx, posts.kind AS post_kind,
                posts.chapter AS post_chapter, posts.text AS post_text
         FROM post_fts
         JOIN posts ON posts.rowid = post_fts.rowid
         JOIN visible_books ON visible_books.id = posts.book_id
         JOIN books b ON b.id = posts.book_id
         JOIN users owner ON owner.id = b.user_id
         LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
         WHERE post_fts MATCH ?
         ORDER BY bm25(post_fts), b.imported_at DESC, posts.idx ASC, b.id ASC
         LIMIT ? OFFSET ?"
    );
    let rows = sqlx::query_as::<_, PostHitRow>(&sql)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(&match_query)
        .bind(SEARCH_LIMIT + 1)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    let (rows, next_cursor) = page(rows, offset);
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.into_hit(&viewer.user_id)?);
    }
    Ok(SearchPage {
        hits: SearchHits::Posts { items },
        next_cursor,
    })
}

#[derive(Debug, FromRow)]
struct PostHitRow {
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
}

impl PostHitRow {
    fn into_hit(self, viewer: &str) -> Result<PostHitRecord, ApiError> {
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
        Ok(PostHitRecord {
            book,
            post: Post {
                id: self.post_idx.to_string(),
                kind: parse_kind(&self.post_kind)?,
                text: self.post_text,
                chapter: self.post_chapter,
            },
        })
    }
}

pub(crate) fn parse_kind(kind: &str) -> Result<PostKind, ApiError> {
    match kind {
        "Heading" => Ok(PostKind::Heading),
        "Body" => Ok(PostKind::Body),
        _ => {
            tracing::error!(kind, "stored post kind is not Heading or Body");
            Err(ApiError::Internal)
        }
    }
}

fn page<T>(mut rows: Vec<T>, offset: i64) -> (Vec<T>, Option<String>) {
    if rows.len() > SEARCH_LIMIT as usize {
        rows.truncate(SEARCH_LIMIT as usize);
        let next = offset.checked_add(SEARCH_LIMIT).map(encode_offset);
        (rows, next)
    } else {
        (rows, None)
    }
}

fn encode_offset(offset: i64) -> String {
    STANDARD.encode(offset.to_string())
}

fn decode_offset(cursor: &str) -> Result<i64, ApiError> {
    let text = decode_text(cursor)?;
    let offset: i64 = text.parse().map_err(|_| bad_cursor())?;
    if offset < 0 {
        return Err(bad_cursor());
    }
    Ok(offset)
}

pub(crate) fn encode_keyset(created_at: i64, id: &str) -> String {
    STANDARD.encode(format!("{created_at}:{id}"))
}

pub(crate) fn decode_keyset(cursor: &str) -> Result<(i64, String), ApiError> {
    let text = decode_text(cursor)?;
    let (raw_at, id) = text.split_once(':').ok_or_else(bad_cursor)?;
    if id.is_empty() {
        return Err(bad_cursor());
    }
    let created_at: i64 = raw_at.parse().map_err(|_| bad_cursor())?;
    Ok((created_at, id.to_string()))
}

fn decode_text(cursor: &str) -> Result<String, ApiError> {
    let bytes = STANDARD.decode(cursor).map_err(|_| bad_cursor())?;
    String::from_utf8(bytes).map_err(|_| bad_cursor())
}

fn bad_cursor() -> ApiError {
    ApiError::BadRequest("That page token is not valid.".into())
}

/// Writes the feed file and the search rows in one database transaction.
/// The file is written before commit, so a crash leaves `posts_indexed` at 0
/// and the next backfill retries. Returns the bytes stored in `posts.json`.
pub(crate) async fn store_feed(
    pool: &SqlitePool,
    dir: &Path,
    book_id: &str,
    posts: &Posts,
) -> Result<Vec<u8>, ApiError> {
    let bytes = serde_json::to_vec(posts).map_err(|err| {
        tracing::error!(error = %err, "could not encode posts.json");
        ApiError::Internal
    })?;
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM posts WHERE book_id = ?")
        .bind(book_id)
        .execute(&mut *tx)
        .await?;
    for post in &posts.posts {
        let idx: i64 = post.id.parse().map_err(|_| {
            tracing::error!(id = %post.id, book_id, "post id is not an index");
            ApiError::Internal
        })?;
        sqlx::query("INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES (?, ?, ?, ?, ?)")
            .bind(book_id)
            .bind(idx)
            .bind(kind_label(&post.kind))
            .bind(&post.chapter)
            .bind(&post.text)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE books SET posts_indexed = 1, post_count = ? WHERE id = ?")
        .bind(posts.posts.len() as i64)
        .bind(book_id)
        .execute(&mut *tx)
        .await?;
    write_posts_file(dir, &bytes).await?;
    tx.commit().await?;
    Ok(bytes)
}

fn kind_label(kind: &PostKind) -> &'static str {
    match kind {
        PostKind::Heading => "Heading",
        PostKind::Body => "Body",
    }
}

async fn write_posts_file(dir: &Path, bytes: &[u8]) -> Result<(), ApiError> {
    let temp = dir.join(format!("posts-{}.tmp", Uuid::new_v4()));
    tokio::fs::write(&temp, bytes).await?;
    if let Err(err) = tokio::fs::rename(&temp, dir.join("posts.json")).await {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(err.into());
    }
    Ok(())
}

pub(crate) async fn read_posts(dir: &Path) -> Result<Option<Vec<u8>>, ApiError> {
    match tokio::fs::read(dir.join("posts.json")).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// Indexes every book still at `posts_indexed = 0`. Safe to run again: a book
/// is marked only after its file and rows commit, and a second pass skips it.
pub(crate) async fn backfill(state: AppState) {
    let pending = sqlx::query_as::<_, (String, String)>(
        "SELECT id, user_id FROM books WHERE posts_indexed = 0 ORDER BY imported_at ASC, id ASC",
    )
    .fetch_all(&state.pool)
    .await;
    let pending = match pending {
        Ok(pending) => pending,
        Err(err) => {
            tracing::error!(error = %err, "search backfill could not list books");
            return;
        }
    };
    let mut indexed = 0u64;
    for (book_id, user_id) in &pending {
        match index_pending(&state, book_id, user_id).await {
            Ok(true) => indexed += 1,
            Ok(false) => {}
            Err(err) => {
                tracing::error!(error = %err, %book_id, "search backfill failed for a book");
            }
        }
    }
    tracing::info!(indexed, pending = pending.len(), "search backfill finished");
}

/// `Ok(true)` when this call stored the feed. `Ok(false)` when another caller
/// indexed the book after the pending list was read.
async fn index_pending(state: &AppState, book_id: &str, user_id: &str) -> Result<bool, ApiError> {
    let _guard = state.lazy_parse_lock.lock().await;
    let indexed: Option<i64> = sqlx::query_scalar("SELECT posts_indexed FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(&state.pool)
        .await?;
    if indexed != Some(0) {
        return Ok(false);
    }
    let dir = book_dir(&state.data_dir, user_id, book_id);
    let posts = match read_posts(&dir).await? {
        Some(bytes) => serde_json::from_slice::<Posts>(&bytes).map_err(|err| {
            tracing::error!(error = %err, book_id, "posts.json is not a feed");
            ApiError::Internal
        })?,
        None => posts_from_epub(dir.join("book.epub")).await?,
    };
    store_feed(&state.pool, &dir, book_id, &posts).await?;
    Ok(true)
}

async fn posts_from_epub(path: PathBuf) -> Result<Posts, ApiError> {
    let parsed =
        tokio::task::spawn_blocking(move || crate::epub::parse_file(&path, MAX_COVER_BYTES))
            .await
            .map_err(|err| {
                tracing::error!(error = %err, "EPUB parser task failed");
                ApiError::Internal
            })?
            .map_err(|reason| {
                ApiError::BadRequest(format!("Could not read that EPUB: {reason}"))
            })?;
    Ok(Posts {
        posts: parsed.posts,
    })
}

fn book_dir(data_dir: &Path, user_id: &str, book_id: &str) -> PathBuf {
    data_dir.join("books").join(user_id).join(book_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_match_quotes_terms_and_prefixes_the_last() {
        assert_eq!(
            fts_match("first para").as_deref(),
            Some(r#""first" "para"*"#)
        );
        assert_eq!(
            fts_match("first OR second").as_deref(),
            Some(r#""first" "OR" "second"*"#)
        );
        assert_eq!(
            fts_match(r#"" * NEAR - ( )"#).as_deref(),
            Some(r#""NEAR"*"#)
        );
        assert_eq!(fts_match("***").as_deref(), None);
        assert_eq!(fts_match("   ").as_deref(), None);
        assert_eq!(like_pattern(r"100%_a\b"), r"100\%\_a\\b");
    }
}
