//! One-time signup links. The wire token is 64 hex chars from [`new_token`]; the row stores
//! [`session::hash_token`]. Only this module writes `invites`.

use super::new_token;
use super::session::hash_token;
use crate::error::ApiError;
use serde::Serialize;
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use uuid::Uuid;

pub const DEFAULT_DAYS: u32 = 7;
pub const MAX_DAYS: u32 = 90;
pub const MAX_NOTE_CHARS: usize = 200;

const DAY_MS: i64 = 86_400_000;

const SELECT_ADMIN: &str = "\
SELECT invites.id AS id,
       invites.note AS note,
       creator.username AS created_by,
       invites.created_at AS created_at,
       invites.expires_at AS expires_at,
       invites.used_at AS used_at,
       recipient.username AS used_by,
       invites.revoked_at AS revoked_at
FROM invites
LEFT JOIN users AS creator ON creator.id = invites.created_by
LEFT JOIN users AS recipient ON recipient.id = invites.used_by_user_id";

/// Admin view state. Precedence: used, then revoked, then expired, then active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InviteState {
    Active,
    Used,
    Expired,
    Revoked,
}

/// Public check. Unknown and revoked are both [`CheckStatus::Invalid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Valid,
    Used,
    Expired,
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminInvite {
    pub id: String,
    pub note: Option<String>,
    /// Username of the admin who created the link. Null for the CLI.
    pub created_by: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_at: Option<i64>,
    /// Username of the account that redeemed the link. Null when that account was deleted.
    pub used_by: Option<String>,
    pub revoked_at: Option<i64>,
    pub state: InviteState,
}

#[derive(Debug, FromRow)]
struct InviteRow {
    id: String,
    note: Option<String>,
    created_by: Option<String>,
    created_at: i64,
    expires_at: i64,
    used_at: Option<i64>,
    used_by: Option<String>,
    revoked_at: Option<i64>,
}

impl InviteRow {
    fn state(&self, now: i64) -> InviteState {
        if self.used_at.is_some() {
            InviteState::Used
        } else if self.revoked_at.is_some() {
            InviteState::Revoked
        } else if self.expires_at <= now {
            InviteState::Expired
        } else {
            InviteState::Active
        }
    }

    fn into_admin(self, now: i64) -> AdminInvite {
        let state = self.state(now);
        AdminInvite {
            id: self.id,
            note: self.note,
            created_by: self.created_by,
            created_at: self.created_at,
            expires_at: self.expires_at,
            used_at: self.used_at,
            used_by: self.used_by,
            revoked_at: self.revoked_at,
            state,
        }
    }
}

/// Prepared by the auth handler after validation and Argon2. No raw password and no admin flag.
pub struct NewAccount<'a> {
    pub username: &'a str,
    pub username_norm: &'a str,
    pub password_hash: &'a str,
}

fn clean_note(note: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) else {
        return Ok(None);
    };
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(ApiError::BadRequest(
            "Note must be 200 characters or fewer.".into(),
        ));
    }
    Ok(Some(note.to_owned()))
}

fn clean_days(days: u32) -> Result<u32, ApiError> {
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(ApiError::BadRequest("Expiry must be 1 to 90 days.".into()));
    }
    Ok(days)
}

fn expires_at(now: i64, days: u32) -> Result<i64, ApiError> {
    let span = i64::from(days)
        .checked_mul(DAY_MS)
        .and_then(|span| now.checked_add(span))
        .ok_or(ApiError::Internal)?;
    Ok(span)
}

