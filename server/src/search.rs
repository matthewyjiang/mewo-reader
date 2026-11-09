//! Account, book, and post search, plus the only writer of a book's feed.
//!
//! `store_feed` writes `posts.json`, the `posts` rows, and `books.posts_indexed`
//! together. Upload and the legacy cache fill both call it. Startup backfill
//! calls it for every book still marked unindexed.

use crate::auth::Authed;
use crate::error::ApiError;
use crate::feed::{Post, PostKind, Posts};
use crate::models::{AccountRecord, BookRow, PostHitRecord, SearchHits, SearchKind, SearchPage};
use crate::visibility::{social_visible, BOOK_VISIBLE_TO};
use crate::AppState;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sqlx::{Connection, FromRow, SqlitePool};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const SEARCH_LIMIT: i64 = 20;
pub(crate) const PROFILE_POSTS_LIMIT: i64 = 30;
const MAX_QUERY_CHARS: usize = 100;
const BROWSE_ACCOUNTS: i64 = 20;
const BROWSE_BOOKS: i64 = 30;
const BROWSE_POSTS: i64 = 20;
/// Comments and reposts older than this do not move a browse ranking.
const WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;

pub(crate) async fn run(
    pool: &SqlitePool,
    viewer: &Authed,
    q: &str,
    kind: SearchKind,
    cursor: Option<&str>,
) -> Result<SearchPage, ApiError> {
    let q = q.trim();
    if q.is_empty() {
        // A cursor belongs to a typed query. Browse is one page, so it is ignored.
        let _ = cursor;
        return browse(pool, viewer, kind).await;
    }
    let asked = q.chars().count();
    if asked > MAX_QUERY_CHARS {
        return Err(ApiError::BadRequest(format!(
            "Search is longer than {MAX_QUERY_CHARS} characters (asked {asked})."
        )));
    }
    match kind {
        SearchKind::Accounts => accounts(pool, viewer, q, cursor).await,
        SearchKind::Books => books(pool, viewer, q, cursor).await,
        SearchKind::Posts => posts(pool, viewer, q, cursor).await,
    }
}

/// One curated page. `nextCursor` stays empty so the client does not ask for more.
async fn browse(
    pool: &SqlitePool,
    viewer: &Authed,
    kind: SearchKind,
) -> Result<SearchPage, ApiError> {
    let now = crate::db::now_ms();
    match kind {
        SearchKind::Accounts => suggested_accounts(pool, viewer, now).await,
        SearchKind::Books => popular_books(pool, viewer, now).await,
        SearchKind::Posts => top_posts(pool, viewer, now).await,
    }
}

/// Other accounts, busiest first on books this viewer can see.
/// Private-copy replies and reposts do not count, so a number cannot leak them.
async fn suggested_accounts(
    pool: &SqlitePool,
    viewer: &Authed,
    now: i64,
) -> Result<SearchPage, ApiError> {
    let since = now.saturating_sub(WINDOW_MS);
    let sql = format!(
        "WITH vis AS (SELECT b.id FROM books b WHERE {BOOK_VISIBLE_TO}),
         act AS (
             SELECT c.user_id AS uid, COUNT(*) AS n
             FROM comments c
             JOIN vis ON vis.id = c.book_id
             JOIN books b ON b.id = c.book_id
             WHERE c.created_at >= ? AND {social_c}
             GROUP BY c.user_id
             UNION ALL
             SELECT r.user_id, COUNT(*)
             FROM reposts r
             JOIN vis ON vis.id = r.book_id
             JOIN books b ON b.id = r.book_id
             WHERE r.created_at >= ? AND {social_r}
             GROUP BY r.user_id
         )
         SELECT u.username
         FROM users u
         LEFT JOIN act ON act.uid = u.id
         WHERE u.id <> ?
         GROUP BY u.id
         ORDER BY COALESCE(SUM(act.n), 0) DESC, u.created_at DESC, u.username_norm ASC
         LIMIT {BROWSE_ACCOUNTS}",
        social_c = social_visible("c"),
        social_r = social_visible("r"),
    );
    let rows = sqlx::query_as::<_, SuggestedAccount>(&sql)
        .bind(&viewer.user_id)
        .bind(since)
        .bind(&viewer.user_id)
        .bind(since)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .fetch_all(pool)
        .await?;
    Ok(SearchPage {
        hits: SearchHits::Accounts {
            items: rows
                .into_iter()
                .map(|row| AccountRecord {
                    mine: false,
                    username: row.username,
                })
                .collect(),
        },
        next_cursor: None,
    })
}

