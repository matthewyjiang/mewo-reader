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
}

impl Reparser<'_> {
    /// Books with `parser_rev < PARSER_REV`, oldest first. `--force` includes
    /// a current revision. One book at a time, with [`search::BACKFILL_PAUSE`]
    /// between them so a live server can take the write lock. Each report is
    /// handed to `emit` as the book finishes. A per-book error is a `Failed`
    /// report; the rest of the run continues.
    pub(crate) async fn all(
        &self,
        mode: Mode,
        force: bool,
        mut emit: impl FnMut(&Report) -> std::io::Result<()>,
    ) -> Result<(), ApiError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM books
             WHERE posts_indexed = 1 AND (? = 1 OR parser_rev < ?)
             ORDER BY imported_at ASC, id ASC",
        )
        .bind(if force { 1 } else { 0 })
        .bind(PARSER_REV)
        .fetch_all(self.pool)
        .await?;
        for (offset, id) in ids.iter().enumerate() {
            let report = match self.one(id, mode, force).await {
                Ok(report) => report,
                Err(error) => {
                    tracing::warn!(book_id = %id, error = %error, "reparse failed for a book");
                    Report::failed(id, error.to_string())
                }
            };
            emit(&report).map_err(|err| {
                tracing::error!(error = %err, "could not print a reparse report");
                ApiError::Internal
            })?;
            if offset + 1 != ids.len() {
                tokio::time::sleep(search::BACKFILL_PAUSE).await;
            }
        }
        Ok(())
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
        let (old_posts, seen_feed, seen_parser) = search::load_seen(self.pool, &book.id).await?;
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
        if same_posts(&old_posts, &parsed.posts) {
            if matches!(mode, Mode::Apply) {
                let bytes = serde_json::to_vec(&Posts {
                    posts: parsed.posts.clone(),
                })
                .map_err(|err| {
                    tracing::error!(error = %err, "could not encode posts.json");
                    ApiError::Internal
                })?;
                let dir = book_dir(self.data_dir, &book.files_owner, &book.id);
                search::note_unchanged(self.pool, &dir, &book.id, &bytes, seen_feed, seen_parser)
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
                    seen: Some((seen_feed, seen_parser)),
                    epub: None,
                    replaced_at: None,
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
                    b.posts_indexed
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

impl Report {
    pub(crate) fn failed(book_id: &str, message: String) -> Self {
        Self {
            book_id: book_id.to_owned(),
            title: String::new(),
            owner: String::new(),
            old_posts: 0,
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
}

/// Kind, text, and chapter. Alignment still ignores chapter; this is only
/// the "nothing to rewrite" check, so a chapter-only revision is not skipped.
fn same_posts(old: &[Post], new: &[Post]) -> bool {
    old.len() == new.len()
        && old.iter().zip(new).all(|(left, right)| {
            left.kind == right.kind && left.text == right.text && left.chapter == right.chapter
        })
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
    fn chapter_only_is_not_the_same_feed() {
        let post = Post {
            id: "0".to_owned(),
            kind: crate::feed::PostKind::Body,
            text: "Hi".to_owned(),
            chapter: "I".to_owned(),
        };
        let mut retitled = post.clone();
        retitled.chapter = "II".to_owned();
        assert!(same_posts(&[post.clone()], &[post.clone()]));
        assert!(!same_posts(&[post], &[retitled]));
    }

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
