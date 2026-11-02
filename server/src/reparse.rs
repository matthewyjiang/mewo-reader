//! Rebuild a stored book's posts from its EPUB and remap every anchor.
//!
//! The book id stays. Old books keep the feed the parser wrote at import, so
//! a later parser fix does not reach them until this runs.

use crate::anchors::{self, PostKey, Remap};
use crate::error::ApiError;
use crate::feed::{Post, Posts, PARSER_REV};
use crate::library::book_dir;
use crate::parse_pool::ParsePool;
use crate::search::{self, FeedUpdate, FillLocks};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::Path;

pub(crate) struct Reparser<'a> {
    pub pool: &'a SqlitePool,
    pub data_dir: &'a Path,
    pub parsers: &'a ParsePool,
    /// `Some` in-process. The CLI cannot see the server's locks.
    pub locks: Option<&'a FillLocks>,
}

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    DryRun,
    Apply,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnchorCounts {
    pub comments: u32,
    pub likes: u32,
    pub reposts: u32,
    pub progress: u32,
}

impl AnchorCounts {
    fn zero() -> Self {
        Self {
            comments: 0,
            likes: 0,
            reposts: 0,
            progress: 0,
        }
    }

    pub(crate) fn total(&self) -> u32 {
        self.comments + self.likes + self.reposts + self.progress
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Outcome {
    Unchanged,
    WouldChange,
    Changed,
    Skipped(String),
    Failed(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub book_id: String,
    pub title: String,
    pub owner: String,
    pub old_posts: u32,
    pub new_posts: u32,
    pub headings_removed: u32,
    pub headings_added: u32,
    pub matched: u32,
    pub snapped: u32,
    pub body_changed: u32,
    pub anchors_moved: AnchorCounts,
    pub outcome: Outcome,
}

struct BookRef {
    id: String,
    title: String,
    owner: String,
    files_owner: String,
    posts_indexed: i64,
    parser_rev: i64,
}

impl Reparser<'_> {
    /// Books with `parser_rev < PARSER_REV`, oldest first. `--force` includes
    /// a current revision. One book at a time, with [`search::BACKFILL_PAUSE`]
    /// between them so a live server can take the write lock.
    pub(crate) async fn all(&self, mode: Mode, force: bool) -> Result<Vec<Report>, ApiError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM books
             WHERE posts_indexed = 1 AND (? = 1 OR parser_rev < ?)
             ORDER BY imported_at ASC, id ASC",
        )
        .bind(if force { 1 } else { 0 })
        .bind(PARSER_REV)
        .fetch_all(self.pool)
        .await?;
        let mut reports = Vec::with_capacity(ids.len());
        for (offset, id) in ids.iter().enumerate() {
            reports.push(self.one(id, mode, force).await?);
            if offset + 1 != ids.len() {
                tokio::time::sleep(search::BACKFILL_PAUSE).await;
            }
        }
        Ok(reports)
    }

