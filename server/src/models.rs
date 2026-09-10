use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
}

impl BookRow {
    pub fn into_record(self) -> BookRecord {
        BookRecord {
            id: self.id,
            title: self.title,
            author: self.author,
            handle: self.handle,
            imported_at: self.imported_at,
            post_count: self.post_count as i32,
            progress_index: self.progress_index as i32,
            is_sample: self.is_sample != 0,
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
