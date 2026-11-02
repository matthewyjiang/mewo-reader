//! Bearer sessions. The wire token stays 64 hex characters. The row stores
//! `sha256(token)`. Lifetimes are fixed here so a caller can pass `now`.

use crate::error::ApiError;
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, SqlitePool};
use std::time::Duration;
use uuid::Uuid;

use super::Authed;

/// Idle limit, measured from `last_seen_at`.
pub const IDLE: Duration = Duration::from_secs(90 * 86_400);
/// Absolute limit, measured from issue. A request does not move it.
pub const ABSOLUTE: Duration = Duration::from_secs(365 * 86_400);
/// Write `last_seen_at` at most this often.
pub const TOUCH_AFTER: Duration = Duration::from_secs(3_600);
/// Oldest by `last_seen_at` is dropped on the next issue past this.
pub const MAX_PER_USER: i64 = 20;

/// `sha256(token)`, 32 bytes. Public so tests can seed rows.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn duration_ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).expect("session lifetime fits in i64 milliseconds")
}

/// Prune expired rows, enforce [`MAX_PER_USER`], insert. The raw token is
/// returned once and is not stored.
pub async fn issue(pool: &SqlitePool, user_id: &str, now: i64) -> Result<String, ApiError> {
    issue_in(pool, user_id, now, None).await
}

/// Stores `new_hash` only when `verified_hash` is still the row and
/// `session_id` still belongs to that user. The checks, the update, and the
/// revoke share one `BEGIN IMMEDIATE`, so a password change or revoke that
/// committed after verify is not overwritten.
pub async fn commit_password_change(
    pool: &SqlitePool,
    user_id: &str,
    session_id: &str,
    verified_hash: &str,
    new_hash: &str,
) -> Result<(), ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let alive: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM sessions WHERE id = ? AND user_id = ?")
            .bind(session_id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    if alive.is_none() {
        return Err(ApiError::Unauthorized("Sign in first.".into()));
    }
    let current: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    if current.as_deref() != Some(verified_hash) {
        return Err(ApiError::Forbidden("Wrong password.".into()));
    }
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    revoke_all(&mut tx, user_id, Some(session_id)).await?;
    tx.commit().await?;
    Ok(())
}

/// Login. `password_hash` is the hash just verified. The insert and the
/// re-check share one write transaction, so a password change that committed
/// in between does not leave a session for the old password.
pub async fn issue_for_login(
    pool: &SqlitePool,
    user_id: &str,
    now: i64,
    password_hash: &str,
) -> Result<String, ApiError> {
    issue_in(pool, user_id, now, Some(password_hash)).await
}