/// 3× readers + 2× comments in the window + reposts in the window.
/// Readers are this account, or anyone on a shared or sample book: another
/// person's private reading does not move the rank. The sample sorts last.
async fn popular_books(
    pool: &SqlitePool,
    viewer: &Authed,
    now: i64,
) -> Result<SearchPage, ApiError> {
    let since = now.saturating_sub(WINDOW_MS);
    let sql = format!(
        "WITH vis AS (SELECT b.id FROM books b WHERE {BOOK_VISIBLE_TO}),
         readers AS (
             SELECT bp.book_id AS book_id, COUNT(*) AS n
             FROM book_progress bp
             JOIN vis ON vis.id = bp.book_id
             JOIN books b ON b.id = bp.book_id
             WHERE bp.progress_index > 0
               AND (bp.user_id = ? OR b.shared = 1 OR b.is_sample = 1)
             GROUP BY bp.book_id
         ),
         talk AS (
             SELECT c.book_id AS book_id, COUNT(*) AS n
             FROM comments c
             JOIN vis ON vis.id = c.book_id
             JOIN books b ON b.id = c.book_id
             WHERE c.created_at >= ? AND {social_c}
             GROUP BY c.book_id
         ),
         shares AS (
             SELECT r.book_id AS book_id, COUNT(*) AS n
             FROM reposts r
             JOIN vis ON vis.id = r.book_id
             JOIN books b ON b.id = r.book_id
             WHERE r.created_at >= ? AND {social_r}
             GROUP BY r.book_id
         )
         SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
                COALESCE(p.progress_index, 0) AS progress_index,
                b.is_sample, b.shared, b.user_id AS owner_id,
                owner.username AS owner_username, b.feed_rev,
                (b.replaced_at IS NULL AND EXISTS (
                    SELECT 1 FROM book_requests lr
                    WHERE lr.id = b.id AND lr.release_title IS NOT NULL
                )) AS has_release,
                EXISTS (
                    SELECT 1 FROM book_requests holder
                    WHERE holder.book_id = b.id AND holder.user_id = ?
                      AND holder.state = 'imported'
                ) AS holds
         FROM books b
         JOIN vis ON vis.id = b.id
         JOIN users owner ON owner.id = b.user_id
         LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
         LEFT JOIN readers ON readers.book_id = b.id
         LEFT JOIN talk ON talk.book_id = b.id
         LEFT JOIN shares ON shares.book_id = b.id
         ORDER BY b.is_sample ASC,
                  3 * COALESCE(readers.n, 0) + 2 * COALESCE(talk.n, 0) + COALESCE(shares.n, 0) DESC,
                  b.imported_at DESC, b.id ASC
         LIMIT {BROWSE_BOOKS}",
        social_c = social_visible("c"),
        social_r = social_visible("r"),
    );
    let rows = sqlx::query_as::<_, BookRow>(&sql)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(since)
        .bind(&viewer.user_id)
        .bind(since)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .fetch_all(pool)
        .await?;
    Ok(SearchPage {
        hits: SearchHits::Books {
            items: rows
                .into_iter()
                .map(|row| row.into_record(&viewer.user_id))
                .collect(),
        },
        next_cursor: None,
    })
}

/// Paragraphs on visible books. The window score wins. When it is quiet, the
/// newest engaged post still shows, instead of an empty list.
async fn top_posts(pool: &SqlitePool, viewer: &Authed, now: i64) -> Result<SearchPage, ApiError> {
    let since = now.saturating_sub(WINDOW_MS);
    let posts = posts_for("ranked");
    let sql = format!(
        "WITH vis AS (SELECT b.id FROM books b WHERE {BOOK_VISIBLE_TO}),
         ev AS (
             SELECT c.book_id AS book_id, c.post_id AS post_id, c.created_at AS at, 2 AS w
             FROM comments c
             JOIN vis ON vis.id = c.book_id
             JOIN books b ON b.id = c.book_id
             WHERE {social_c}
             UNION ALL
             SELECT r.book_id, r.post_id, r.created_at, 1
             FROM reposts r
             JOIN vis ON vis.id = r.book_id
             JOIN books b ON b.id = r.book_id
             WHERE {social_r}
         ),
         ranked AS (
             SELECT book_id, post_id,
                    SUM(CASE WHEN at >= ? THEN w ELSE 0 END) AS score,
                    MAX(at) AS last_at
             FROM ev
             GROUP BY book_id, post_id
             ORDER BY score DESC, last_at DESC, book_id ASC, post_id ASC
             LIMIT {BROWSE_POSTS}
         )
         SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
                COALESCE(p.progress_index, 0) AS progress_index,
                b.is_sample, b.shared, b.user_id AS owner_id,
                owner.username AS owner_username, b.feed_rev AS feed_rev,
                posts.idx AS post_idx, posts.kind AS post_kind,
                posts.chapter AS post_chapter, posts.text AS post_text,
                CAST(ranked.score AS REAL) AS score
         FROM ranked
         JOIN books b ON b.id = ranked.book_id
         JOIN users owner ON owner.id = b.user_id
         LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
         {posts}
         ORDER BY ranked.score DESC, ranked.last_at DESC, ranked.book_id ASC, ranked.post_id ASC",
        social_c = social_visible("c"),
        social_r = social_visible("r"),
    );
    let rows = sqlx::query_as::<_, PostHitRow>(&sql)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(since)
        .bind(&viewer.user_id)
        .fetch_all(pool)
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(row.into_hit(&viewer.user_id)?);
    }
    Ok(SearchPage {
        hits: SearchHits::Posts { items },
        next_cursor: None,
    })
}

