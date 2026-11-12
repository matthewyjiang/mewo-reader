//! Admin account tools: reset a password, disable, delete.
//!
//! The only writer of `users.disabled_at`, the only admin writer of
//! `users.password_hash`, and the only `DELETE FROM users`. Every change runs
//! in one `BEGIN IMMEDIATE` that also re-checks the acting admin, so two
//! admins acting on each other at once cannot both win.

use crate::admin::AdminChange;
use crate::auth::{normalize_username, session, Passwords};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::models::AccountRemoval;
use crate::{library, requests, AppState};
use rand::Rng;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

/// No `i`, `l`, `o`, `0`, or `1`, so a password read aloud is not misheard.
const TEMP_ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// 16 characters in four dash-joined groups of 4, about 79 bits.
pub fn temp_password() -> String {
    let mut rng = rand::thread_rng();
    let groups: Vec<String> = (0..4)
        .map(|_| {
            (0..4)
                .map(|_| char::from(TEMP_ALPHABET[rng.gen_range(0..TEMP_ALPHABET.len())]))
                .collect()
        })
        .collect();
    groups.join("-")
}

/// Sets a new random password and signs the account out everywhere. Returns
/// the password, which is not stored. `actor` is the admin's user id, or
/// `None` for the CLI. A disabled account stays disabled.
pub async fn reset_password(
    pool: &SqlitePool,
    passwords: &Passwords,
    actor: Option<&str>,
    username: &str,
) -> Result<String, ApiError> {
    let password = temp_password();
    let hash = passwords.hash(&password).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let target = target(
        &mut tx,
        actor,
        username,
        "Use Change password for your own account.",
    )
    .await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&hash)
        .bind(&target.id)
        .execute(&mut *tx)
        .await?;
    session::revoke_all(&mut tx, &target.id, None).await?;
    tx.commit().await?;
    Ok(password)
}

/// The only writer of `users.is_admin`. `actor` is the admin's user id, or
/// `None` for the CLI grant and revoke. Idempotent.
pub(crate) async fn set_admin(
    pool: &SqlitePool,
    actor: Option<&str>,
    username: &str,
    admin: bool,
) -> Result<AdminChange, ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let target = target(
        &mut tx,
        actor,
        username,
        "You can't change your own admin access.",
    )
    .await?;
    let changed = sqlx::query("UPDATE users SET is_admin = ? WHERE id = ? AND is_admin <> ?")
        .bind(i64::from(admin))
        .bind(&target.id)
        .bind(i64::from(admin))
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(if changed == 0 {
        AdminChange::Unchanged
    } else {
        AdminChange::Changed
    })
}