async fn issue_in(
    pool: &SqlitePool,
    user_id: &str,
    now: i64,
    password_hash: Option<&str>,
) -> Result<String, ApiError> {
    // IMMEDIATE when the hash is checked, so the read and the insert take the
    // write lock together. A deferred read then an insert fails at once if a
    // password change committed in between, and `busy_timeout` does not apply.
    let mut tx = if password_hash.is_some() {
        pool.begin_with("BEGIN IMMEDIATE").await?
    } else {
        pool.begin().await?
    };
    if let Some(expected) = password_hash {
        let current: Option<String> =
            sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await?;
        if current.as_deref() != Some(expected) {
            return Err(ApiError::Unauthorized("Wrong username or password.".into()));
        }
    }
    delete_expired(&mut tx, now).await?;
    trim_user(&mut tx, user_id).await?;
    let token = super::new_token();
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sessions (id, token_hash, user_id, created_at, last_seen_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(now)
    .bind(now)
    .bind(now.saturating_add(duration_ms(ABSOLUTE)))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(token)
}

/// Hash match, and both `expires_at > now` and `last_seen_at > now - IDLE`.
/// Touches `last_seen_at` when it is [`TOUCH_AFTER`] stale. A failed touch is
/// logged. The session still resolves.
pub async fn resolve(pool: &SqlitePool, token: &str, now: i64) -> Result<Option<Authed>, ApiError> {
    let idle_after = now.saturating_sub(duration_ms(IDLE));
    let row = sqlx::query_as::<_, Resolved>(
        "SELECT sessions.id AS session_id, users.id AS user_id, users.username,
                users.is_admin AS is_admin, sessions.last_seen_at AS last_seen_at
         FROM sessions
         JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = ?
           AND sessions.expires_at > ?
           AND sessions.last_seen_at > ?",
    )
    .bind(hash_token(token))
    .bind(now)
    .bind(idle_after)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if now.saturating_sub(row.last_seen_at) >= duration_ms(TOUCH_AFTER) {
        if let Err(err) =
            sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE id = ? AND last_seen_at = ?")
                .bind(now)
                .bind(&row.session_id)
                .bind(row.last_seen_at)
                .execute(pool)
                .await
        {
            tracing::error!(error = %err, session_id = %row.session_id, "session touch failed");
        }
    }
    Ok(Some(Authed {
        user_id: row.user_id,
        username: row.username,
        is_admin: row.is_admin,
        session_id: row.session_id,
    }))
}

/// Idempotent. An unknown id deletes nothing.
pub async fn revoke(pool: &SqlitePool, session_id: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes every session for `user_id`. `keep` is the caller's session id.
pub async fn revoke_all(
    conn: &mut SqliteConnection,
    user_id: &str,
    keep: Option<&str>,
) -> Result<u64, ApiError> {
    let result = match keep {
        Some(session_id) => {
            sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id <> ?")
                .bind(user_id)
                .bind(session_id)
                .execute(&mut *conn)
                .await?
        }
        None => {
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(user_id)
                .execute(&mut *conn)
                .await?
        }
    };
    Ok(result.rows_affected())
}

/// CLI. Unknown username → [`ApiError::NotFound`].
pub async fn revoke_username(pool: &SqlitePool, username: &str) -> Result<u64, ApiError> {
    let (_, norm) = super::normalize_username(username)?;
    let user_id = sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE username_norm = ?")
        .bind(&norm)
        .fetch_optional(pool)
        .await?;
    let Some(user_id) = user_id else {
        return Err(ApiError::NotFound);
    };
    let mut conn = pool.acquire().await?;
    revoke_all(&mut conn, &user_id, None).await
}

/// CLI `--all`.
pub async fn revoke_everyone(pool: &SqlitePool) -> Result<u64, ApiError> {
    let result = sqlx::query("DELETE FROM sessions").execute(pool).await?;
    Ok(result.rows_affected())
}

/// Deletes idle and absolutely expired rows.
pub async fn prune(pool: &SqlitePool, now: i64) -> Result<u64, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    delete_expired(&mut conn, now).await
}

/// One-time: hash rows left in `sessions_legacy`, then drop that table.
/// Idempotent. Returns 0 when the table is already gone and the WAL wipe has
/// finished. A legacy row whose user is gone is skipped. `secure_delete` and
/// a truncating checkpoint keep the raw token out of the database file and
/// the WAL. A busy checkpoint is remembered in `sessions_legacy_wipe` and
/// retried on the next call. It does not fail startup.
pub async fn upgrade_legacy(pool: &SqlitePool, now: i64) -> Result<u64, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let legacy = legacy_table(&mut conn).await?;
    let wipe = wipe_pending(&mut conn).await?;
    // Nothing left to hash and the WAL wipe already finished. Skip the write
    // lock so a CLI command against a live server does not checkpoint.
    if !legacy && !wipe {
        return Ok(0);
    }
    let count = if legacy {
        // Per connection, outside the transaction. On so DROP overwrites the
        // legacy token pages. Off before this connection returns to the pool.
        sqlx::query("PRAGMA secure_delete = ON")
            .execute(&mut *conn)
            .await?;
        let result = upgrade_immediate(&mut conn, now).await;
        let off = sqlx::query("PRAGMA secure_delete = OFF")
            .execute(&mut *conn)
            .await;
        match (result, off) {
            (Ok(count), Ok(_)) => count,
            (Err(err), _) => return Err(err),
            (Ok(_), Err(err)) => return Err(err),
        }
    } else {
        0
    };
    if !checkpoint_truncate(&mut conn).await? {
        tracing::warn!("wal checkpoint busy; legacy token wipe retries on the next start");
        return Ok(count);
    }
    if wipe_pending(&mut conn).await? {
        sqlx::query("DROP TABLE sessions_legacy_wipe")
            .execute(&mut *conn)
            .await?;
    }
    Ok(count)
}

async fn legacy_table(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    table_present(conn, "sessions_legacy").await
}

/// Created in the same transaction as `DROP TABLE sessions_legacy`. Present
/// until `wal_checkpoint(TRUNCATE)` succeeds, so a busy checkpoint is retried.
async fn wipe_pending(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    table_present(conn, "sessions_legacy_wipe").await
}

async fn table_present(conn: &mut SqliteConnection, name: &str) -> Result<bool, sqlx::Error> {
    let present: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(name)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(present.is_some())
}

async fn checkpoint_truncate(conn: &mut SqliteConnection) -> Result<bool, sqlx::Error> {
    // Checkpoint cannot run inside a transaction. TRUNCATE drops WAL frames
    // that still hold the raw token. busy != 0 leaves those frames in place.
    let (busy, _, _): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut *conn)
        .await?;
    Ok(busy == 0)
}

