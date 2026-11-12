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
    /// Username of the account that added the book. Empty for a private hold,
    /// so that account is not told who else has the copy. A shared book still
    /// names the owner.
    pub owner_username: String,
    /// The viewer has an imported request for this book and is not the owner.
    /// Set whether or not the book is shared, so they can drop their own copy.
    #[serde(default)]
    pub held: bool,
    /// Bumped when a reparse renumbers posts. Clients drop a cached feed when
    /// it changes. Absent on a server that predates revisions.
    #[serde(default)]
    pub feed_rev: i64,
    /// The imported file still has a release the owner can report.
    #[serde(default)]
    pub has_release: bool,
    /// The viewer added this book and it still has a reportable release.
    #[serde(default)]
    pub reportable: bool,
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
    /// Directory owner. Missing from selects that never open the file.
    #[sqlx(default)]
    pub files_owner: Option<String>,
    /// 1 when this viewer has an imported request for the book.
    /// Selects that never needed it leave the column out.
    #[sqlx(default)]
    pub holds: i64,
    /// Missing from a select that does not return the book to a client.
    #[sqlx(default)]
    pub feed_rev: i64,
    /// 1 when this copy still has the release it was grabbed from.
    #[sqlx(default)]
    pub has_release: i64,
}

impl BookRow {
    /// Account id the epub directory is stored under. An ownership transfer
    /// leaves the file where the first importer wrote it.
    pub(crate) fn dir_owner(&self) -> &str {
        self.files_owner.as_deref().unwrap_or(&self.owner_id)
    }

    pub fn into_record(self, user_id: &str) -> BookRecord {
        let mine = self.owner_id == user_id;
        let shared = self.shared != 0;
        let is_sample = self.is_sample != 0;
        // A private book is visible to a non-owner only because they hold it.
        // `holds` covers the same fact once the book is shared.
        let private_hold = !mine && !shared && !is_sample;
        let held = !mine && (self.holds != 0 || private_hold);
        BookRecord {
            id: self.id,
            title: self.title,
            author: self.author,
            handle: self.handle,
            imported_at: self.imported_at,
            post_count: self.post_count as i32,
            progress_index: self.progress_index as i32,
            is_sample,
            mine,
            shared,
            owner_username: if private_hold {
                String::new()
            } else {
                self.owner_username
            },
            held,
            feed_rev: self.feed_rev,
            has_release: self.has_release != 0,
            reportable: mine && self.has_release != 0,
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

/// `POST /v1/auth/register`. Login keeps [`AuthRequest`]. A missing or blank
/// `invite` is the ordinary signup path.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub invite: Option<String>,
    /// Optional. Verified by mail before it is stored on the account.
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub username: String,
    pub is_admin: bool,
    /// Catalog search and book requests are on. Every signed-in account.
    pub can_request: bool,
    /// Verified address, or null.
    pub email: Option<String>,
    /// Address waiting on its verify link, or null.
    pub pending_email: Option<String>,
    /// This server can mail reset and verify links.
    pub password_reset: bool,
}

/// Public `GET /v1/instance`. A future `name` is one more field.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInfo {
    pub allow_signup: bool,
    /// "Forgot password?" works here.
    pub password_reset: bool,
}

/// `POST /v1/auth/reset/request`. A username or an email.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetRequest {
    pub login: String,
}

/// `POST /v1/auth/reset`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RedeemReset {
    pub token: String,
    pub new_password: String,
}

/// `POST /v1/auth/email/verify`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerifyEmail {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct VerifiedEmail {
    pub email: String,
}

/// `PUT /v1/me/email`. `email: null` removes the address.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetEmail {
    pub email: Option<String>,
    pub current_password: String,
}

/// `POST /v1/admin/mail/test`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailTest {
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct AdminUsers {
    pub items: Vec<AdminUser>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    pub username: String,
    pub joined_at: i64,
    pub is_admin: bool,
    pub books: i64,
    pub mine: bool,
    pub disabled: bool,
}

/// Both fields are optional. A patch with neither is a 400.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatchUser {
    pub is_admin: Option<bool>,
    pub disabled: Option<bool>,
}

/// An admin reset. Shown once and never stored.
#[derive(Debug, Serialize)]
pub struct TempPassword {
    pub password: String,
}

/// What deleting an account removes. The preview and the delete return the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRemoval {
    /// Books deleted with their files.
    pub books_deleted: i64,
    /// Of those, books someone else could read (shared, or a sample).
    pub shared_deleted: i64,
    /// Books another account also holds. They pass to that account.
    pub books_passed: i64,
    /// Books from other accounts this account was holding.
    pub holds_released: i64,
    /// Active requests canceled. Followers move to a new leader.
    pub requests_canceled: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub users: i64,
    pub books: i64,
    pub comments: i64,
    pub reposts: i64,
    pub storage_bytes: u64,
    /// Active invites: unused, unrevoked, and not yet expired.
    pub invites: i64,
    /// Releases a reader reported as the wrong file.
    pub blocked: i64,
}

#[derive(Debug, Deserialize)]
pub struct InviteCheckRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct InviteCheck {
    pub status: crate::auth::invite::CheckStatus,
}

fn default_invite_days() -> u32 {
    crate::auth::invite::DEFAULT_DAYS
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewInvite {
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default = "default_invite_days")]
    pub days: u32,
}

#[derive(Debug, Serialize)]
pub struct CreatedInvite {
    pub invite: crate::auth::invite::AdminInvite,
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct AdminInvites {
    pub items: Vec<crate::auth::invite::AdminInvite>,
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
    /// Posts with at least one repost. Zero counts are omitted, same as `counts`.
    pub reposts: HashMap<String, i64>,
    /// Post ids this viewer has reposted. Same shape as `mine` and like ids.
    pub reposted: Vec<String>,
}

/// Body of `PUT` and `DELETE /books/{id}/posts/{postId}/repost`.
#[derive(Debug, Serialize)]
pub struct RepostState {
    pub reposts: i64,
    pub reposted: bool,
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
#[serde(rename_all = "camelCase")]
pub struct NewComment {
    pub text: String,
    /// Client-made comment id. Must be the lowercase hyphenated form; anything
    /// else is 400. A replay with the same id, user, book and post returns the
    /// thread without a second row. The same id from anyone else, or on a
    /// different book or post, is 409. Omitted keeps today's server id.
    pub id: Option<String>,
    /// The client wrote this while the book was private to it. Only ever raises privacy.
    #[serde(default)]
    pub private: bool,
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
    pub repost_count: i64,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRepostRecord {
    pub book: BookRecord,
    pub post: Post,
    pub reposted_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRepostsPage {
    pub items: Vec<ProfileRepostRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProfileBooks {
    pub items: Vec<BookRecord>,
}

#[derive(Debug, Serialize)]
pub struct ActorRecord {
    pub username: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FeedItem {
    Comment {
        actor: ActorRecord,
        book: BookRecord,
        post: Post,
        comment: CommentRecord,
        at: i64,
    },
    Repost {
        actor: ActorRecord,
        book: BookRecord,
        post: Post,
        at: i64,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPage {
    pub items: Vec<FeedItem>,
    pub next_cursor: Option<String>,
}