#[derive(Debug, FromRow)]
struct SuggestedAccount {
    username: String,
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

/// Join `posts` for a comment, repost, or ranked row (`c`, `r`, `ranked`).
/// The idx comparison uses UNIQUE (book_id, idx). The text comparison rejects
/// a non-canonical id that CASTs to that idx.
pub(crate) fn posts_for(alias: &str) -> String {
    format!(
        "JOIN posts ON posts.book_id = {alias}.book_id \
         AND posts.idx = CAST({alias}.post_id AS INTEGER) \
         AND {alias}.post_id = CAST(posts.idx AS TEXT)"
    )
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
    cursor: Option<&str>,
) -> Result<SearchPage, ApiError> {
    // username_norm is the ASCII-lowercase username, so fold the query the same way.
    let folded = q.to_ascii_lowercase();
    let infix = format!("%{}%", like_pattern(&folded));
    let prefix = format!("{}%", like_pattern(&folded));
    let after = account_cursor(cursor)?;
    let rows = sqlx::query_as::<_, AccountHitRow>(
        "SELECT id, username, rank, name_len, username_norm
         FROM (
             SELECT id, username, username_norm,
                    CASE WHEN username_norm LIKE ? ESCAPE '\\' THEN 0 ELSE 1 END AS rank,
                    length(username_norm) AS name_len
             FROM users
             WHERE username_norm LIKE ? ESCAPE '\\'
         ) AS ranked
         WHERE rank > ?
            OR (rank = ? AND name_len > ?)
            OR (rank = ? AND name_len = ? AND username_norm > ?)
         ORDER BY rank, name_len, username_norm
         LIMIT ?",
    )
    .bind(&prefix)
    .bind(&infix)
    .bind(after.rank)
    .bind(after.rank)
    .bind(after.name_len)
    .bind(after.rank)
    .bind(after.name_len)
    .bind(&after.username_norm)
    .bind(SEARCH_LIMIT + 1)
    .fetch_all(pool)
    .await?;
    let (rows, next_cursor) = page_rows(rows, |row| {
        encode_json(serde_json::json!([
            row.rank,
            row.name_len,
            row.username_norm
        ]))
    });
    Ok(SearchPage {
        hits: SearchHits::Accounts {
            items: rows
                .into_iter()
                .map(|row| AccountRecord {
                    mine: row.id == viewer.user_id,
                    username: row.username,
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
    cursor: Option<&str>,
) -> Result<SearchPage, ApiError> {
    let infix = format!("%{}%", like_pattern(q));
    let prefix = format!("{}%", like_pattern(q));
    let after = book_cursor(cursor)?;
    let sql = format!(
        "SELECT id, title, author, handle, imported_at, post_count, progress_index,
                is_sample, shared, owner_id, owner_username, feed_rev, rank
         FROM (
             SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
                    COALESCE(p.progress_index, 0) AS progress_index,
                    b.is_sample, b.shared, b.user_id AS owner_id,
                    owner.username AS owner_username, b.feed_rev AS feed_rev,
                    CASE WHEN b.title LIKE ? ESCAPE '\\' THEN 0 ELSE 1 END AS rank
             FROM books b
             JOIN users owner ON owner.id = b.user_id
             LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
             WHERE {BOOK_VISIBLE_TO}
               AND (
                 b.title LIKE ? ESCAPE '\\'
                 OR b.author LIKE ? ESCAPE '\\'
                 OR b.handle LIKE ? ESCAPE '\\'
                 OR (
                   (b.user_id = ? OR b.shared = 1 OR b.is_sample = 1)
                   AND owner.username LIKE ? ESCAPE '\\'
                 )
               )
         ) AS ranked
         WHERE rank > ?
            OR (rank = ? AND imported_at < ?)
            OR (rank = ? AND imported_at = ? AND id > ?)
         ORDER BY rank, imported_at DESC, id ASC
         LIMIT ?"
    );
    let rows = sqlx::query_as::<_, RankedBook>(&sql)
        .bind(&prefix)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(&infix)
        .bind(&infix)
        .bind(&infix)
        .bind(&viewer.user_id)
        .bind(&infix)
        .bind(after.rank)
        .bind(after.rank)
        .bind(after.imported_at)
        .bind(after.rank)
        .bind(after.imported_at)
        .bind(&after.id)
        .bind(SEARCH_LIMIT + 1)
        .fetch_all(pool)
        .await?;
    let (rows, next_cursor) = page_rows(rows, |row| {
        encode_json(serde_json::json!([
            row.rank,
            row.row.imported_at,
            row.row.id
        ]))
    });
    Ok(SearchPage {
        hits: SearchHits::Books {
            items: rows
                .into_iter()
                .map(|row| row.row.into_record(&viewer.user_id))
                .collect(),
        },
        next_cursor,
    })
}

async fn posts(
    pool: &SqlitePool,
    viewer: &Authed,
    q: &str,
    cursor: Option<&str>,
) -> Result<SearchPage, ApiError> {
    let Some(match_query) = fts_match(q) else {
        return Ok(SearchPage {
            hits: SearchHits::Posts { items: vec![] },
            next_cursor: None,
        });
    };
    // The visible-books CTE is the only visibility check in this statement.
    // bm25 is the relevance order. The cursor is that key, not an offset, so a
    // row inserted ahead of this page is not repeated on the next one.
    let after = post_cursor(cursor)?;
    let sql = format!(
        "WITH visible_books AS (
             SELECT b.id AS id
             FROM books b
             WHERE {BOOK_VISIBLE_TO}
         )
         SELECT id, title, author, handle, imported_at, post_count, progress_index,
                is_sample, shared, owner_id, owner_username, feed_rev,
                post_idx, post_kind, post_chapter, post_text, score
         FROM (
             SELECT b.id AS id, b.title AS title, b.author AS author, b.handle AS handle,
                    b.imported_at AS imported_at, b.post_count AS post_count,
                    COALESCE(p.progress_index, 0) AS progress_index,
                    b.is_sample AS is_sample, b.shared AS shared, b.user_id AS owner_id,
                    owner.username AS owner_username, b.feed_rev AS feed_rev,
                    posts.idx AS post_idx, posts.kind AS post_kind,
                    posts.chapter AS post_chapter, posts.text AS post_text,
                    bm25(post_fts) AS score
             FROM post_fts
             JOIN posts ON posts.rowid = post_fts.rowid
             JOIN visible_books ON visible_books.id = posts.book_id
             JOIN books b ON b.id = posts.book_id
             JOIN users owner ON owner.id = b.user_id
             LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
             WHERE post_fts MATCH ?
         ) AS ranked
         WHERE ? = 0
            OR score > ?
            OR (score = ? AND imported_at < ?)
            OR (score = ? AND imported_at = ? AND post_idx > ?)
            OR (score = ? AND imported_at = ? AND post_idx = ? AND id > ?)
         ORDER BY score ASC, imported_at DESC, post_idx ASC, id ASC
         LIMIT ?"
    );
    let rows = sqlx::query_as::<_, PostHitRow>(&sql)
        .bind(&viewer.user_id)
        .bind(&viewer.user_id)
        .bind(&match_query)
        .bind(after.has)
        .bind(after.score)
        .bind(after.score)
        .bind(after.imported_at)
        .bind(after.score)
        .bind(after.imported_at)
        .bind(after.idx)
        .bind(after.score)
        .bind(after.imported_at)
        .bind(after.idx)
        .bind(&after.book_id)
        .bind(SEARCH_LIMIT + 1)
        .fetch_all(pool)
        .await?;
    let (rows, next_cursor) = page_rows(rows, |row| {
        encode_json(serde_json::json!([
            row.score.to_bits().to_string(),
            row.imported_at,
            row.post_idx,
            row.id,
        ]))
    });
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
    feed_rev: i64,
    post_idx: i64,
    post_kind: String,
    post_chapter: String,
    post_text: String,
    score: f64,
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
            files_owner: None,
            holds: 0,
            feed_rev: self.feed_rev,
            has_release: 0,
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

fn page_rows<T>(mut rows: Vec<T>, cursor_of: impl Fn(&T) -> String) -> (Vec<T>, Option<String>) {
    if rows.len() > SEARCH_LIMIT as usize {
        rows.truncate(SEARCH_LIMIT as usize);
        let next = cursor_of(rows.last().expect("truncated page"));
        (rows, Some(next))
    } else {
        (rows, None)
    }
}

#[derive(Debug, FromRow)]
struct AccountHitRow {
    id: String,
    username: String,
    rank: i64,
    name_len: i64,
    username_norm: String,
}

struct AccountCursor {
    rank: i64,
    name_len: i64,
    username_norm: String,
}

fn account_cursor(cursor: Option<&str>) -> Result<AccountCursor, ApiError> {
    let Some(cursor) = nonempty(cursor) else {
        return Ok(AccountCursor {
            rank: -1,
            name_len: 0,
            username_norm: String::new(),
        });
    };
    let items = decode_json_array(cursor)?;
    if items.len() != 3 {
        return Err(bad_cursor());
    }
    Ok(AccountCursor {
        rank: items[0]
            .as_i64()
            .filter(|rank| *rank >= 0)
            .ok_or_else(bad_cursor)?,
        name_len: items[1]
            .as_i64()
            .filter(|len| *len >= 0)
            .ok_or_else(bad_cursor)?,
        username_norm: items[2].as_str().ok_or_else(bad_cursor)?.to_string(),
    })
}

#[derive(Debug, FromRow)]
struct RankedBook {
    #[sqlx(flatten)]
    row: BookRow,
    rank: i64,
}

struct BookCursor {
    rank: i64,
    imported_at: i64,
    id: String,
}

fn book_cursor(cursor: Option<&str>) -> Result<BookCursor, ApiError> {
    let Some(cursor) = nonempty(cursor) else {
        return Ok(BookCursor {
            rank: -1,
            imported_at: i64::MAX,
            id: String::new(),
        });
    };
    let items = decode_json_array(cursor)?;
    if items.len() != 3 {
        return Err(bad_cursor());
    }
    let id = items[2]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(bad_cursor)?;
    Ok(BookCursor {
        rank: items[0]
            .as_i64()
            .filter(|rank| *rank >= 0)
            .ok_or_else(bad_cursor)?,
        imported_at: items[1].as_i64().ok_or_else(bad_cursor)?,
        id: id.to_string(),
    })
}

struct PostCursor {
    has: i64,
    score: f64,
    imported_at: i64,
    idx: i64,
    book_id: String,
}

fn post_cursor(cursor: Option<&str>) -> Result<PostCursor, ApiError> {
    let Some(cursor) = nonempty(cursor) else {
        return Ok(PostCursor {
            has: 0,
            score: 0.0,
            imported_at: 0,
            idx: 0,
            book_id: String::new(),
        });
    };
    let items = decode_json_array(cursor)?;
    if items.len() != 4 {
        return Err(bad_cursor());
    }
    let bits: u64 = items[0]
        .as_str()
        .ok_or_else(bad_cursor)?
        .parse()
        .map_err(|_| bad_cursor())?;
    let book_id = items[3]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(bad_cursor)?;
    Ok(PostCursor {
        has: 1,
        score: f64::from_bits(bits),
        imported_at: items[1].as_i64().ok_or_else(bad_cursor)?,
        idx: items[2].as_i64().ok_or_else(bad_cursor)?,
        book_id: book_id.to_string(),
    })
}

fn nonempty(cursor: Option<&str>) -> Option<&str> {
    cursor.map(str::trim).filter(|cursor| !cursor.is_empty())
}

fn encode_json(value: serde_json::Value) -> String {
    STANDARD.encode(serde_json::to_vec(&value).unwrap_or_default())
}

fn decode_json_array(cursor: &str) -> Result<Vec<serde_json::Value>, ApiError> {
    let text = decode_text(cursor)?;
    match serde_json::from_str(&text).map_err(|_| bad_cursor())? {
        serde_json::Value::Array(items) => Ok(items),
        _ => Err(bad_cursor()),
    }
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

/// Opaque cursor for a descending triple `(at, second, third)`.
/// Profile reposts use `(created_at, book_id, post_id)`. The feed uses
/// `(at, kind, id)`. JSON keeps a post id that contains `:` or a newline intact.
pub(crate) fn encode_keyset3(at: i64, second: &str, third: &str) -> String {
    let payload = serde_json::to_vec(&serde_json::json!([at, second, third])).unwrap_or_default();
    STANDARD.encode(payload)
}

pub(crate) fn decode_keyset3(cursor: &str) -> Result<(i64, String, String), ApiError> {
    let text = decode_text(cursor)?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| bad_cursor())?;
    let items = value.as_array().ok_or_else(bad_cursor)?;
    if items.len() != 3 {
        return Err(bad_cursor());
    }
    let at = items[0].as_i64().ok_or_else(bad_cursor)?;
    let second = items[1]
        .as_str()
        .filter(|part| !part.is_empty())
        .ok_or_else(bad_cursor)?;
    let third = items[2]
        .as_str()
        .filter(|part| !part.is_empty())
        .ok_or_else(bad_cursor)?;
    Ok((at, second.to_string(), third.to_string()))
}

pub(crate) fn decode_feed_cursor(cursor: &str) -> Result<(i64, String, String), ApiError> {
    let (at, kind, id) = decode_keyset3(cursor)?;
    if kind != "comment" && kind != "repost" {
        return Err(bad_cursor());
    }
    Ok((at, kind, id))
}

fn decode_text(cursor: &str) -> Result<String, ApiError> {
    let bytes = STANDARD.decode(cursor).map_err(|_| bad_cursor())?;
    String::from_utf8(bytes).map_err(|_| bad_cursor())
}

fn bad_cursor() -> ApiError {
    ApiError::BadRequest("That page token is not valid.".into())
}

/// How a feed write updates `books`. Import and backfill use [`FeedUpdate::fresh`].
/// A crash leftover `posts.json` uses [`FeedUpdate::leftover`] so it does not
/// claim the current parser. Reparse passes the anchor remap and bumps `feed_rev`.
pub(crate) struct FeedUpdate<'a> {
    pub remap: Option<&'a crate::anchors::Remap>,
    pub bump_feed_rev: bool,
    pub parser_rev: i64,
    /// `(feed_rev, parser_rev)` read with the posts this diff was built from.
    /// The write matches both and refuses when another reparse already won.
    pub seen: Option<(i64, i64)>,
    /// Swap this file onto `book.epub` in the same transaction as the feed.
    pub epub: Option<&'a Path>,
    /// Set when a reader uploaded a replacement.
    pub replaced_at: Option<i64>,
}

impl FeedUpdate<'static> {
    pub(crate) fn fresh() -> Self {
        Self {
            remap: None,
            bump_feed_rev: false,
            parser_rev: crate::feed::PARSER_REV,
            seen: None,
            epub: None,
            replaced_at: None,
        }
    }

    /// The file was already on disk. This process did not parse it.
    pub(crate) fn leftover() -> Self {
        Self {
            remap: None,
            bump_feed_rev: false,
            parser_rev: 0,
            seen: None,
            epub: None,
            replaced_at: None,
        }
    }
}

#[derive(sqlx::FromRow)]
struct SeenPost {
    idx: i64,
    kind: String,
    text: String,
    chapter: String,
}

/// Posts and the book revisions, from one snapshot. The write later matches
/// those revisions, so a second rewrite built on the same rows refuses.
pub(crate) async fn load_seen(
    pool: &SqlitePool,
    book_id: &str,
) -> Result<(Vec<Post>, i64, i64), ApiError> {
    let mut tx = pool.begin().await?;
    let (feed_rev, parser_rev): (i64, i64) =
        sqlx::query_as("SELECT feed_rev, parser_rev FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_one(&mut *tx)
            .await?;
    let rows = sqlx::query_as::<_, SeenPost>(
        "SELECT idx, kind, text, chapter FROM posts WHERE book_id = ? ORDER BY idx ASC",
    )
    .bind(book_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut posts = Vec::with_capacity(rows.len());
    for (expected, row) in rows.into_iter().enumerate() {
        if row.idx != expected as i64 {
            tracing::error!(book_id, idx = row.idx, expected, "post indexes have a gap");
            return Err(ApiError::Internal);
        }
        posts.push(Post {
            id: row.idx.to_string(),
            kind: parse_kind(&row.kind)?,
            text: row.text,
            chapter: row.chapter,
        });
    }
    Ok((posts, feed_rev, parser_rev))
}

pub(crate) fn stale_reparse() -> ApiError {
    ApiError::Conflict("This book changed while it was being reparsed. Run it again.".into())
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
    store_feed_with(pool, dir, book_id, posts, FeedUpdate::fresh()).await
}

pub(crate) async fn store_feed_with(
    pool: &SqlitePool,
    dir: &Path,
    book_id: &str,
    posts: &Posts,
    update: FeedUpdate<'_>,
) -> Result<Vec<u8>, ApiError> {
    let bytes = serde_json::to_vec(posts).map_err(|err| {
        tracing::error!(error = %err, "could not encode posts.json");
        ApiError::Internal
    })?;
    // A startup backfill and a comment can want the write lock together.
    // One long wait loses: the writer times out, or backfill gives up and
    // leaves the book unindexed. Wait briefly, let the other statement
    // finish, and try the same book again.
    let mut attempt = 0;
    loop {
        match write_indexed(pool, dir, book_id, posts, &bytes, &update).await {
            Ok(()) => return Ok(bytes),
            Err(WriteFailure::Locked) if attempt + 1 < FEED_WRITE_ATTEMPTS => {
                attempt += 1;
                tokio::time::sleep(BACKFILL_PAUSE).await;
            }
            Err(WriteFailure::Locked) => {
                tracing::error!(book_id, "posts index stayed busy");
                return Err(ApiError::Internal);
            }
            Err(WriteFailure::Other(err)) => return Err(err),
        }
    }
}

const POST_INSERT_BATCH: usize = 500;

type BookMutex = std::sync::Arc<tokio::sync::Mutex<()>>;

/// One mutex per book. The map entry is removed when the last guard drops,
/// so a book that has already been filled does not stay in the table.
#[derive(Clone)]
pub(crate) struct FillLocks {
    books: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, BookMutex>>>,
}

pub(crate) struct FillGuard {
    id: String,
    books: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, BookMutex>>>,
    mutex: BookMutex,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl FillLocks {
    pub(crate) fn new() -> Self {
        Self {
            books: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub(crate) async fn lock(&self, book_id: &str) -> FillGuard {
        let mutex = {
            let mut books = self.books.lock().unwrap_or_else(|err| err.into_inner());
            books
                .entry(book_id.to_string())
                .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let guard = mutex.clone().lock_owned().await;
        FillGuard {
            id: book_id.to_string(),
            books: std::sync::Arc::clone(&self.books),
            mutex,
            guard: Some(guard),
        }
    }
}

impl Drop for FillGuard {
    fn drop(&mut self) {
        self.guard.take();
        let mut books = self.books.lock().unwrap_or_else(|err| err.into_inner());
        let idle = books.get(&self.id).is_some_and(|existing| {
            std::sync::Arc::ptr_eq(existing, &self.mutex)
                && std::sync::Arc::strong_count(&self.mutex) == 2
        });
        if idle {
            books.remove(&self.id);
        }
    }
}

pub(crate) const BACKFILL_PAUSE: Duration = Duration::from_millis(75);
const FEED_WRITE_ATTEMPTS: usize = 40;
const FEED_BUSY_MS: u64 = 250;

enum WriteFailure {
    Locked,
    Other(ApiError),
}

impl WriteFailure {
    fn sqlx(err: sqlx::Error) -> Self {
        if err.to_string().contains("database is locked") {
            WriteFailure::Locked
        } else {
            WriteFailure::Other(err.into())
        }
    }
}

async fn write_indexed(
    pool: &SqlitePool,
    dir: &Path,
    book_id: &str,
    posts: &Posts,
    bytes: &[u8],
    update: &FeedUpdate<'_>,
) -> Result<(), WriteFailure> {
    let mut conn = pool.acquire().await.map_err(WriteFailure::sqlx)?;
    // The pooled connection keeps this pragma. Put the normal timeout back
    // before the connection is reused, including when the write fails.
    if let Err(err) = sqlx::query(&format!("PRAGMA busy_timeout = {FEED_BUSY_MS}"))
        .execute(&mut *conn)
        .await
    {
        // The pragma did not change. Nothing to put back.
        return Err(WriteFailure::sqlx(err));
    }
    let outcome = write_tx(&mut conn, dir, book_id, posts, bytes, update).await;
    let restore = format!(
        "PRAGMA busy_timeout = {}",
        crate::db::BUSY_TIMEOUT.as_millis()
    );
    if let Err(err) = sqlx::query(&restore).execute(&mut *conn).await {
        tracing::error!(error = %err, "could not restore busy_timeout");
    }
    outcome
}

async fn write_tx(
    conn: &mut sqlx::SqliteConnection,
    dir: &Path,
    book_id: &str,
    posts: &Posts,
    bytes: &[u8],
    update: &FeedUpdate<'_>,
) -> Result<(), WriteFailure> {
    // Deferred is enough for a fresh import. A remap has to lock out a like
    // that read the old index and would insert it after the ids move. A seen
    // revision is the same lock: the claim below has to observe a stable row.
    let mut tx = if update.remap.is_some() || update.seen.is_some() {
        conn.begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(WriteFailure::sqlx)?
    } else {
        conn.begin().await.map_err(WriteFailure::sqlx)?
    };
    sqlx::query("DELETE FROM posts WHERE book_id = ?")
        .bind(book_id)
        .execute(&mut *tx)
        .await
        .map_err(WriteFailure::sqlx)?;
    insert_posts(&mut tx, book_id, &posts.posts).await?;
    if let Some(remap) = update.remap {
        crate::anchors::apply(&mut tx, book_id, remap)
            .await
            .map_err(WriteFailure::Other)?;
    }
    let (feed, parser) = update.seen.unwrap_or((0, 0));
    let book_update = match (update.bump_feed_rev, update.seen.is_some()) {
        (true, true) => {
            "UPDATE books SET posts_indexed = 1, post_count = ?, parser_rev = ?, feed_rev = feed_rev + 1
             WHERE id = ? AND feed_rev = ? AND parser_rev = ?"
        }
        (false, true) => {
            "UPDATE books SET posts_indexed = 1, post_count = ?, parser_rev = ?
             WHERE id = ? AND feed_rev = ? AND parser_rev = ?"
        }
        (true, false) => {
            "UPDATE books SET posts_indexed = 1, post_count = ?, parser_rev = ?, feed_rev = feed_rev + 1 WHERE id = ?"
        }
        (false, false) => {
            "UPDATE books SET posts_indexed = 1, post_count = ?, parser_rev = ? WHERE id = ?"
        }
    };
    let mut book_update = sqlx::query(book_update)
        .bind(posts.posts.len() as i64)
        .bind(update.parser_rev)
        .bind(book_id);
    if update.seen.is_some() {
        book_update = book_update.bind(feed).bind(parser);
    }
    let updated = book_update
        .execute(&mut *tx)
        .await
        .map_err(WriteFailure::sqlx)?;
    if let Some(at) = update.replaced_at {
        sqlx::query("UPDATE books SET replaced_at = ? WHERE id = ?")
            .bind(at)
            .bind(book_id)
            .execute(&mut *tx)
            .await
            .map_err(WriteFailure::sqlx)?;
    }
    if update.seen.is_some() && updated.rows_affected() != 1 {
        // A bump always changes feed_rev, so 0 rows means the WHERE missed.
        // Without a bump, SQLite also reports 0 when the SET matches the row.
        let still_ours = if update.bump_feed_rev {
            false
        } else {
            revs_still(&mut tx, book_id, feed, parser)
                .await
                .map_err(WriteFailure::sqlx)?
        };
        if !still_ours {
            return Err(WriteFailure::Other(stale_reparse()));
        }
    }
    write_posts_file(dir, bytes)
        .await
        .map_err(WriteFailure::Other)?;
    if let Some(src) = update.epub {
        tokio::fs::copy(src, dir.join("book.epub"))
            .await
            .map_err(|err| WriteFailure::Other(err.into()))?;
    }
    tx.commit().await.map_err(WriteFailure::sqlx)?;
    Ok(())
}

async fn insert_posts(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
    posts: &[Post],
) -> Result<(), WriteFailure> {
    for chunk in posts.chunks(POST_INSERT_BATCH) {
        let mut sql = String::from("INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES ");
        for (offset, _) in chunk.iter().enumerate() {
            if offset > 0 {
                sql.push(',');
            }
            sql.push_str("(?,?,?,?,?)");
        }
        let mut query = sqlx::query(&sql);
        for post in chunk {
            let idx: i64 = post.id.parse().map_err(|_| {
                tracing::error!(id = %post.id, book_id, "post id is not an index");
                WriteFailure::Other(ApiError::Internal)
            })?;
            query = query
                .bind(book_id)
                .bind(idx)
                .bind(kind_label(&post.kind))
                .bind(&post.chapter)
                .bind(&post.text);
        }
        query
            .execute(tx.deref_mut())
            .await
            .map_err(WriteFailure::sqlx)?;
    }
    Ok(())
}

fn kind_label(kind: &PostKind) -> &'static str {
    match kind {
        PostKind::Heading => "Heading",
        PostKind::Body => "Body",
    }
}

async fn revs_still(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
    feed_rev: i64,
    parser_rev: i64,
) -> Result<bool, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM books WHERE id = ? AND feed_rev = ? AND parser_rev = ?",
    )
    .bind(book_id)
    .bind(feed_rev)
    .bind(parser_rev)
    .fetch_one(&mut **tx)
    .await?;
    Ok(count == 1)
}

/// Stamps `parser_rev` and, when `bytes` differ from `posts.json`, rewrites
/// the file. The book must still be at the revisions read with its posts.
/// A feed whose posts did not change takes this path instead of a remap.
pub(crate) async fn note_unchanged(
    pool: &SqlitePool,
    dir: &Path,
    book_id: &str,
    bytes: &[u8],
    seen_feed: i64,
    seen_parser: i64,
) -> Result<(), ApiError> {
    let current = match tokio::fs::read(dir.join("posts.json")).await {
        Ok(current) => Some(current),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    let file_differs = current.as_deref() != Some(bytes);
    if !file_differs && seen_parser == crate::feed::PARSER_REV {
        return Ok(());
    }
    let mut conn = pool.acquire().await?;
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    let updated = sqlx::query(
        "UPDATE books SET parser_rev = ?
         WHERE id = ? AND feed_rev = ? AND parser_rev = ?",
    )
    .bind(crate::feed::PARSER_REV)
    .bind(book_id)
    .bind(seen_feed)
    .bind(seen_parser)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 && !revs_still(&mut tx, book_id, seen_feed, seen_parser).await?
    {
        return Err(stale_reparse());
    }
    if file_differs {
        write_posts_file(dir, bytes).await?;
    }
    tx.commit().await?;
    Ok(())
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
/// An unreadable EPUB with no `posts.json` is marked `-1` and left alone.
/// [`ApiError::Busy`] is not unreadable: the row stays at 0, and this pass
/// retries that same book after a pause. The pause does not hold the book's
/// fill lock.
pub(crate) async fn backfill(state: AppState) {
    let pending = sqlx::query_as::<_, (String, String)>(
        "SELECT id, COALESCE(files_user_id, user_id)
         FROM books WHERE posts_indexed = 0 ORDER BY imported_at ASC, id ASC",
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
    for (offset, (book_id, files_owner)) in pending.iter().enumerate() {
        loop {
            match index_pending(&state, book_id, files_owner).await {
                Ok(true) => {
                    indexed += 1;
                    break;
                }
                Ok(false) => break,
                Err(ApiError::Busy) => tokio::time::sleep(BACKFILL_PAUSE).await,
                Err(err) => {
                    tracing::error!(error = %err, %book_id, "search backfill failed for a book");
                    break;
                }
            }
        }
        // The write lock is free between books. A pause lets comments and
        // progress updates in before the next book starts.
        if offset + 1 != pending.len() {
            tokio::time::sleep(BACKFILL_PAUSE).await;
        }
    }
    tracing::info!(indexed, pending = pending.len(), "search backfill finished");
}

/// `Ok(true)` when this call stored the feed. `Ok(false)` when another caller
/// indexed the book after the pending list was read.
async fn index_pending(
    state: &AppState,
    book_id: &str,
    files_owner: &str,
) -> Result<bool, ApiError> {
    if !unindexed(state, book_id).await? {
        return Ok(false);
    }
    let _guard = state.fill_locks.lock(book_id).await;
    // The other caller of this book may have stored the feed while we waited.
    if !unindexed(state, book_id).await? {
        return Ok(false);
    }
    let dir = crate::library::book_dir(&state.data_dir, files_owner, book_id);
    // A crash can leave posts.json on disk before posts_indexed commits. That
    // file needs no parse, so a full queue must not turn the retry into Busy.
    // It was not produced by this parser, so parser_rev stays 0.
    let (posts, update) = match read_posts(&dir).await? {
        Some(bytes) => (
            serde_json::from_slice::<Posts>(&bytes).map_err(|err| {
                tracing::error!(error = %err, book_id, "posts.json is not a feed");
                ApiError::Internal
            })?,
            FeedUpdate::leftover(),
        ),
        None => {
            // Non-waiting, and only on the EPUB path, so a full queue does not
            // keep a permit that this book is not going to use. The caller
            // sleeps after this returns, which drops the guard first.
            let permit = state.parsers.try_acquire()?;
            match posts_from_epub(state, dir.join("book.epub"), permit).await {
                Ok(posts) => (posts, FeedUpdate::fresh()),
                Err(err @ ApiError::BadRequest(_)) => {
                    tracing::error!(%book_id, error = %err, "search backfill skipped an unreadable EPUB");
                    sqlx::query(
                        "UPDATE books SET posts_indexed = -1 WHERE id = ? AND posts_indexed = 0",
                    )
                    .bind(book_id)
                    .execute(&state.pool)
                    .await?;
                    return Ok(false);
                }
                Err(err) => return Err(err),
            }
        }
    };
    store_feed_with(&state.pool, &dir, book_id, &posts, update).await?;
    Ok(true)
}

async fn unindexed(state: &AppState, book_id: &str) -> Result<bool, ApiError> {
    let indexed: Option<i64> = sqlx::query_scalar("SELECT posts_indexed FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(&state.pool)
        .await?;
    Ok(indexed == Some(0))
}

async fn posts_from_epub(
    state: &AppState,
    path: PathBuf,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Posts, ApiError> {
    let parsed = state.parsers.parse_owned(path, permit).await?;
    Ok(Posts {
        posts: parsed.posts,
    })
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

    #[test]
    fn keyset3_roundtrips_and_feed_kind_is_checked() {
        let cursor = encode_keyset3(12, "book:1", "post\nid");
        assert_eq!(
            decode_keyset3(&cursor).unwrap(),
            (12, "book:1".into(), "post\nid".into())
        );
        assert!(decode_keyset3("%%%").is_err());
        assert!(decode_feed_cursor(&encode_keyset3(1, "comment", "abc")).is_ok());
        assert!(decode_feed_cursor(&encode_keyset3(1, "nope", "abc")).is_err());
    }

    #[tokio::test]
    async fn backfill_busy_leaves_posts_indexed_at_zero() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crate::prepare(crate::config::Config {
            listen: "127.0.0.1:0".into(),
            data_dir: tmp.path().to_path_buf(),
            web_dir: None,
            allow_signup: false,
            max_epub_bytes: crate::config::MIN_EPUB_BYTES,
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
            auth: crate::config::AuthConfig::default(),
            limits: crate::config::LimitsConfig {
                max_parses: 0,
                parse_wait: Duration::from_millis(10),
                min_free_bytes: 0,
            },
        })
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES ('owner', 'owner', 'owner', 'x', 1)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO books
             (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
             VALUES ('book', 'owner', 'Legacy', 'Author', 'author', 1, 0, 0, 0, 0, 0)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        let dir = crate::library::book_dir(&state.data_dir, "owner", "book");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("book.epub"), b"not an epub")
            .await
            .unwrap();

        let err = index_pending(&state, "book", "owner").await.unwrap_err();
        assert!(matches!(err, ApiError::Busy), "{err}");
        let indexed: i64 = sqlx::query_scalar("SELECT posts_indexed FROM books WHERE id = 'book'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(indexed, 0);
        assert!(state.parsers.stats().busy >= 1);
    }

    #[tokio::test]
    async fn a_saved_feed_indexes_without_a_parse_permit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crate::prepare(crate::config::Config {
            listen: "127.0.0.1:0".into(),
            data_dir: tmp.path().to_path_buf(),
            web_dir: None,
            allow_signup: false,
            max_epub_bytes: crate::config::MIN_EPUB_BYTES,
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
            auth: crate::config::AuthConfig::default(),
            limits: crate::config::LimitsConfig {
                max_parses: 0,
                parse_wait: Duration::from_millis(10),
                min_free_bytes: 0,
            },
        })
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES ('owner', 'owner', 'owner', 'x', 1)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO books
             (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
             VALUES ('book', 'owner', 'Legacy', 'Author', 'author', 1, 0, 0, 0, 0, 0)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        let dir = crate::library::book_dir(&state.data_dir, "owner", "book");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(
            dir.join("posts.json"),
            r#"{"posts":[{"id":"0","kind":"Body","text":"Hi","chapter":"I"}]}"#,
        )
        .await
        .unwrap();

        let stored = index_pending(&state, "book", "owner").await.unwrap();
        assert!(stored);
        let indexed: i64 = sqlx::query_scalar("SELECT posts_indexed FROM books WHERE id = 'book'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(indexed, 1);
        assert_eq!(state.parsers.stats().busy, 0);
        assert_eq!(state.parsers.stats().parses, 0);
        let rev: i64 = sqlx::query_scalar("SELECT parser_rev FROM books WHERE id = 'book'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(
            rev, 0,
            "a leftover posts.json was not parsed by this revision"
        );
    }

    #[tokio::test]
    async fn a_second_stale_remap_refuses_and_leaves_the_anchor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = crate::prepare(crate::config::Config {
            listen: "127.0.0.1:0".into(),
            data_dir: tmp.path().to_path_buf(),
            web_dir: None,
            allow_signup: false,
            max_epub_bytes: crate::config::MIN_EPUB_BYTES,
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
            auth: crate::config::AuthConfig::default(),
            limits: crate::config::LimitsConfig {
                max_parses: 1,
                parse_wait: Duration::from_millis(10),
                min_free_bytes: 0,
            },
        })
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES ('owner', 'owner', 'owner', 'x', 1)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO books
             (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
             VALUES ('book', 'owner', 'Pain', 'Lewis', 'lewis', 1, 3, 0, 0, 0, 1)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        for (idx, kind, text) in [
            (0, "Heading", "Chapter"),
            (1, "Heading", "Chapter"),
            (2, "Body", "y"),
        ] {
            sqlx::query(
                "INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES ('book', ?, ?, 'C', ?)",
            )
            .bind(idx)
            .bind(kind)
            .bind(text)
            .execute(&state.pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO comments (id, user_id, book_id, post_id, text, created_at)
             VALUES ('11111111-1111-4111-8111-111111111111', 'owner', 'book', '2', 'on y', 1)",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        let dir = crate::library::book_dir(&state.data_dir, "owner", "book");
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let old = [
            post("0", PostKind::Heading, "Chapter"),
            post("1", PostKind::Heading, "Chapter"),
            post("2", PostKind::Body, "y"),
        ];
        let new = [
            post("0", PostKind::Heading, "Chapter"),
            post("1", PostKind::Body, "y"),
        ];
        let old_keys: Vec<crate::anchors::PostKey> =
            old.iter().map(crate::anchors::PostKey::from).collect();
        let new_keys: Vec<crate::anchors::PostKey> =
            new.iter().map(crate::anchors::PostKey::from).collect();
        let remap = crate::anchors::align(&old_keys, &new_keys);
        assert_eq!(remap.new_of, vec![0, 0, 1]);
        let update = || FeedUpdate {
            remap: Some(&remap),
            bump_feed_rev: true,
            parser_rev: crate::feed::PARSER_REV,
            seen: Some((0, 0)),
            epub: None,
            replaced_at: None,
        };
        store_feed_with(
            &state.pool,
            &dir,
            "book",
            &Posts {
                posts: new.to_vec(),
            },
            update(),
        )
        .await
        .unwrap();
        let moved: String = sqlx::query_scalar(
            "SELECT post_id FROM comments WHERE book_id = 'book' -- own rows only",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(moved, "1");

        let err = store_feed_with(
            &state.pool,
            &dir,
            "book",
            &Posts {
                posts: new.to_vec(),
            },
            update(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ApiError::Conflict(_)), "{err}");
        assert_eq!(
            err.to_string(),
            "This book changed while it was being reparsed. Run it again."
        );
        let stayed: String = sqlx::query_scalar(
            "SELECT post_id FROM comments WHERE book_id = 'book' -- own rows only",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(
            stayed, "1",
            "the second remap must not shift the anchor again"
        );
        let feed_rev: i64 = sqlx::query_scalar("SELECT feed_rev FROM books WHERE id = 'book'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(feed_rev, 1);
    }

    fn post(id: &str, kind: PostKind, text: &str) -> Post {
        Post {
            id: id.to_owned(),
            kind,
            text: text.to_owned(),
            chapter: "C".to_owned(),
        }
    }
}