async fn upgrade_immediate(conn: &mut SqliteConnection, now: i64) -> Result<u64, sqlx::Error> {
    // IMMEDIATE reserves the write lock before the read. A deferred
    // transaction that already read `sqlite_master` gets "database is locked"
    // at the insert, and `busy_timeout` does not apply to that failure.
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
    if !legacy_table(&mut tx).await? {
        return Ok(0);
    }
    // Orphans (a user deleted with foreign keys off) can never authenticate.
    // Inserting them rolls this transaction back on every start.
    let rows = sqlx::query_as::<_, LegacyRow>(
        "SELECT l.token, l.user_id, l.created_at
         FROM sessions_legacy l
         JOIN users ON users.id = l.user_id",
    )
    .fetch_all(&mut *tx)
    .await?;
    let count = u64::try_from(rows.len()).expect("session count fits in u64");
    let expires_at = now.saturating_add(duration_ms(ABSOLUTE));
    for row in rows {
        sqlx::query(
            "INSERT OR IGNORE INTO sessions
                (id, token_hash, user_id, created_at, last_seen_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(hash_token(&row.token))
        .bind(&row.user_id)
        .bind(row.created_at)
        .bind(now)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
    }
    // Same transaction as the drop. A crash or a busy checkpoint leaves this
    // table, and the next start retries the wipe instead of seeing no work.
    sqlx::query("CREATE TABLE sessions_legacy_wipe (id INTEGER PRIMARY KEY)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DROP TABLE sessions_legacy")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if count > 0 {
        tracing::info!(count, "hashed legacy sessions");
    }
    Ok(count)
}

async fn delete_expired(conn: &mut SqliteConnection, now: i64) -> Result<u64, sqlx::Error> {
    let idle_after = now.saturating_sub(duration_ms(IDLE));
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ? OR last_seen_at <= ?")
        .bind(now)
        .bind(idle_after)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected())
}

/// Leaves room for one insert so the account ends at [`MAX_PER_USER`].
async fn trim_user(conn: &mut SqliteConnection, user_id: &str) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await?;
    if count < MAX_PER_USER {
        return Ok(());
    }
    let extra = count - (MAX_PER_USER - 1);
    sqlx::query(
        "DELETE FROM sessions WHERE id IN (
            SELECT id FROM sessions
            WHERE user_id = ?
            ORDER BY last_seen_at ASC, created_at ASC, id ASC
            LIMIT ?
         )",
    )
    .bind(user_id)
    .bind(extra)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
struct Resolved {
    session_id: String,
    user_id: String,
    username: String,
    is_admin: bool,
    last_seen_at: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct LegacyRow {
    token: String,
    user_id: String,
    created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use sqlx::migrate::Migrate;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::path::Path;
    use std::str::FromStr;
    use tempfile::TempDir;

    /// Far enough from the epoch that `now - IDLE` stays positive.
    const T0: i64 = 2_000_000_000_000;

    async fn pool_at(path: &Path) -> SqlitePool {
        let url = format!("sqlite://{}?mode=rwc", path.join("mewo.db").display());
        let options = SqliteConnectOptions::from_str(&url)
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap()
    }

    async fn fresh() -> (TempDir, SqlitePool) {
        let tmp = TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = db::connect(&url, tmp.path()).await.unwrap();
        assert_eq!(upgrade_legacy(&pool, T0).await.unwrap(), 0);
        (tmp, pool)
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

    async fn row(pool: &SqlitePool, token: &str) -> (String, Vec<u8>, i64, i64, i64) {
        sqlx::query_as(
            "SELECT id, token_hash, created_at, last_seen_at, expires_at
             FROM sessions WHERE token_hash = ?",
        )
        .bind(hash_token(token))
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

    #[test]
    fn hash_token_is_32_bytes_and_deterministic() {
        let hash = hash_token("abc");
        assert_eq!(hash.len(), 32);
        assert_eq!(hash, hash_token("abc"));
        assert_ne!(hash, hash_token("abd"));
        assert_ne!(hash, b"abc");
    }

    #[tokio::test]
    async fn issue_stores_the_hash_and_resolve_round_trips() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "Matt").await;
        let token = issue(&pool, "user", T0).await.unwrap();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));

        let (id, hash, created_at, last_seen_at, expires_at) = row(&pool, &token).await;
        assert_eq!(hash, hash_token(&token));
        assert_ne!(hash, token.as_bytes());
        assert!(!id.contains(&token));
        assert_eq!(created_at, T0);
        assert_eq!(last_seen_at, T0);
        assert_eq!(expires_at, T0 + duration_ms(ABSOLUTE));

        let token_column: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'token'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(token_column, 0);
        let raw: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE token_hash = ?")
            .bind(token.as_bytes())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(raw, 0);

