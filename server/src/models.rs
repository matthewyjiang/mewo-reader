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
    pub owner_id: String,
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
    pub post_count: Option<i32>,
    pub progress_index: Option<i32>,
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