    pub(crate) async fn one(
        &self,
        book_id: &str,
        mode: Mode,
        force: bool,
    ) -> Result<Report, ApiError> {
        let Some(book) = self.book(book_id).await? else {
            return Err(ApiError::NotFound);
        };
        let _guard = match self.locks {
            Some(locks) => Some(locks.lock(&book.id).await),
            None => None,
        };
        if book.posts_indexed != 1 {
            return Ok(self.skipped(&book, 0, "not indexed"));
        }
        let old_posts = load_posts(self.pool, &book.id).await?;
        let path = book_dir(self.data_dir, &book.files_owner, &book.id).join("book.epub");
        let permit = self.parsers.acquire().await?;
        let parsed = match self.parsers.parse_owned(path, permit).await {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::warn!(book_id = %book.id, error = %err, "reparse could not read the EPUB");
                return Ok(self.failed(&book, old_posts.len() as u32, err.to_string()));
            }
        };
        let old_keys: Vec<PostKey> = old_posts.iter().map(PostKey::from).collect();
        let new_keys: Vec<PostKey> = parsed.posts.iter().map(PostKey::from).collect();
        let remap = anchors::align(&old_keys, &new_keys);
        let (headings_removed, headings_added) = remap.heading_delta(&old_keys, &new_keys);
        let changed = body_changed(&old_keys, &new_keys, &remap);
        if old_keys == new_keys {
            if matches!(mode, Mode::Apply) && book.parser_rev != PARSER_REV {
                sqlx::query("UPDATE books SET parser_rev = ? WHERE id = ?")
                    .bind(PARSER_REV)
                    .bind(&book.id)
                    .execute(self.pool)
                    .await?;
            }
            return Ok(Report {
                book_id: book.id,
                title: book.title,
                owner: book.owner,
                old_posts: old_posts.len() as u32,
                new_posts: parsed.posts.len() as u32,
                headings_removed: 0,
                headings_added: 0,
                matched: remap.matched,
                snapped: remap.snapped,
                body_changed: 0,
                anchors_moved: AnchorCounts::zero(),
                outcome: Outcome::Unchanged,
            });
        }
        let body_posts = old_keys
            .iter()
            .filter(|post| post.kind == crate::feed::PostKind::Body)
            .count() as u32;
        if !force && body_change_refused(body_posts, changed) {
            return Ok(Report {
                book_id: book.id,
                title: book.title,
                owner: book.owner,
                old_posts: old_posts.len() as u32,
                new_posts: parsed.posts.len() as u32,
                headings_removed,
                headings_added,
                matched: remap.matched,
                snapped: remap.snapped,
                body_changed: changed,
                anchors_moved: AnchorCounts::zero(),
                outcome: Outcome::Skipped("text changed, not just headings".to_owned()),
            });
        }
        let anchors_moved = count_anchors(self.pool, &book.id, &remap).await?;
        let outcome = if matches!(mode, Mode::DryRun) {
            Outcome::WouldChange
        } else {
            let dir = book_dir(self.data_dir, &book.files_owner, &book.id);
            search::store_feed_with(
                self.pool,
                &dir,
                &book.id,
                &Posts {
                    posts: parsed.posts.clone(),
                },
                FeedUpdate {
                    remap: Some(&remap),
                    bump_feed_rev: true,
                    parser_rev: PARSER_REV,
                },
            )
            .await?;
            Outcome::Changed
        };
        Ok(Report {
            book_id: book.id,
            title: book.title,
            owner: book.owner,
            old_posts: old_posts.len() as u32,
            new_posts: parsed.posts.len() as u32,
            headings_removed,
            headings_added,
            matched: remap.matched,
            snapped: remap.snapped,
            body_changed: changed,
            anchors_moved,
            outcome,
        })
    }

    async fn book(&self, book_id: &str) -> Result<Option<BookRef>, ApiError> {
        let row = sqlx::query_as::<_, BookRow>(
            "SELECT b.id, b.title, owner.username AS owner,
                    COALESCE(b.files_user_id, b.user_id) AS files_owner,
                    b.posts_indexed, b.parser_rev
             FROM books b
             JOIN users owner ON owner.id = b.user_id
             WHERE b.id = ?",
        )
        .bind(book_id)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.map(|row| BookRef {
            id: row.id,
            title: row.title,
            owner: row.owner,
            files_owner: row.files_owner,
            posts_indexed: row.posts_indexed,
            parser_rev: row.parser_rev,
        }))
    }

    fn skipped(&self, book: &BookRef, old_posts: u32, reason: &str) -> Report {
        Report {
            book_id: book.id.clone(),
            title: book.title.clone(),
            owner: book.owner.clone(),
            old_posts,
            new_posts: old_posts,
            headings_removed: 0,
            headings_added: 0,
            matched: 0,
            snapped: 0,
            body_changed: 0,
            anchors_moved: AnchorCounts::zero(),
            outcome: Outcome::Skipped(reason.to_owned()),
        }
    }

    fn failed(&self, book: &BookRef, old_posts: u32, message: String) -> Report {
        Report {
            book_id: book.id.clone(),
            title: book.title.clone(),
            owner: book.owner.clone(),
            old_posts,
            new_posts: 0,
            headings_removed: 0,
            headings_added: 0,
            matched: 0,
            snapped: 0,
            body_changed: 0,
            anchors_moved: AnchorCounts::zero(),
            outcome: Outcome::Failed(message),
        }
    }
}