        let authed = resolve(&pool, &token, T0).await.unwrap().unwrap();
        assert_eq!(authed.user_id, "user");
        assert_eq!(authed.username, "Matt");
        assert!(!authed.is_admin);
        assert_eq!(authed.session_id, id);
        assert!(resolve(&pool, "nope", T0).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn idle_and_absolute_boundaries() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "matt").await;
        let idle = duration_ms(IDLE);
        let alive = issue(&pool, "user", T0).await.unwrap();
        let dead = issue(&pool, "user", T0).await.unwrap();
        assert!(resolve(&pool, &alive, T0 + idle - 1)
            .await
            .unwrap()
            .is_some());
        assert!(resolve(&pool, &dead, T0 + idle).await.unwrap().is_none());

        let absolute = issue(&pool, "user", T0).await.unwrap();
        let exp = T0 + duration_ms(ABSOLUTE);
        sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
            .bind(exp - 1)
            .bind(hash_token(&absolute))
            .execute(&pool)
            .await
            .unwrap();
        assert!(resolve(&pool, &absolute, exp - 1).await.unwrap().is_some());
        assert!(resolve(&pool, &absolute, exp).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn resolve_slides_idle_but_not_absolute_expiry() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "matt").await;
        let token = issue(&pool, "user", T0).await.unwrap();
        let touch = duration_ms(TOUCH_AFTER);
        let expires = T0 + duration_ms(ABSOLUTE);

        assert!(resolve(&pool, &token, T0 + touch - 1)
            .await
            .unwrap()
            .is_some());
        let (_, _, _, last_seen_at, expires_at) = row(&pool, &token).await;
        assert_eq!(last_seen_at, T0);
        assert_eq!(expires_at, expires);

        let slid = T0 + touch;
        assert!(resolve(&pool, &token, slid).await.unwrap().is_some());
        let (_, _, _, last_seen_at, expires_at) = row(&pool, &token).await;
        assert_eq!(last_seen_at, slid);
        assert_eq!(expires_at, expires);

        assert!(resolve(&pool, &token, slid + duration_ms(IDLE) - 1)
            .await
            .unwrap()
            .is_some());
        let (_, _, _, _, expires_at) = row(&pool, &token).await;
        assert_eq!(expires_at, expires);
        assert!(resolve(&pool, &token, expires).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_cap_evicts_the_stalest_session_for_that_user() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "matt").await;
        user(&pool, "other", "ada").await;
        let mut tokens = Vec::new();
        for i in 0..MAX_PER_USER {
            tokens.push(issue(&pool, "user", T0 + i).await.unwrap());
        }
        sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
            .bind(T0 - 10)
            .bind(hash_token(&tokens[19]))
            .execute(&pool)
            .await
            .unwrap();
        let ada = issue(&pool, "other", T0 - 10).await.unwrap();
        let extra = issue(&pool, "user", T0 + 100).await.unwrap();