/// Exactly 64 lowercase ASCII hex characters, the shape [`new_token`] returns.
fn token_shape(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn status_of(
    used_at: Option<i64>,
    revoked_at: Option<i64>,
    expires_at: i64,
    now: i64,
) -> CheckStatus {
    if used_at.is_some() {
        CheckStatus::Used
    } else if revoked_at.is_some() {
        CheckStatus::Invalid
    } else if expires_at <= now {
        CheckStatus::Expired
    } else {
        CheckStatus::Valid
    }
}

fn refusal(status: CheckStatus) -> ApiError {
    match status {
        CheckStatus::Used => ApiError::Forbidden("This invite was already used.".into()),
        CheckStatus::Expired => ApiError::Forbidden("This invite has expired.".into()),
        CheckStatus::Invalid => ApiError::Forbidden("This invite link isn't valid.".into()),
        CheckStatus::Valid => ApiError::Internal,
    }
}

async fn rollback(tx: sqlx::Transaction<'_, sqlx::Sqlite>, err: ApiError) -> ApiError {
    if let Err(rollback_err) = tx.rollback().await {
        tracing::error!(error = %rollback_err, "invite transaction rollback failed");
        return ApiError::Internal;
    }
    err
}

async fn load(conn: &mut SqliteConnection, id: &str) -> Result<Option<InviteRow>, sqlx::Error> {
    let sql = format!("{SELECT_ADMIN} WHERE invites.id = ?");
    sqlx::query_as::<_, InviteRow>(&sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

async fn classify(
    conn: &mut SqliteConnection,
    token: &str,
    now: i64,
) -> Result<CheckStatus, ApiError> {
    if !token_shape(token) {
        return Ok(CheckStatus::Invalid);
    }
    let row = sqlx::query_as::<_, (Option<i64>, Option<i64>, i64)>(
        "SELECT used_at, revoked_at, expires_at FROM invites WHERE token_hash = ?",
    )
    .bind(hash_token(token))
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match row {
        None => CheckStatus::Invalid,
        Some((used_at, revoked_at, expires_at)) => status_of(used_at, revoked_at, expires_at, now),
    })
}

/// Validates the note and the lifetime, then inserts one row. The raw token is returned once.
pub async fn create(
    pool: &SqlitePool,
    created_by: Option<&str>,
    note: Option<&str>,
    days: u32,
    now: i64,
) -> Result<(AdminInvite, String), ApiError> {
    let note = clean_note(note)?;
    let days = clean_days(days)?;
    let expires_at = expires_at(now, days)?;
    let id = Uuid::new_v4().to_string();
    let token = new_token();
    sqlx::query(
        "INSERT INTO invites (id, token_hash, note, created_by, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(hash_token(&token))
    .bind(&note)
    .bind(created_by)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    let mut conn = pool.acquire().await?;
    let row = load(&mut conn, &id).await?.ok_or(ApiError::Internal)?;
    Ok((row.into_admin(now), token))
}

/// Every row, newest first. Ties break by id. Older active links stay in the list.
pub async fn list(pool: &SqlitePool, now: i64) -> Result<Vec<AdminInvite>, ApiError> {
    let sql = format!("{SELECT_ADMIN} ORDER BY invites.created_at DESC, invites.id DESC");
    let rows = sqlx::query_as::<_, InviteRow>(&sql).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|row| row.into_admin(now)).collect())
}

/// Unknown id is [`ApiError::NotFound`]. A used invite is 409. Revoking twice returns the same row.
/// An expired unused invite can still be revoked.
pub async fn revoke(pool: &SqlitePool, id: &str, now: i64) -> Result<AdminInvite, ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let row = match load(&mut tx, id).await {
        Ok(row) => row,
        Err(err) => return Err(rollback(tx, err.into()).await),
    };
    let Some(row) = row else {
        return Err(rollback(tx, ApiError::NotFound).await);
    };
    if row.used_at.is_some() {
        return Err(rollback(
            tx,
            ApiError::Conflict("This invite was already used.".into()),
        )
        .await);
    }
    if row.revoked_at.is_some() {
        let invite = row.into_admin(now);
        tx.commit().await?;
        return Ok(invite);
    }
    let updated = sqlx::query(
        "UPDATE invites SET revoked_at = ?
         WHERE id = ? AND used_at IS NULL AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(id)
    .execute(&mut *tx)
    .await;
    let updated = match updated {
        Ok(updated) => updated,
        Err(err) => return Err(rollback(tx, err.into()).await),
    };
    if updated.rows_affected() != 1 {
        let row = match load(&mut tx, id).await {
            Ok(row) => row,
            Err(err) => return Err(rollback(tx, err.into()).await),
        };
        let Some(row) = row else {
            return Err(rollback(tx, ApiError::NotFound).await);
        };
        if row.used_at.is_some() {
            return Err(rollback(
                tx,
                ApiError::Conflict("This invite was already used.".into()),
            )
            .await);
        }
        if row.revoked_at.is_some() {
            let invite = row.into_admin(now);
            tx.commit().await?;
            return Ok(invite);
        }
        return Err(rollback(tx, ApiError::Internal).await);
    }
    let row = match load(&mut tx, id).await {
        Ok(Some(row)) => row,
        Ok(None) => return Err(rollback(tx, ApiError::Internal).await),
        Err(err) => return Err(rollback(tx, err.into()).await),
    };
    let invite = row.into_admin(now);
    tx.commit().await?;
    Ok(invite)
}

/// Read-only. Malformed, unknown, and revoked tokens are [`CheckStatus::Invalid`].
/// The token is classified as given: this does not trim.
pub async fn check(pool: &SqlitePool, token: &str, now: i64) -> Result<CheckStatus, ApiError> {
    let mut conn = pool.acquire().await?;
    classify(&mut conn, token, now).await
}

/// [`check`], then [`CheckStatus::Valid`] is ok. Anything else is a 403. This is not redemption.
pub async fn admit(pool: &SqlitePool, token: &str, now: i64) -> Result<(), ApiError> {
    let status = check(pool, token, now).await?;
    if status == CheckStatus::Valid {
        Ok(())
    } else {
        Err(refusal(status))
    }
}

/// Inserts a non-admin user and consumes `token` in one `BEGIN IMMEDIATE`.
/// `clock` is called only after the write lock is held.
/// A failure rolls both writes back. Returns the new user id.
pub async fn register(
    pool: &SqlitePool,
    token: &str,
    account: NewAccount<'_>,
    clock: impl FnOnce() -> i64,
) -> Result<String, ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let now = clock();
    let id = Uuid::new_v4().to_string();
    let inserted = sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(account.username)
    .bind(account.username_norm)
    .bind(account.password_hash)
    .bind(now)
    .execute(&mut *tx)
    .await;
    if let Err(err) = inserted {
        let err = match err {
            sqlx::Error::Database(err) if err.is_unique_violation() => {
                ApiError::Conflict("That username is taken.".into())
            }
            other => other.into(),
        };
        return Err(rollback(tx, err).await);
    }
    if let Err(err) = redeem(&mut tx, token, &id, now).await {
        return Err(rollback(tx, err).await);
    }
    tx.commit().await?;
    Ok(id)
}