/// Turns sign-in off or back on. Disabling signs the account out everywhere
/// and keeps its books and posts. Idempotent: a second disable keeps the
/// first `disabled_at`.
pub async fn set_disabled(
    pool: &SqlitePool,
    actor: &str,
    username: &str,
    disabled: bool,
) -> Result<(), ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let target = target(
        &mut tx,
        Some(actor),
        username,
        "You can't disable your own account.",
    )
    .await?;
    if disabled {
        sqlx::query("UPDATE users SET disabled_at = COALESCE(disabled_at, ?) WHERE id = ?")
            .bind(now_ms())
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        session::revoke_all(&mut tx, &target.id, None).await?;
    } else {
        sqlx::query("UPDATE users SET disabled_at = NULL WHERE id = ?")
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// What [`remove`] would do now. Reads only.
pub async fn removal_preview(
    pool: &SqlitePool,
    actor: &str,
    username: &str,
) -> Result<AccountRemoval, ApiError> {
    let mut tx = pool.begin().await?;
    let target = target(
        &mut tx,
        Some(actor),
        username,
        "You can't delete your own account.",
    )
    .await?;
    let plan = plan(&mut tx, &target.id).await?;
    tx.rollback().await?;
    Ok(plan.summary())
}

/// Deletes the account. Its active requests hand off to followers, its holds
/// on other books are released, a book someone else also holds passes to the
/// earliest holder, and every other book it owns is deleted with its files.
/// The `users` cascade removes the rest.
pub async fn remove(
    state: &AppState,
    actor: &str,
    username: &str,
) -> Result<AccountRemoval, ApiError> {
    let now = now_ms();
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let target = target(
        &mut tx,
        Some(actor),
        username,
        "You can't delete your own account.",
    )
    .await?;
    let plan = plan(&mut tx, &target.id).await?;
    // The worker's grab only checks `state = 'requested'`, so a cancel and a
    // retry during a grab can leave a follower downloading. Make those plain
    // followers again, so the hand-offs below move them, and drop their jobs.
    let mut jobs: Vec<String> = sqlx::query_scalar(
        "SELECT download_id FROM book_requests
         WHERE state = 'downloading' AND download_id IS NOT NULL
           AND follows IN (SELECT id FROM book_requests WHERE user_id = ?)",
    )
    .bind(&target.id)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE book_requests
         SET state = 'requested', download_id = NULL, progress = 0, updated_at = ?
         WHERE state = 'downloading'
           AND follows IN (SELECT id FROM book_requests WHERE user_id = ?)",
    )
    .bind(now)
    .bind(&target.id)
    .execute(&mut *tx)
    .await?;
    // Leaders next. `follows ON DELETE SET NULL` would otherwise turn every
    // follower into a leader and break `book_requests_one_leader`.
    for request_id in &plan.leaders {
        if let Some(job) = requests::cancel_leading(&mut tx, request_id, now).await? {
            jobs.push(job);
        }
    }
    // A canceled or failed row can still have followers until the worker
    // sweeps it. The same SET NULL would make each of them a leader.
    for request_id in &plan.stranded {
        requests::adopt_followers(&mut tx, request_id, now).await?;
    }
    // A leader demoted to follower keeps its job until the worker clears it.
    // The cascade would drop the row and leave the job in SABnzbd.
    let follower_jobs: Vec<String> = sqlx::query_scalar(
        "SELECT download_id FROM book_requests
         WHERE user_id = ? AND follows IS NOT NULL AND download_id IS NOT NULL",
    )
    .bind(&target.id)
    .fetch_all(&mut *tx)
    .await?;
    jobs.extend(follower_jobs);
    for book_id in &plan.holds {
        library::release_hold(&mut tx, &target.id, book_id).await?;
    }
    let mut dirs = Vec::new();
    for book in &plan.owned {
        if let Some(holder) = &book.next_holder {
            library::pass_ownership(&mut tx, &book.id, &target.id, holder).await?;
            continue;
        }
        requests::cancel_imported(&mut *tx, &target.id, &book.id).await?;
        sqlx::query("DELETE FROM books WHERE id = ? AND user_id = ?")
            .bind(&book.id)
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        dirs.push(library::book_dir(
            &state.data_dir,
            &book.dir_owner,
            &book.id,
        ));
    }
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&target.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    // One directory per deleted book. `books/<user_id>/` also holds the files
    // of books this account passed on earlier, so it is never removed whole.
    for dir in dirs {
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
    for job in jobs {
        requests::delete_job(state, &job).await;
    }
    state.worker.notify_one();
    Ok(plan.summary())
}

#[derive(Debug, FromRow)]
struct Target {
    id: String,
}

/// Unknown → 404. The actor's own account → 409 with `self_message`. An actor
/// that is no longer an active admin → 403. Run inside the caller's
/// transaction so the check and the change see the same rows.
async fn target(
    conn: &mut SqliteConnection,
    actor: Option<&str>,
    username: &str,
    self_message: &str,
) -> Result<Target, ApiError> {
    let (_, norm) = normalize_username(username)?;
    let target = sqlx::query_as::<_, Target>("SELECT id FROM users WHERE username_norm = ?")
        .bind(&norm)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(ApiError::NotFound)?;
    let Some(actor) = actor else {
        return Ok(target);
    };
    if target.id == actor {
        return Err(ApiError::Conflict(self_message.into()));
    }
    let admin: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM users WHERE id = ? AND is_admin = 1 AND disabled_at IS NULL",
    )
    .bind(actor)
    .fetch_optional(&mut *conn)
    .await?;
    if admin.is_none() {
        return Err(ApiError::Forbidden("Admins only.".into()));
    }
    Ok(target)
}

#[derive(Debug, FromRow)]
struct OwnedBook {
    id: String,
    dir_owner: String,
    visible_to_others: bool,
    next_holder: Option<String>,
}

#[derive(Debug)]
struct RemovalPlan {
    /// Active requests no one else's row follows.
    leaders: Vec<String>,
    /// Canceled or failed requests that still have waiting followers.
    stranded: Vec<String>,
    /// Books owned by someone else that this account holds.
    holds: Vec<String>,
    owned: Vec<OwnedBook>,
}

impl RemovalPlan {
    fn summary(&self) -> AccountRemoval {
        let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
        let deleted = || self.owned.iter().filter(|book| book.next_holder.is_none());
        AccountRemoval {
            books_deleted: count(deleted().count()),
            shared_deleted: count(deleted().filter(|book| book.visible_to_others).count()),
            books_passed: count(self.owned.len() - deleted().count()),
            holds_released: count(self.holds.len()),
            requests_canceled: count(self.leaders.len()),
        }
    }
}