#[derive(sqlx::FromRow)]
struct BookRow {
    id: String,
    title: String,
    owner: String,
    files_owner: String,
    posts_indexed: i64,
    parser_rev: i64,
}

#[derive(sqlx::FromRow)]
struct StoredPost {
    idx: i64,
    kind: String,
    text: String,
    chapter: String,
}

async fn load_posts(pool: &SqlitePool, book_id: &str) -> Result<Vec<Post>, ApiError> {
    let rows = sqlx::query_as::<_, StoredPost>(
        "SELECT idx, kind, text, chapter FROM posts WHERE book_id = ? ORDER BY idx ASC",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await?;
    let mut posts = Vec::with_capacity(rows.len());
    for (expected, row) in rows.into_iter().enumerate() {
        if row.idx != expected as i64 {
            tracing::error!(book_id, idx = row.idx, expected, "post indexes have a gap");
            return Err(ApiError::Internal);
        }
        posts.push(Post {
            id: row.idx.to_string(),
            kind: search::parse_kind(&row.kind)?,
            text: row.text,
            chapter: row.chapter,
        });
    }
    Ok(posts)
}

fn body_changed(old: &[PostKey], new: &[PostKey], remap: &Remap) -> u32 {
    old.iter()
        .enumerate()
        .filter(|(index, post)| {
            post.kind == crate::feed::PostKind::Body
                && new
                    .get(remap.new_of[*index] as usize)
                    .map(|next| next != *post)
                    .unwrap_or(true)
        })
        .count() as u32
}

/// More than `max(5, 2% of body posts)` body posts landed on different text.
fn body_change_refused(body_posts: u32, changed: u32) -> bool {
    let limit = 5.max(body_posts.saturating_mul(2) / 100);
    changed > limit
}

async fn count_anchors(
    pool: &SqlitePool,
    book_id: &str,
    remap: &Remap,
) -> Result<AnchorCounts, ApiError> {
    Ok(AnchorCounts {
        comments: count_post_ids(pool, "comments", book_id, remap).await?,
        likes: count_post_ids(pool, "likes", book_id, remap).await?,
        reposts: count_post_ids(pool, "reposts", book_id, remap).await?,
        progress: count_progress(pool, book_id, remap).await?,
    })
}

async fn count_post_ids(
    pool: &SqlitePool,
    table: &str,
    book_id: &str,
    remap: &Remap,
) -> Result<u32, ApiError> {
    let ids: Vec<String> =
        sqlx::query_scalar(&format!("SELECT post_id FROM {table} WHERE book_id = ?"))
            .bind(book_id)
            .fetch_all(pool)
            .await?;
    Ok(ids.iter().filter(|id| post_id_moves(remap, id)).count() as u32)
}

fn post_id_moves(remap: &Remap, id: &str) -> bool {
    let Ok(index) = id.parse::<usize>() else {
        return false;
    };
    let canonical = index.to_string();
    if canonical != id {
        return false;
    }
    remap
        .new_of
        .get(index)
        .is_some_and(|new_index| *new_index as usize != index)
}

async fn count_progress(pool: &SqlitePool, book_id: &str, remap: &Remap) -> Result<u32, ApiError> {
    let indexes: Vec<i64> =
        sqlx::query_scalar("SELECT progress_index FROM book_progress WHERE book_id = ?")
            .bind(book_id)
            .fetch_all(pool)
            .await?;
    Ok(indexes
        .into_iter()
        .filter(|index| progress_moves(remap, *index))
        .count() as u32)
}

fn progress_moves(remap: &Remap, index: i64) -> bool {
    if index < 0 {
        return false;
    }
    let Some(&mapped) = remap.new_of.get(index as usize) else {
        return false;
    };
    let clamped = if remap.new_len == 0 {
        0
    } else {
        mapped.min(remap.new_len - 1)
    };
    clamped as i64 != index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_guard_allows_five_and_refuses_the_sixth() {
        assert!(!body_change_refused(7, 5));
        assert!(body_change_refused(7, 6));
        assert!(!body_change_refused(1000, 20));
        assert!(body_change_refused(1000, 21));
        assert!(!body_change_refused(3, 5));
        assert!(body_change_refused(3, 6));
    }
}