/// Consumes one invite on the caller's transaction, after the user row exists.
async fn redeem(
    conn: &mut SqliteConnection,
    token: &str,
    user_id: &str,
    now: i64,
) -> Result<(), ApiError> {
    let updated = sqlx::query(
        "UPDATE invites SET used_at = ?, used_by_user_id = ?
         WHERE token_hash = ? AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .bind(user_id)
    .bind(hash_token(token))
    .bind(now)
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() == 1 {
        return Ok(());
    }
    let status = classify(conn, token, now).await?;
    Err(refusal(status))
}

/// Unused, unrevoked, and not yet expired.
pub async fn active_count(pool: &SqlitePool, now: i64) -> Result<i64, ApiError> {
    let count = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invites
         WHERE used_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use sqlx::migrate::Migrate;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;

    const T0: i64 = 2_000_000_000_000;

    async fn file_pool(connections: u32) -> (TempDir, SqlitePool) {
        let tmp = TempDir::new().unwrap();
        let pool = open_file(tmp.path(), connections).await;
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        (tmp, pool)
    }

    async fn open_file(path: &Path, connections: u32) -> SqlitePool {
        let url = format!("sqlite://{}?mode=rwc", path.join("mewo.db").display());
        let options = SqliteConnectOptions::from_str(&url)
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(db::BUSY_TIMEOUT);
        SqlitePoolOptions::new()
            .max_connections(connections)
            .connect_with(options)
            .await
            .unwrap()
    }

    async fn user(pool: &SqlitePool, id: &str, username: &str) {
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES (?, ?, ?, 'x', 1)",
        )
        .bind(id)
        .bind(username)
        .bind(username.to_ascii_lowercase())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn user_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn table_exists(pool: &SqlitePool, name: &str) -> bool {
        let found: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await
                .unwrap();
        found.is_some()
    }

    fn account<'a>(username: &'a str) -> NewAccount<'a> {
        NewAccount {
            username,
            username_norm: username,
            password_hash: "hash",
        }
    }

    fn bad_request(err: ApiError) -> String {
        match err {
            ApiError::BadRequest(message) => message,
            other => panic!("{other:?}"),
        }
    }

    fn forbidden(err: ApiError) -> String {
        match err {
            ApiError::Forbidden(message) => message,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn create_stores_the_hash_and_defaults_to_seven_days() {
        let (_tmp, pool) = file_pool(1).await;
        user(&pool, "admin", "Ada").await;
        let (invite, token) = create(&pool, Some("admin"), Some("  for ada  "), DEFAULT_DAYS, T0)
            .await
            .unwrap();
        assert_eq!(token.len(), 64);
        assert!(token_shape(&token));
        assert_eq!(invite.note.as_deref(), Some("for ada"));
        assert_eq!(invite.created_by.as_deref(), Some("Ada"));
        assert_eq!(invite.created_at, T0);
        assert_eq!(invite.expires_at, T0 + i64::from(DEFAULT_DAYS) * DAY_MS);
        assert_eq!(invite.state, InviteState::Active);
        assert!(invite.used_at.is_none());
        assert!(invite.used_by.is_none());
        assert!(invite.revoked_at.is_none());

        let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM invites WHERE id = ?")
            .bind(&invite.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored, hash_token(&token));
        assert_ne!(stored, token.as_bytes());
        let raw_hits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invites WHERE token_hash = ?")
            .bind(token.as_bytes())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(raw_hits, 0);
        let hash_hits: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM invites WHERE token_hash = ?")
                .bind(hash_token(&token))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(hash_hits, 1);
        let texts: (String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT id, note, created_by, used_by_user_id FROM invites WHERE id = ?",
        )
        .bind(&invite.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        for value in [Some(texts.0), texts.1, texts.2, texts.3]
            .into_iter()
            .flatten()
        {
            assert_ne!(value, token);
            assert!(!value.contains(&token));
        }

        let (blank, _) = create(&pool, None, Some("   "), 7, T0).await.unwrap();
        assert!(blank.note.is_none());
        assert!(blank.created_by.is_none());
        let note: Option<String> = sqlx::query_scalar("SELECT note FROM invites WHERE id = ?")
            .bind(&blank.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(note.is_none());
    }

    #[tokio::test]
    async fn create_rejects_a_bad_lifetime_and_a_long_note() {
        let (_tmp, pool) = file_pool(1).await;
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invites")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            bad_request(create(&pool, None, None, 0, T0).await.unwrap_err()),
            "Expiry must be 1 to 90 days."
        );
        assert_eq!(
            bad_request(create(&pool, None, None, 91, T0).await.unwrap_err()),
            "Expiry must be 1 to 90 days."
        );
        assert_eq!(
            bad_request(
                create(&pool, None, Some(&"a".repeat(201)), 7, T0)
                    .await
                    .unwrap_err()
            ),
            "Note must be 200 characters or fewer."
        );
        let wide = "é".repeat(200);
        let (ok, _) = create(&pool, None, Some(&wide), 1, T0).await.unwrap();
        assert_eq!(ok.note.as_deref(), Some(wide.as_str()));
        assert_eq!(ok.expires_at, T0 + DAY_MS);
        assert_eq!(
            bad_request(
                create(&pool, None, Some(&"é".repeat(201)), 7, T0)
                    .await
                    .unwrap_err()
            ),
            "Note must be 200 characters or fewer."
        );
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invites")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after, before + 1);
    }

    #[tokio::test]
    async fn state_precedence_and_the_expiry_boundary() {
        let (_tmp, pool) = file_pool(1).await;
        let (invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        let expires = invite.expires_at;
        assert_eq!(
            check(&pool, &token, expires - 1).await.unwrap(),
            CheckStatus::Valid
        );
        assert_eq!(
            list(&pool, expires - 1).await.unwrap()[0].state,
            InviteState::Active
        );
        assert_eq!(
            check(&pool, &token, expires).await.unwrap(),
            CheckStatus::Expired
        );
        assert_eq!(
            list(&pool, expires).await.unwrap()[0].state,
            InviteState::Expired
        );
        assert_eq!(active_count(&pool, expires - 1).await.unwrap(), 1);
        assert_eq!(active_count(&pool, expires).await.unwrap(), 0);

        let id = register(&pool, &token, account("ada"), || T0)
            .await
            .unwrap();
        sqlx::query("UPDATE invites SET expires_at = ? WHERE id = ?")
            .bind(T0)
            .bind(&invite.id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            check(&pool, &token, T0 + 1).await.unwrap(),
            CheckStatus::Used
        );
        let listed = list(&pool, T0 + 1).await.unwrap();
        assert_eq!(listed[0].state, InviteState::Used);
        assert_eq!(listed[0].used_by.as_deref(), Some("ada"));
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let listed = list(&pool, T0 + 1).await.unwrap();
        assert_eq!(listed[0].state, InviteState::Used);
        assert!(listed[0].used_by.is_none());
    }

    #[tokio::test]
    async fn check_classifies_used_expired_unknown_malformed_and_revoked() {
        let (_tmp, pool) = file_pool(1).await;
        let (invite, token) = create(&pool, None, Some("n"), 7, T0).await.unwrap();
        assert_eq!(check(&pool, &token, T0).await.unwrap(), CheckStatus::Valid);
        assert_eq!(check(&pool, "zz", T0).await.unwrap(), CheckStatus::Invalid);
        assert_eq!(check(&pool, "", T0).await.unwrap(), CheckStatus::Invalid);
        assert_eq!(
            check(&pool, &"A".repeat(64), T0).await.unwrap(),
            CheckStatus::Invalid
        );
        assert_eq!(
            check(&pool, "not-a-real-token", T0).await.unwrap(),
            CheckStatus::Invalid
        );
        let _ = register(&pool, &token, account("ada"), || T0)
            .await
            .unwrap();
        assert_eq!(check(&pool, &token, T0).await.unwrap(), CheckStatus::Used);

        let (second, other) = create(&pool, None, None, 7, T0).await.unwrap();
        assert_eq!(
            check(&pool, &other, second.expires_at).await.unwrap(),
            CheckStatus::Expired
        );
        revoke(&pool, &second.id, second.expires_at).await.unwrap();
        assert_eq!(
            check(&pool, &other, T0).await.unwrap(),
            CheckStatus::Invalid
        );
        let _ = invite;
    }

    #[tokio::test]
    async fn register_consumes_the_invite_and_a_second_call_is_used() {
        let (_tmp, pool) = file_pool(1).await;
        let (_invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        let id = register(&pool, &token, account("ada"), || T0)
            .await
            .unwrap();
        let is_admin: i64 = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(is_admin, 0);
        let (used_at, used_by): (Option<i64>, Option<String>) =
            sqlx::query_as("SELECT used_at, used_by_user_id FROM invites WHERE token_hash = ?")
                .bind(hash_token(&token))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(used_at, Some(T0));
        assert_eq!(used_by.as_deref(), Some(id.as_str()));
        assert_eq!(user_count(&pool).await, 1);
        assert_eq!(
            forbidden(
                register(&pool, &token, account("bea"), || T0)
                    .await
                    .unwrap_err()
            ),
            "This invite was already used."
        );
        assert_eq!(user_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn register_rolls_back_the_user_when_the_invite_cannot_be_consumed() {
        let (_tmp, pool) = file_pool(1).await;
        let (invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        assert_eq!(
            forbidden(
                register(&pool, &token, account("ada"), || invite.expires_at)
                    .await
                    .unwrap_err()
            ),
            "This invite has expired."
        );
        assert_eq!(user_count(&pool).await, 0);
        assert!(still_unused(&pool, &invite.id).await);

        revoke(&pool, &invite.id, T0).await.unwrap();
        assert_eq!(
            forbidden(
                register(&pool, &token, account("ada"), || T0)
                    .await
                    .unwrap_err()
            ),
            "This invite link isn't valid."
        );
        assert_eq!(user_count(&pool).await, 0);

        let (_other, bad) = create(&pool, None, None, 7, T0).await.unwrap();
        let _ = bad;
        assert_eq!(
            forbidden(
                register(&pool, "zz", account("ada"), || T0)
                    .await
                    .unwrap_err()
            ),
            "This invite link isn't valid."
        );
        assert_eq!(user_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn register_rolls_back_a_taken_name_and_a_failed_update() {
        let (_tmp, pool) = file_pool(1).await;
        user(&pool, "ada-id", "ada").await;
        let (invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        match register(&pool, &token, account("ada"), || T0)
            .await
            .unwrap_err()
        {
            ApiError::Conflict(message) => assert_eq!(message, "That username is taken."),
            other => panic!("{other:?}"),
        }
        assert_eq!(user_count(&pool).await, 1);
        assert!(still_unused(&pool, &invite.id).await);

        let (second, token) = create(&pool, None, None, 7, T0).await.unwrap();
        sqlx::query(
            "CREATE TRIGGER invites_abort_update
             BEFORE UPDATE ON invites
             BEGIN
                 SELECT RAISE(ABORT, 'injected');
             END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let err = register(&pool, &token, account("bea"), || T0)
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::Internal), "{err:?}");
        assert_eq!(user_count(&pool).await, 1);
        assert!(still_unused(&pool, &second.id).await);
    }

    #[tokio::test]
    async fn list_is_newest_first_and_keeps_an_older_active_invite() {
        let (_tmp, pool) = file_pool(1).await;
        let (older, _) = create(&pool, None, Some("older"), 90, T0 - 10 * DAY_MS)
            .await
            .unwrap();
        let (newer, _) = create(&pool, None, Some("newer"), 7, T0).await.unwrap();
        let (same, _) = create(&pool, None, None, 7, T0).await.unwrap();
        let items = list(&pool, T0).await.unwrap();
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|item| item.id == older.id));
        assert_eq!(items.last().unwrap().id, older.id);
        let top: Vec<_> = items.iter().take(2).map(|item| item.id.as_str()).collect();
        let mut expected = vec![newer.id.as_str(), same.id.as_str()];
        expected.sort_by(|left, right| right.cmp(left));
        assert_eq!(top, expected);
        assert!(items.iter().all(|item| item.state == InviteState::Active));
        assert_eq!(active_count(&pool, T0).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn revoke_is_idempotent_and_rejects_used_or_unknown() {
        let (_tmp, pool) = file_pool(1).await;
        let (invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        let revoked = revoke(&pool, &invite.id, T0 + 5).await.unwrap();
        assert_eq!(revoked.state, InviteState::Revoked);
        assert_eq!(revoked.revoked_at, Some(T0 + 5));
        let again = revoke(&pool, &invite.id, T0 + 9).await.unwrap();
        assert_eq!(again.revoked_at, Some(T0 + 5));
        assert_eq!(again.state, InviteState::Revoked);

        let (expired, _) = create(&pool, None, None, 1, T0).await.unwrap();
        let revoked = revoke(&pool, &expired.id, expired.expires_at)
            .await
            .unwrap();
        assert_eq!(revoked.state, InviteState::Revoked);

        let (used, used_token) = create(&pool, None, None, 7, T0).await.unwrap();
        register(&pool, &used_token, account("ada"), || T0)
            .await
            .unwrap();
        match revoke(&pool, &used.id, T0).await.unwrap_err() {
            ApiError::Conflict(message) => assert_eq!(message, "This invite was already used."),
            other => panic!("{other:?}"),
        }
        let still_used: Option<i64> =
            sqlx::query_scalar("SELECT revoked_at FROM invites WHERE id = ?")
                .bind(&used.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(still_used.is_none());
        match revoke(&pool, "missing", T0).await.unwrap_err() {
            ApiError::NotFound => {}
            other => panic!("{other:?}"),
        }
        let _ = token;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_register_consumes_the_invite_once() {
        let (_tmp, pool) = file_pool(2).await;
        assert!(pool.options().get_max_connections() >= 2);
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let (_invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut handles = Vec::new();
        for name in ["ada", "bea"] {
            let pool = pool.clone();
            let token = token.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                register(&pool, &token, account(name), || T0).await
            }));
        }
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }
        let mut winner = None;
        let mut used = 0;
        for result in results {
            match result {
                Ok(id) => {
                    assert!(winner.is_none());
                    winner = Some(id);
                }
                Err(err) => {
                    used += 1;
                    assert_eq!(forbidden(err), "This invite was already used.");
                }
            }
        }
        assert_eq!(used, 1);
        let winner = winner.unwrap();
        assert_eq!(user_count(&pool).await, 1);
        let used_by: String =
            sqlx::query_scalar("SELECT used_by_user_id FROM invites WHERE token_hash = ?")
                .bind(hash_token(&token))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(used_by, winner);
        let is_admin: i64 = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = ?")
            .bind(&winner)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(is_admin, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_revoke_and_register_cannot_both_win() {
        let (_tmp, pool) = file_pool(2).await;
        let (invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let register_task = {
            let pool = pool.clone();
            let token = token.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                register(&pool, &token, account("ada"), || T0).await
            })
        };
        let revoke_task = {
            let pool = pool.clone();
            let id = invite.id.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                revoke(&pool, &id, T0).await
            })
        };
        let registered = register_task.await.unwrap();
        let revoked = revoke_task.await.unwrap();
        let (used_at, revoked_at): (Option<i64>, Option<i64>) =
            sqlx::query_as("SELECT used_at, revoked_at FROM invites WHERE id = ?")
                .bind(&invite.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(used_at.is_none() || revoked_at.is_none());
        match (used_at, revoked_at) {
            (Some(_), None) => {
                assert!(registered.is_ok());
                match revoked.unwrap_err() {
                    ApiError::Conflict(message) => {
                        assert_eq!(message, "This invite was already used.")
                    }
                    other => panic!("{other:?}"),
                }
                assert_eq!(user_count(&pool).await, 1);
            }
            (None, Some(_)) => {
                assert!(revoked.is_ok());
                assert_eq!(
                    forbidden(registered.unwrap_err()),
                    "This invite link isn't valid."
                );
                assert_eq!(user_count(&pool).await, 0);
            }
            other => panic!("invite ended as {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn register_reads_the_clock_after_the_write_lock() {
        let (_tmp, pool) = file_pool(4).await;
        let (invite, token) = create(&pool, None, None, 7, T0).await.unwrap();
        let expires = invite.expires_at;
        let now = Arc::new(AtomicI64::new(expires - 1));
        let clock_ran = Arc::new(AtomicBool::new(false));
        let mut holder = pool.acquire().await.unwrap();
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *holder)
            .await
            .unwrap();
        let baseline_size = pool.size();
        let baseline_idle = pool.num_idle();
        let pool_for_task = pool.clone();
        let token_for_task = token.clone();
        let now_for_task = now.clone();
        let clock_for_task = clock_ran.clone();
        let task = tokio::spawn(async move {
            register(&pool_for_task, &token_for_task, account("ada"), move || {
                let sampled = now_for_task.load(Ordering::SeqCst);
                clock_for_task.store(true, Ordering::SeqCst);
                sampled
            })
            .await
        });
        let started = std::time::Instant::now();
        loop {
            let waiting = pool.size() > baseline_size || pool.num_idle() < baseline_idle;
            if clock_ran.load(Ordering::SeqCst) || waiting {
                break;
            }
            if started.elapsed() > Duration::from_secs(5) {
                panic!("register did not reach the write lock");
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        now.store(expires, Ordering::SeqCst);
        sqlx::query("COMMIT").execute(&mut *holder).await.unwrap();
        drop(holder);
        assert_eq!(
            forbidden(task.await.unwrap().unwrap_err()),
            "This invite has expired."
        );
        assert!(clock_ran.load(Ordering::SeqCst));
        assert_eq!(user_count(&pool).await, 0);
        assert!(still_unused(&pool, &invite.id).await);
    }

    #[tokio::test]
    async fn migration_011_applies_on_a_fresh_database_and_after_010() {
        let migrator = sqlx::migrate!("./migrations");
        let listed: Vec<(i64, &str)> = migrator
            .iter()
            .map(|migration| (migration.version, migration.description.as_ref()))
            .collect();
        assert!(listed.windows(2).all(|pair| pair[0].0 < pair[1].0));
        let ten = listed.iter().position(|row| row.0 == 10).unwrap();
        let eleven = listed.iter().position(|row| row.0 == 11).unwrap();
        assert!(ten < eleven);
        assert_eq!(listed[ten].1, "reparse");
        assert_eq!(listed[eleven].1, "invites");
        assert_eq!(listed.last().unwrap().0, 13);
        assert_eq!(listed.last().unwrap().1, "account disable");

        let (_tmp, fresh) = file_pool(1).await;
        assert_invites(&fresh).await;

        let tmp = TempDir::new().unwrap();
        let stopped = open_file(tmp.path(), 1).await;
        let migrator = sqlx::migrate!("./migrations");
        let mut conn = stopped.acquire().await.unwrap();
        Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
        for migration in migrator.iter() {
            if migration.version < 11 {
                Migrate::apply(&mut *conn, migration).await.unwrap();
            }
        }
        drop(conn);
        let applied: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&stopped)
                .await
                .unwrap();
        assert_eq!(applied.last().copied(), Some(10));
        assert!(!applied.contains(&11));
        assert!(!table_exists(&stopped, "invites").await);
        sqlx::migrate!("./migrations").run(&stopped).await.unwrap();
        assert_invites(&stopped).await;
    }

    async fn still_unused(pool: &SqlitePool, id: &str) -> bool {
        let row: (Option<i64>, Option<String>, Option<i64>) =
            sqlx::query_as("SELECT used_at, used_by_user_id, revoked_at FROM invites WHERE id = ?")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap();
        row.0.is_none() && row.1.is_none() && row.2.is_none()
    }

    async fn assert_invites(pool: &SqlitePool) {
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT version, description, success FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        let ten = rows.iter().find(|row| row.0 == 10).unwrap();
        let eleven = rows.iter().find(|row| row.0 == 11).unwrap();
        assert_eq!(ten.1, "reparse");
        assert_eq!(eleven.1, "invites");
        assert_eq!((ten.2, eleven.2), (1, 1));
        let twelve = rows.iter().find(|row| row.0 == 12).unwrap();
        assert_eq!(twelve.1, "replace blocklist");
        assert_eq!(twelve.2, 1);
        assert!(rows.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(rows.last().unwrap().0, 13);
        assert!(table_exists(pool, "release_blocklist").await);
        assert!(table_exists(pool, "work_aliases").await);
        assert!(table_exists(pool, "invites").await);
        let index: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'invites_created'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(index, 1);
        let token_hash: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('invites') WHERE name = 'token_hash'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(token_hash, 1);
    }
}
