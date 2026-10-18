use crate::feed::Post;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookRecord {
    pub id: String,
    pub title: String,
    pub author: String,
    pub handle: String,
    pub imported_at: i64,
    pub post_count: i32,
    pub progress_index: i32,
    pub is_sample: bool,
    pub mine: bool,
    /// Owner chose to show this book on other accounts' shelves.
    pub shared: bool,
    /// Username of the account that added the book.
    pub owner_username: String,
}

#[derive(Debug, FromRow)]
pub struct BookRow {
    pub id: String,
    pub title: String,
    pub author: String,
    pub handle: String,
    pub imported_at: i64,
    pub post_count: i64,
    pub progress_index: i64,
    pub is_sample: i64,
    pub shared: i64,
    pub owner_id: String,
    pub owner_username: String,
}

impl BookRow {
    pub fn into_record(self, user_id: &str) -> BookRecord {
        BookRecord {
            id: self.id,
            title: self.title,
            author: self.author,
            handle: self.handle,
            imported_at: self.imported_at,
            post_count: self.post_count as i32,
            progress_index: self.progress_index as i32,
            is_sample: self.is_sample != 0,
            mine: self.owner_id == user_id,
            shared: self.shared != 0,
            owner_username: self.owner_username,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LibrarySnapshot {
    pub books: Vec<BookRecord>,
}

#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub username: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub username: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchBook {
    /// Accepted for old clients but ignored: posts and their count belong to the server.
    pub post_count: Option<i32>,
    pub progress_index: Option<i32>,
    /// Only the owner may set this. Omitted leaves sharing unchanged.
    pub shared: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct LikeSet {
    pub ids: Vec<String>,
}

/// Compose field shows about 8 lines of 40 characters. A long paragraph
/// in the sample books stays under 1000. 2000 is six times the visible
/// field and still a hard stop if someone pastes a chapter.
pub const MAX_COMMENT_CHARS: usize = 2000;

/// Feed post ids are sequential indexes as decimal strings. 64 is well
/// above a 100k-post book and rejects junk in the path.
pub const MAX_POST_ID_CHARS: usize = 64;

#[derive(Debug, Serialize)]
pub struct CommentIndex {
    pub counts: HashMap<String, i64>,
    pub mine: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentRecord {
    pub id: String,
    pub username: String,
    pub text: String,
    pub created_at: i64,
    pub mine: bool,
}

#[derive(Debug, Serialize)]
pub struct CommentThread {
    pub comments: Vec<CommentRecord>,
}

#[derive(Debug, Deserialize)]
pub struct NewComment {
    pub text: String,
}

#[derive(Debug, FromRow)]
pub struct CommentRow {
    pub id: String,
    pub username: String,
    pub text: String,
    pub created_at: i64,
    pub user_id: String,
}

impl CommentRow {
    pub fn into_record(self, user_id: &str) -> CommentRecord {
        CommentRecord {
            id: self.id,
            username: self.username,
            text: self.text,
            created_at: self.created_at,
            mine: self.user_id == user_id,
        }
    }
}

#[derive(Debug, FromRow)]
pub struct ProfileReplyRow {
    pub id: String,
    pub book_id: String,
    pub post_id: String,
    pub username: String,
    pub text: String,
    pub created_at: i64,
    pub user_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReplyRecord {
    pub book_id: String,
    pub post_id: String,
    pub comment: CommentRecord,
}

#[derive(Debug, Serialize)]
pub struct ProfileReplies {
    pub username: String,
    pub replies: Vec<ProfileReplyRecord>,
}

/// Query value for `type`. Serde name is lowercase (`accounts`, `books`, `posts`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchKind {
    Accounts,
    Books,
    Posts,
}

#[derive(Debug, Serialize)]
pub struct AccountRecord {
    pub username: String,
    pub mine: bool,
}

#[derive(Debug, Serialize)]
pub struct PostHitRecord {
    pub book: BookRecord,
    pub post: Post,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SearchHits {
    Accounts { items: Vec<AccountRecord> },
    Books { items: Vec<BookRecord> },
    Posts { items: Vec<PostHitRecord> },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    #[serde(flatten)]
    pub hits: SearchHits,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRecord {
    pub username: String,
    pub joined_at: i64,
    pub post_count: i64,
    pub mine: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfilePostRecord {
    pub book: BookRecord,
    pub post: Post,
    pub comment: CommentRecord,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePostsPage {
    pub items: Vec<ProfilePostRecord>,
    pub next_cursor: Option<String>,
}