async fn plan(conn: &mut SqliteConnection, user_id: &str) -> Result<RemovalPlan, ApiError> {
    let leaders = sqlx::query_scalar(
        "SELECT id FROM book_requests
         WHERE user_id = ? AND follows IS NULL AND state IN ('requested', 'downloading')
         ORDER BY created_at ASC, id ASC",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let stranded = sqlx::query_scalar(
        "SELECT DISTINCT lead.id
         FROM book_requests lead
         JOIN book_requests follower ON follower.follows = lead.id
         WHERE lead.user_id = ? AND lead.state IN ('canceled', 'failed')
           AND follower.state IN ('requested', 'downloading')
         ORDER BY lead.id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let holds = sqlx::query_scalar(
        "SELECT DISTINCT r.book_id FROM book_requests r
         JOIN books b ON b.id = r.book_id
         WHERE r.user_id = ? AND r.state = 'imported' AND b.user_id <> r.user_id
         ORDER BY r.book_id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    // next_holder is the holder query in `routes::delete_book`.
    let owned = sqlx::query_as::<_, OwnedBook>(
        "SELECT b.id AS id, COALESCE(b.files_user_id, b.user_id) AS dir_owner,
                (b.shared = 1 OR b.is_sample = 1) AS visible_to_others,
                (SELECT r.user_id FROM book_requests r
                 WHERE r.book_id = b.id AND r.state = 'imported' AND r.user_id <> b.user_id
                 ORDER BY r.created_at ASC, r.id ASC
                 LIMIT 1) AS next_holder
         FROM books b
         WHERE b.user_id = ?
         ORDER BY b.imported_at ASC, b.id ASC",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(RemovalPlan {
        leaders,
        stranded,
        holds,
        owned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::check_password;
    use crate::db;
    use std::collections::HashSet;
    use tempfile::TempDir;

    async fn fresh() -> (TempDir, SqlitePool) {
        let tmp = TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = db::connect(&url, tmp.path()).await.unwrap();
        (tmp, pool)
    }

    async fn user(pool: &SqlitePool, id: &str, is_admin: bool) {
        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at, is_admin)
             VALUES (?, ?, ?, 'x', 1, ?)",
        )
        .bind(id)
        .bind(id)
        .bind(id)
        .bind(i64::from(is_admin))
        .execute(pool)
        .await
        .unwrap();
    }

    async fn check(
        pool: &SqlitePool,
        actor: Option<&str>,
        username: &str,
    ) -> Result<String, ApiError> {
        let mut conn = pool.acquire().await.unwrap();
        target(&mut conn, actor, username, "self")
            .await
            .map(|t| t.id)
    }

    #[test]
    fn temp_password_is_four_groups_from_the_alphabet_and_passes_the_rules() {
        let mut seen = HashSet::new();
        for _ in 0..200 {
            let password = temp_password();
            assert_eq!(password.len(), 19, "{password}");
            let groups: Vec<&str> = password.split('-').collect();
            assert_eq!(groups.len(), 4, "{password}");
            for group in groups {
                assert_eq!(group.len(), 4, "{password}");
                assert!(
                    group.bytes().all(|c| TEMP_ALPHABET.contains(&c)),
                    "{password}"
                );
            }
            check_password(&password).unwrap();
            assert!(seen.insert(password), "repeated a password");
        }
    }

    #[tokio::test]
    async fn set_admin_refuses_an_actor_who_is_no_longer_an_active_admin() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "matt", true).await;
        user(&pool, "bob", true).await;
        let admins = || async {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM users WHERE is_admin = 1 AND disabled_at IS NULL",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };

        assert!(matches!(
            set_admin(&pool, Some("matt"), "matt", false).await,
            Err(ApiError::Conflict(message)) if message == "You can't change your own admin access."
        ));
        sqlx::query("UPDATE users SET disabled_at = 5 WHERE id = 'matt'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            set_admin(&pool, Some("matt"), "bob", false).await,
            Err(ApiError::Forbidden(message)) if message == "Admins only."
        ));
        assert_eq!(admins().await, 1);

        assert_eq!(
            set_admin(&pool, None, "bob", true).await.unwrap(),
            AdminChange::Unchanged
        );
        assert_eq!(
            set_admin(&pool, None, "matt", false).await.unwrap(),
            AdminChange::Changed
        );
        assert!(matches!(
            set_admin(&pool, None, "nobody", true).await,
            Err(ApiError::NotFound)
        ));
    }

    #[tokio::test]
    async fn target_refuses_self_unknown_and_an_actor_who_is_no_longer_an_active_admin() {
        let (_tmp, pool) = fresh().await;
        user(&pool, "matt", true).await;
        user(&pool, "ada", false).await;

        assert_eq!(check(&pool, Some("matt"), "Ada").await.unwrap(), "ada");
        assert_eq!(check(&pool, None, "matt").await.unwrap(), "matt");
        assert!(matches!(
            check(&pool, Some("matt"), "nobody").await,
            Err(ApiError::NotFound)
        ));
        assert!(matches!(
            check(&pool, Some("matt"), "matt").await,
            Err(ApiError::Conflict(message)) if message == "self"
        ));

        sqlx::query("UPDATE users SET is_admin = 0 WHERE id = 'matt'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            check(&pool, Some("matt"), "ada").await,
            Err(ApiError::Forbidden(message)) if message == "Admins only."
        ));

        sqlx::query("UPDATE users SET is_admin = 1, disabled_at = 5 WHERE id = 'matt'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(matches!(
            check(&pool, Some("matt"), "ada").await,
            Err(ApiError::Forbidden(message)) if message == "Admins only."
        ));
    }
}