        assert!(resolve(&pool, &tokens[19], T0 + 100)
            .await
            .unwrap()
            .is_none());
        assert!(resolve(&pool, &tokens[0], T0 + 100)
            .await
            .unwrap()
            .is_some());
        assert!(resolve(&pool, &extra, T0 + 100).await.unwrap().is_some());
        assert!(resolve(&pool, &ada, T0 + 100).await.unwrap().is_some());
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = 'user'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, MAX_PER_USER);
    }

    #[tokio::test]
    async fn issue_prunes_before_it_counts_the_cap() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "matt").await;
        user(&pool, "other", "ada").await;
        let ada = issue(&pool, "other", T0).await.unwrap();
        let mut expired = Vec::new();
        for i in 0..MAX_PER_USER {
            expired.push(issue(&pool, "user", T0 + i).await.unwrap());
        }
        let cutoff = T0 + MAX_PER_USER;
        for token in &expired {
            sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
                .bind(cutoff)
                .bind(hash_token(token))
                .execute(&pool)
                .await
                .unwrap();
        }
        let fresh_token = issue(&pool, "user", cutoff).await.unwrap();
        assert!(resolve(&pool, &fresh_token, cutoff)
            .await
            .unwrap()
            .is_some());
        assert!(resolve(&pool, &ada, cutoff).await.unwrap().is_some());
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = 'user'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }

    #[tokio::test]
    async fn prune_removes_idle_and_absolute_rows() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "matt").await;
        let idle = issue(&pool, "user", T0).await.unwrap();
        let absolute = issue(&pool, "user", T0).await.unwrap();
        let kept = issue(&pool, "user", T0).await.unwrap();
        let idle_ms = duration_ms(IDLE);
        sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
            .bind(T0 - idle_ms)
            .bind(hash_token(&idle))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
            .bind(T0)
            .bind(hash_token(&absolute))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE token_hash = ?")
            .bind(T0 - idle_ms + 1)
            .bind(T0 + 1)
            .bind(hash_token(&kept))
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(prune(&pool, T0).await.unwrap(), 2);
        assert!(resolve(&pool, &idle, T0).await.unwrap().is_none());
        assert!(resolve(&pool, &absolute, T0).await.unwrap().is_none());
        assert!(resolve(&pool, &kept, T0).await.unwrap().is_some());
        assert_eq!(prune(&pool, T0).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn revoke_is_idempotent_and_revoke_all_can_keep_one() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "user", "Matt").await;
        user(&pool, "other", "ada").await;
        let first = issue(&pool, "user", T0).await.unwrap();
        let second = issue(&pool, "user", T0 + 1).await.unwrap();
        let third = issue(&pool, "user", T0 + 2).await.unwrap();
        let ada = issue(&pool, "other", T0).await.unwrap();
        let keep = resolve(&pool, &second, T0 + 2).await.unwrap().unwrap();

        let mut tx = pool.begin().await.unwrap();
        let removed = revoke_all(&mut tx, "user", Some(&keep.session_id))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(removed, 2);
        assert!(resolve(&pool, &first, T0 + 2).await.unwrap().is_none());
        assert!(resolve(&pool, &third, T0 + 2).await.unwrap().is_none());
        assert!(resolve(&pool, &second, T0 + 2).await.unwrap().is_some());
        assert!(resolve(&pool, &ada, T0 + 2).await.unwrap().is_some());

        let removed = revoke_username(&pool, "MATT").await.unwrap();
        assert_eq!(removed, 1);
        assert!(resolve(&pool, &second, T0 + 2).await.unwrap().is_none());
        assert!(resolve(&pool, &ada, T0 + 2).await.unwrap().is_some());
        let err = revoke_username(&pool, "nobody").await.unwrap_err();
        assert!(matches!(err, ApiError::NotFound));

        revoke(&pool, &keep.session_id).await.unwrap();
        revoke(&pool, &keep.session_id).await.unwrap();
        revoke(&pool, "missing").await.unwrap();

        let again = issue(&pool, "user", T0 + 3).await.unwrap();
        let ada_again = issue(&pool, "other", T0 + 3).await.unwrap();
        assert_eq!(revoke_everyone(&pool).await.unwrap(), 3);
        assert!(resolve(&pool, &again, T0 + 3).await.unwrap().is_none());
        assert!(resolve(&pool, &ada, T0 + 3).await.unwrap().is_none());
        assert!(resolve(&pool, &ada_again, T0 + 3).await.unwrap().is_none());
        assert_eq!(revoke_everyone(&pool).await.unwrap(), 0);
    }

    /// Login verifies, then a password change commits, then login inserts.
    /// The insert has to see the new hash and refuse. Otherwise the old
    /// password's session outlives the revoke.
    #[tokio::test]
    async fn a_login_verified_against_the_old_hash_issues_no_session_after_a_password_change() {
        let (_tmp, pool) = fresh().await;
        let passwords = crate::auth::Passwords::new(&crate::config::AuthConfig::default());
        let old_hash = passwords.hash("password1").await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES ('user', 'matt', 'matt', ?, 1)",
        )
        .bind(&old_hash)
        .execute(&pool)
        .await
        .unwrap();
        let kept = issue(&pool, "user", T0).await.unwrap();
        let kept_id = resolve(&pool, &kept, T0).await.unwrap().unwrap().session_id;

        let ok = passwords
            .verify_login("password1", Some(&old_hash))
            .await
            .unwrap();
        assert!(ok);

        let new_hash = passwords.hash("password2").await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = 'user'")
            .bind(&new_hash)
            .execute(&mut *tx)
            .await
            .unwrap();
        let removed = revoke_all(&mut tx, "user", Some(&kept_id)).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(removed, 0);

        let err = issue_for_login(&pool, "user", T0 + 1, &old_hash)
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::Unauthorized(_)));
        assert_eq!(err.to_string(), "Wrong username or password.");
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
        assert!(resolve(&pool, &kept, T0 + 1).await.unwrap().is_some());

        let fresh = issue_for_login(&pool, "user", T0 + 1, &new_hash)
            .await
            .unwrap();
        assert!(resolve(&pool, &fresh, T0 + 1).await.unwrap().is_some());
    }

    /// Password change verifies, then another change commits, then the first
    /// change writes. The write has to see the new hash and the revoked
    /// session and refuse. Otherwise the old password replaces the new one.
    #[tokio::test]
    async fn a_password_change_verified_against_the_old_hash_does_not_overwrite() {
        let (_tmp, pool) = fresh().await;
        let passwords = crate::auth::Passwords::new(&crate::config::AuthConfig::default());
        let old_hash = passwords.hash("password1").await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES ('user', 'matt', 'matt', ?, 1)",
        )
        .bind(&old_hash)
        .execute(&pool)
        .await
        .unwrap();
        let victim = issue(&pool, "user", T0).await.unwrap();
        let victim_id = resolve(&pool, &victim, T0)
            .await
            .unwrap()
            .unwrap()
            .session_id;
        let attacker = issue(&pool, "user", T0).await.unwrap();
        let attacker_id = resolve(&pool, &attacker, T0)
            .await
            .unwrap()
            .unwrap()
            .session_id;
        let ok = passwords
            .verify_login("password1", Some(&old_hash))
            .await
            .unwrap();
        assert!(ok);

        let victim_hash = passwords.hash("password2").await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = 'user'")
            .bind(&victim_hash)
            .execute(&mut *tx)
            .await
            .unwrap();
        revoke_all(&mut tx, "user", Some(&victim_id)).await.unwrap();
        tx.commit().await.unwrap();
        assert!(resolve(&pool, &attacker, T0).await.unwrap().is_none());

        let attacker_hash = passwords.hash("password3").await.unwrap();
        let err = commit_password_change(&pool, "user", &attacker_id, &old_hash, &attacker_hash)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Sign in first.");
        let stored: String =
            sqlx::query_scalar("SELECT password_hash FROM users WHERE id = 'user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, victim_hash);
        assert!(resolve(&pool, &victim, T0).await.unwrap().is_some());
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);

        let again = passwords.hash("password4").await.unwrap();
        let err = commit_password_change(&pool, "user", &victim_id, &old_hash, &again)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Wrong password.");
        let stored: String =
            sqlx::query_scalar("SELECT password_hash FROM users WHERE id = 'user'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, victim_hash);
        assert!(resolve(&pool, &victim, T0).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn upgrade_legacy_keeps_the_bearer_token_and_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let pool = pool_at(tmp.path()).await;
        let migrator = sqlx::migrate!("./migrations");
        let mut conn = pool.acquire().await.unwrap();
        Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
        for migration in migrator.iter() {
            if migration.version < 9 {
                Migrate::apply(&mut *conn, migration).await.unwrap();
            }
        }
        drop(conn);

        user(&pool, "user", "matt").await;
        sqlx::query(
            "INSERT INTO sessions (token, user_id, created_at) VALUES ('legacy-token', 'user', 50)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (token, user_id, created_at) VALUES ('kept-token', 'user', 60)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        assert!(table_exists(&pool, "sessions_legacy").await);

        sqlx::query(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_seen_at, expires_at)
             VALUES ('pre', ?, 'user', 7, 8, 9)",
        )
        .bind(hash_token("other-token"))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_seen_at, expires_at)
             VALUES ('collision', ?, 'user', 11, 12, 13)",
        )
        .bind(hash_token("kept-token"))
        .execute(&pool)
        .await
        .unwrap();

        let now = T0;
        assert_eq!(upgrade_legacy(&pool, now).await.unwrap(), 2);
        assert_eq!(upgrade_legacy(&pool, now + 1).await.unwrap(), 0);
        assert!(!table_exists(&pool, "sessions_legacy").await);

        let authed = resolve(&pool, "legacy-token", now).await.unwrap().unwrap();
        assert_eq!(authed.username, "matt");
        let (_, hash, created_at, last_seen_at, expires_at) = row(&pool, "legacy-token").await;
        assert_eq!(hash, hash_token("legacy-token"));
        assert_eq!(created_at, 50);
        assert_eq!(last_seen_at, now);
        assert_eq!(expires_at, now + duration_ms(ABSOLUTE));

        let untouched: (i64, i64, i64) = sqlx::query_as(
            "SELECT created_at, last_seen_at, expires_at FROM sessions WHERE id = 'pre'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(untouched, (7, 8, 9));
        let collision: (i64, i64, i64) = sqlx::query_as(
            "SELECT created_at, last_seen_at, expires_at FROM sessions WHERE id = 'collision'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(collision, (11, 12, 13));
        // The pre-existing rows keep their own windows. A hash collision does
        // not refresh the row upgrade found already there.
        assert!(resolve(&pool, "kept-token", now).await.unwrap().is_none());
        assert!(resolve(&pool, "kept-token", 12).await.unwrap().is_some());
        assert!(resolve(&pool, "other-token", 8).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn upgrade_legacy_on_a_fresh_database_drops_the_empty_table() {
        let tmp = TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = db::connect(&url, tmp.path()).await.unwrap();
        assert!(table_exists(&pool, "sessions_legacy").await);
        assert_eq!(upgrade_legacy(&pool, T0).await.unwrap(), 0);
        assert!(!table_exists(&pool, "sessions_legacy").await);
        assert_eq!(upgrade_legacy(&pool, T0 + 1).await.unwrap(), 0);
    }

    /// 009 renames `sessions` and 010 adds the reparse columns. A new database
    /// and one that stopped at 008 have never had 010. The runner applies 009,
    /// then 010.
    #[tokio::test]
    async fn migration_009_runs_before_010_on_a_database_that_never_had_it() {
        let migrator = sqlx::migrate!("./migrations");
        let listed: Vec<(i64, &str)> = migrator
            .iter()
            .map(|migration| (migration.version, migration.description.as_ref()))
            .collect();
        assert!(listed.windows(2).all(|pair| pair[0].0 < pair[1].0));
        let nine = listed.iter().position(|row| row.0 == 9).unwrap();
        let ten = listed.iter().position(|row| row.0 == 10).unwrap();
        assert!(nine < ten);
        assert_eq!(listed[nine].1, "session hardening");
        assert_eq!(listed[ten].1, "reparse");

        let tmp = TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let fresh = db::connect(&url, tmp.path()).await.unwrap();
        assert_session_then_reparse(&fresh).await;

        let tmp = TempDir::new().unwrap();
        let stopped = wal_pool(tmp.path()).await;
        apply_before_009(&stopped).await;
        let applied: Vec<i64> =
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&stopped)
                .await
                .unwrap();
        assert_eq!(applied.last().copied(), Some(8));
        assert!(!applied.contains(&9));
        assert!(!applied.contains(&10));
        sqlx::migrate!("./migrations").run(&stopped).await.unwrap();
        assert_session_then_reparse(&stopped).await;
    }

    async fn assert_session_then_reparse(pool: &SqlitePool) {
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT version, description, success FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        let nine = rows.iter().find(|row| row.0 == 9).expect("009 applied");
        let ten = rows.iter().find(|row| row.0 == 10).expect("010 applied");
        assert_eq!(nine.1, "session hardening");
        assert_eq!(ten.1, "reparse");
        assert_eq!(nine.2, 1);
        assert_eq!(ten.2, 1);
        assert!(rows.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(table_exists(pool, "sessions_legacy").await);
        let token_hash: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'token_hash'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(token_hash, 1);
        let old_token: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'token'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(old_token, 0);
        let parser_rev: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('books') WHERE name = 'parser_rev'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let feed_rev: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('books') WHERE name = 'feed_rev'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!((parser_rev, feed_rev), (1, 1));
    }

    async fn wal_pool(path: &Path) -> SqlitePool {
        wal_pool_timeout(path, db::BUSY_TIMEOUT).await
    }

    async fn wal_pool_timeout(path: &Path, busy: std::time::Duration) -> SqlitePool {
        let url = format!("sqlite://{}?mode=rwc", path.join("mewo.db").display());
        let options = SqliteConnectOptions::from_str(&url)
            .unwrap()
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(busy);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap()
    }

    async fn apply_before_009(pool: &SqlitePool) {
        let migrator = sqlx::migrate!("./migrations");
        let mut conn = pool.acquire().await.unwrap();
        Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
        for migration in migrator.iter() {
            if migration.version < 9 {
                Migrate::apply(&mut *conn, migration).await.unwrap();
            }
        }
    }

    /// A user deleted from the sqlite3 shell (foreign keys off) leaves a session
    /// row. Copying that row fails the foreign key and rolls the upgrade back,
    /// so the next start fails the same way.
    #[tokio::test]
    async fn upgrade_legacy_skips_an_orphaned_session() {
        let tmp = TempDir::new().unwrap();
        let pool = wal_pool(tmp.path()).await;
        apply_before_009(&pool).await;
        user(&pool, "user", "matt").await;
        user(&pool, "ghost", "ghost").await;
        sqlx::query(
            "INSERT INTO sessions (token, user_id, created_at) VALUES ('kept-legacy', 'user', 50)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (token, user_id, created_at)
             VALUES ('orphan-legacy', 'ghost', 51)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = 'ghost'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        let orphans: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = 'ghost'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(orphans, 1, "the orphan row has to survive the delete");
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_eq!(upgrade_legacy(&pool, T0).await.unwrap(), 1);
        assert!(!table_exists(&pool, "sessions_legacy").await);
        assert!(resolve(&pool, "kept-legacy", T0).await.unwrap().is_some());
        let ghost: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = 'ghost'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(ghost, 0);
        assert!(resolve(&pool, "orphan-legacy", T0).await.unwrap().is_none());
        assert_eq!(upgrade_legacy(&pool, T0 + 1).await.unwrap(), 0);
        drop(pool);
        let db_path = tmp.path().join("mewo.db");
        let wal_path = tmp.path().join("mewo.db-wal");
        assert!(!bytes_contain(&db_path, "orphan-legacy"));
        assert!(!bytes_contain(&wal_path, "orphan-legacy"));
        assert!(!bytes_contain(&db_path, "kept-legacy"));
        assert!(!bytes_contain(&wal_path, "kept-legacy"));
    }

    #[tokio::test]
    async fn upgrade_legacy_erases_the_raw_token_from_the_database_and_wal() {
        let tmp = TempDir::new().unwrap();
        let pool = wal_pool(tmp.path()).await;
        apply_before_009(&pool).await;
        user(&pool, "user", "matt").await;
        const TOKEN: &str = "legacy-raw-token-9f3c1a7e5b284d60a1c8e4f27b903d15";
        sqlx::query("INSERT INTO sessions (token, user_id, created_at) VALUES (?, 'user', 50)")
            .bind(TOKEN)
            .execute(&pool)
            .await
            .unwrap();
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_eq!(upgrade_legacy(&pool, T0).await.unwrap(), 1);
        assert!(resolve(&pool, TOKEN, T0).await.unwrap().is_some());
        let secure: i64 = sqlx::query_scalar("PRAGMA secure_delete")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(secure, 0, "secure_delete must not stay on for the pool");
        drop(pool);

        let db_path = tmp.path().join("mewo.db");
        let wal_path = tmp.path().join("mewo.db-wal");
        let shm_path = tmp.path().join("mewo.db-shm");
        assert!(
            !bytes_contain(&db_path, TOKEN),
            "raw token still in {}",
            db_path.display()
        );
        assert!(
            !bytes_contain(&wal_path, TOKEN),
            "raw token still in {}",
            wal_path.display()
        );
        assert!(
            !bytes_contain(&shm_path, TOKEN),
            "raw token still in {}",
            shm_path.display()
        );
    }

    /// A reader blocks `wal_checkpoint(TRUNCATE)` after the legacy table is
    /// dropped. The next start has to finish the wipe. A busy checkpoint must
    /// not fail the call, or the process crash-loops with the token still in
    /// the WAL.
    #[tokio::test]
    async fn a_busy_checkpoint_is_retried_until_the_raw_token_is_gone() {
        let tmp = TempDir::new().unwrap();
        let pool = wal_pool_timeout(tmp.path(), std::time::Duration::from_millis(50)).await;
        apply_before_009(&pool).await;
        user(&pool, "user", "matt").await;
        const TOKEN: &str = "legacy-raw-token-busy-ckpt-7c1e9a04b6d258f3e0a1";
        sqlx::query("INSERT INTO sessions (token, user_id, created_at) VALUES (?, 'user', 50)")
            .bind(TOKEN)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let reader = wal_pool_timeout(tmp.path(), std::time::Duration::from_millis(50)).await;
        let mut held = reader.begin().await.unwrap();
        let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions_legacy")
            .fetch_one(&mut *held)
            .await
            .unwrap();

        let first = upgrade_legacy(&pool, T0).await.unwrap();
        assert_eq!(first, 1);
        assert!(table_exists(&pool, "sessions_legacy_wipe").await);
        let wal_path = tmp.path().join("mewo.db-wal");
        assert!(
            bytes_contain(&wal_path, TOKEN),
            "the blocked checkpoint has to leave the token in the wal"
        );

        drop(held);
        drop(reader);
        assert_eq!(upgrade_legacy(&pool, T0 + 1).await.unwrap(), 0);
        assert!(!table_exists(&pool, "sessions_legacy_wipe").await);
        assert!(resolve(&pool, TOKEN, T0).await.unwrap().is_some());
        drop(pool);

        let db_path = tmp.path().join("mewo.db");
        assert!(
            !bytes_contain(&db_path, TOKEN),
            "raw token still in the database"
        );
        assert!(
            !bytes_contain(&wal_path, TOKEN),
            "raw token still in the wal"
        );
    }

    fn bytes_contain(path: &Path, needle: &str) -> bool {
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        let needle = needle.as_bytes();
        !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle)
    }

    #[tokio::test]
    async fn prepare_prunes_expired_sessions_at_startup() {
        let tmp = TempDir::new().unwrap();
        let config = crate::config::Config {
            listen: "127.0.0.1:0".into(),
            data_dir: tmp.path().to_path_buf(),
            web_dir: None,
            allow_signup: false,
            max_epub_bytes: 1024 * 1024,
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
            auth: crate::config::AuthConfig::default(),
            limits: crate::config::LimitsConfig {
                min_free_bytes: 0,
                ..crate::config::LimitsConfig::default()
            },
        };
        let state = crate::prepare(config.clone()).await.unwrap();
        user(&state.pool, "user", "matt").await;
        let expired = issue(&state.pool, "user", T0).await.unwrap();
        let live = issue(&state.pool, "user", db::now_ms()).await.unwrap();
        // After both issues. `issue` itself deletes expired rows, so this
        // update is what the next startup has to prune.
        sqlx::query("UPDATE sessions SET expires_at = 1 WHERE token_hash = ?")
            .bind(hash_token(&expired))
            .execute(&state.pool)
            .await
            .unwrap();
        drop(state);

        let state = crate::prepare(config).await.unwrap();
        let now = db::now_ms();
        assert!(resolve(&state.pool, &expired, now).await.unwrap().is_none());
        assert!(resolve(&state.pool, &live, now).await.unwrap().is_some());
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }
}
