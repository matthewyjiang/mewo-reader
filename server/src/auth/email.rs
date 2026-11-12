//! Account email: verify an address, reset a password by mail.
//!
//! The only writer of `users.email`, `users.email_norm` and `email_tokens`.
//! A link carries 32 random bytes as hex. The row stores `sha256(token)`, so
//! a database copy cannot open a link. One live link per account and purpose.

use super::{check_password, new_token, normalize_username, session, Authed, ClientIp};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::mail::{self, Letter};
use crate::AppState;
use lettre::message::Mailbox;
use lettre::Address;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use std::time::Duration;

/// How long a reset link works.
pub const RESET_FOR: Duration = Duration::from_secs(30 * 60);
/// How long a verify link works.
pub const VERIFY_FOR: Duration = Duration::from_secs(24 * 3600);

const MAX_EMAIL: usize = 254;
const MAX_LOGIN: usize = 320;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailChange {
    /// A verify link went out. The address lands when it is opened.
    Sent,
    /// Nothing to confirm: the address was cleared or is already this one.
    Done,
}

/// `email` is verified. `pending` waits in an unexpired verify row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountEmail {
    pub email: Option<String>,
    pub pending: Option<String>,
}

fn invalid_email() -> ApiError {
    ApiError::BadRequest("Enter a valid email.".into())
}

fn expired() -> ApiError {
    ApiError::BadRequest("This link has expired.".into())
}

fn taken() -> ApiError {
    ApiError::Conflict("That email is already in use.".into())
}

fn mail_off() -> ApiError {
    ApiError::Conflict("Mail isn't set up on this server.".into())
}

fn ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).expect("link lifetime fits in i64 milliseconds")
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

/// Trimmed address and its ASCII-lowercased key. A bare `user@host` of at
/// most 254 bytes, no display name. Anything else is 400.
pub fn normalize_email(raw: &str) -> Result<(String, String), ApiError> {
    let email = raw.trim();
    if email.is_empty() || email.len() > MAX_EMAIL {
        return Err(invalid_email());
    }
    let address: Address = email.parse().map_err(|_| invalid_email())?;
    let email = address.to_string();
    let norm = email.to_ascii_lowercase();
    Ok((email, norm))
}

fn mailbox(email: &str) -> Result<Mailbox, ApiError> {
    let address: Address = email.parse().map_err(|_| invalid_email())?;
    Ok(Mailbox::new(None, address))
}

async fn email_in_use(
    pool: &SqlitePool,
    norm: &str,
    except: Option<&str>,
) -> Result<bool, ApiError> {
    let row: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM users WHERE email_norm = ? AND id IS NOT ?")
            .bind(norm)
            .bind(except)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

#[allow(clippy::too_many_arguments)]
async fn replace_token(
    conn: &mut SqliteConnection,
    user_id: &str,
    purpose: &str,
    token: &str,
    email: &str,
    email_norm: &str,
    password_fp: Option<&[u8]>,
    lifetime: Duration,
) -> Result<(), ApiError> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO email_tokens
             (token_hash, user_id, purpose, email, email_norm, password_fp, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (user_id, purpose) DO UPDATE SET
             token_hash = excluded.token_hash,
             email = excluded.email,
             email_norm = excluded.email_norm,
             password_fp = excluded.password_fp,
             created_at = excluded.created_at,
             expires_at = excluded.expires_at",
    )
    .bind(session::hash_token(token))
    .bind(user_id)
    .bind(purpose)
    .bind(email)
    .bind(email_norm)
    .bind(password_fp)
    .bind(now)
    .bind(now.saturating_add(ms(lifetime)))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Public "Forgot password?". Charges the IP, then answers at once. The
/// lookup and the send run in the background, and every reason not to send
/// is silent, so the reply says nothing about which accounts exist.
pub async fn request_reset(state: &AppState, ip: ClientIp, login: &str) -> Result<(), ApiError> {
    state.throttle.mail_ip(ip)?;
    let login = login.trim();
    if login.is_empty() || login.chars().count() > MAX_LOGIN {
        return Err(ApiError::BadRequest("Enter your username or email.".into()));
    }
    let login = login.to_string();
    let task = state.clone();
    mail::spawn(state, async move {
        if let Err(err) = send_reset(&task, &login).await {
            tracing::warn!(kind = "reset", error = %err.brief(), "mail not sent");
        }
    });
    Ok(())
}

#[derive(FromRow)]
struct ResetTarget {
    id: String,
    username: String,
    email: Option<String>,
    email_norm: Option<String>,
    disabled: bool,
    password_hash: String,
}

async fn send_reset(state: &AppState, login: &str) -> Result<(), mail::MailError> {
    if !mail::available(state).await? {
        return Ok(());
    }
    let (column, norm) = if login.contains('@') {
        let Ok((_, norm)) = normalize_email(login) else {
            return Ok(());
        };
        ("email_norm", norm)
    } else {
        let Ok((_, norm)) = normalize_username(login) else {
            return Ok(());
        };
        ("username_norm", norm)
    };
    let target = sqlx::query_as::<_, ResetTarget>(&format!(
        "SELECT id, username, email, email_norm, disabled_at IS NOT NULL AS disabled, password_hash
         FROM users WHERE {column} = ?"
    ))
    .bind(&norm)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;
    let Some(target) = target else {
        return Ok(());
    };
    let (Some(email), Some(email_norm)) = (&target.email, &target.email_norm) else {
        return Ok(());
    };
    if target.disabled || state.throttle.mail_user(&target.id).is_err() {
        return Ok(());
    }
    let token = new_token();
    let mut conn = state.pool.acquire().await.map_err(ApiError::from)?;
    replace_token(
        &mut conn,
        &target.id,
        "reset",
        &token,
        email,
        email_norm,
        Some(&sha256(target.password_hash.as_bytes())),
        RESET_FOR,
    )
    .await?;
    drop(conn);
    let public_url = crate::settings::load(&state.pool).await?.public_url;
    let letter = mail::reset_letter(mailbox(email)?, &target.username, &public_url, &token);
    if let Err(err) = mail::send(state, letter).await {
        tracing::warn!(user = %target.id, kind = "reset", error = %err.brief(), "mail not sent");
    }
    Ok(())
}

#[derive(FromRow)]
struct ResetRow {
    user_id: String,
    email_norm: String,
    password_fp: Option<Vec<u8>>,
    expires_at: i64,
    password_hash: String,
    user_email_norm: Option<String>,
    disabled: bool,
}

/// Sets a new password from a reset link and signs the account out
/// everywhere. Issues no session: the reset page sends you to sign in.
pub async fn redeem_reset(
    state: &AppState,
    ip: ClientIp,
    token: &str,
    new_password: &str,
) -> Result<(), ApiError> {
    state.throttle.redeem(ip)?;
    check_password(new_password)?;
    let token_hash = session::hash_token(token);
    // A dead link costs no Argon2.
    let live: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM email_tokens WHERE token_hash = ? AND purpose = 'reset' AND expires_at > ?",
    )
    .bind(&token_hash)
    .bind(now_ms())
    .fetch_optional(&state.pool)
    .await?;
    if live.is_none() {
        return Err(expired());
    }
    let new_hash = state.passwords.hash(new_password).await?;
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query_as::<_, ResetRow>(
        "SELECT t.user_id, t.email_norm, t.password_fp, t.expires_at, u.password_hash,
                u.email_norm AS user_email_norm, u.disabled_at IS NOT NULL AS disabled
         FROM email_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = ? AND t.purpose = 'reset'",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(expired());
    };
    let current = row.expires_at > now_ms()
        && row.password_fp.as_deref() == Some(sha256(row.password_hash.as_bytes()).as_slice())
        && row.user_email_norm.as_deref() == Some(row.email_norm.as_str());
    if !current {
        return Err(expired());
    }
    if row.disabled {
        return Err(ApiError::Forbidden("This account is disabled.".into()));
    }
    session::replace_password(&mut tx, &row.user_id, &new_hash, None).await?;
    sqlx::query("DELETE FROM email_tokens WHERE token_hash = ?")
        .bind(&token_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Creates the verify row for `email` and returns the letter that carries
/// its link, with the row's token hash. The caller sends it now or in the
/// background.
pub(crate) async fn start_verify(
    state: &AppState,
    user_id: &str,
    email: &str,
) -> Result<(Letter, Vec<u8>), ApiError> {
    let (email, norm) = normalize_email(email)?;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&state.pool)
        .await?;
    let token = new_token();
    let mut conn = state.pool.acquire().await?;
    replace_token(
        &mut conn, user_id, "verify", &token, &email, &norm, None, VERIFY_FOR,
    )
    .await?;
    drop(conn);
    let public_url = crate::settings::load(&state.pool).await?.public_url;
    let letter = mail::verify_letter(mailbox(&email)?, &username, &public_url, &token);
    Ok((letter, session::hash_token(&token)))
}

/// Signup's optional email, checked before the account exists. Blank is no
/// email. Returns the address to verify once the account commits.
pub async fn signup_email(state: &AppState, raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(None);
    };
    if !mail::available(state).await? {
        return Err(mail_off());
    }
    let (email, norm) = normalize_email(raw)?;
    if email_in_use(&state.pool, &norm, None).await? {
        return Err(taken());
    }
    Ok(Some(email))
}

/// After signup commits. A failure here never fails the signup: the
/// account just has no pending email.
pub async fn start_signup_verify(state: &AppState, user_id: &str, email: &str) {
    let (letter, token_hash) = match start_verify(state, user_id, email).await {
        Ok(started) => started,
        Err(err) => {
            tracing::warn!(user = %user_id, kind = "verify", error = %err, "mail not sent");
            return;
        }
    };
    let task = state.clone();
    let user_id = user_id.to_string();
    mail::spawn(state, async move {
        if let Err(err) = mail::send(&task, letter).await {
            tracing::warn!(user = %user_id, kind = "verify", error = %err.brief(), "mail not sent");
            // Only this letter's row: a newer request replaced the hash.
            let dropped = sqlx::query("DELETE FROM email_tokens WHERE token_hash = ?")
                .bind(&token_hash)
                .execute(&task.pool)
                .await;
            if let Err(err) = dropped {
                tracing::warn!(user = %user_id, kind = "verify", error = %err, "pending email kept");
            }
        }
    });
}

/// Add, change or remove the signed-in account's email. Every change needs
/// the current password, checked under the login throttle.
pub async fn set_email(
    state: &AppState,
    authed: &Authed,
    ip: ClientIp,
    email: Option<&str>,
    current_password: &str,
) -> Result<EmailChange, ApiError> {
    let wanted = email.map(normalize_email).transpose()?;
    let (_, username_norm) = normalize_username(&authed.username)?;
    let attempt = state.throttle.login(ip, &username_norm)?;
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = ?")
        .bind(&authed.user_id)
        .fetch_optional(&state.pool)
        .await?;
    let Some(hash) = hash else {
        return Err(ApiError::Unauthorized("Sign in first.".into()));
    };
    let ok = match state
        .passwords
        .verify_login(current_password, Some(&hash))
        .await
    {
        Ok(ok) => ok,
        Err(ApiError::Busy) => {
            attempt.abandon();
            return Err(ApiError::Busy);
        }
        Err(err) => return Err(err),
    };
    if !ok {
        return Err(ApiError::Forbidden("Wrong password.".into()));
    }
    attempt.succeeded();
    let Some((email, norm)) = wanted else {
        let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE users SET email = NULL, email_norm = NULL WHERE id = ?")
            .bind(&authed.user_id)
            .execute(&mut *tx)
            .await?;
        clear_pending(&mut tx, &authed.user_id).await?;
        tx.commit().await?;
        return Ok(EmailChange::Done);
    };
    if !mail::available(state).await? {
        return Err(mail_off());
    }
    let current: Option<String> = sqlx::query_scalar("SELECT email_norm FROM users WHERE id = ?")
        .bind(&authed.user_id)
        .fetch_one(&state.pool)
        .await?;
    if current.as_deref() == Some(norm.as_str()) {
        let mut conn = state.pool.acquire().await?;
        clear_pending(&mut conn, &authed.user_id).await?;
        return Ok(EmailChange::Done);
    }
    if email_in_use(&state.pool, &norm, Some(&authed.user_id)).await? {
        return Err(taken());
    }
    state.throttle.mail_user(&authed.user_id)?;
    let (letter, _) = start_verify(state, &authed.user_id, &email).await?;
    if let Err(err) = mail::send(state, letter).await {
        tracing::warn!(user = %authed.user_id, kind = "verify", error = %err.brief(), "mail not sent");
        let mut conn = state.pool.acquire().await?;
        clear_pending(&mut conn, &authed.user_id).await?;
        return Err(ApiError::BadGateway("Couldn't send the email.".into()));
    }
    Ok(EmailChange::Sent)
}

async fn clear_pending(conn: &mut SqliteConnection, user_id: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM email_tokens WHERE user_id = ? AND purpose = 'verify'")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[derive(FromRow)]
struct VerifyRow {
    user_id: String,
    email: String,
    email_norm: String,
}

/// Opens a verify link: the pending address becomes the account's email.
pub async fn verify(state: &AppState, ip: ClientIp, token: &str) -> Result<String, ApiError> {
    state.throttle.redeem(ip)?;
    let token_hash = session::hash_token(token);
    let mut tx = state.pool.begin_with("BEGIN IMMEDIATE").await?;
    let row = sqlx::query_as::<_, VerifyRow>(
        "SELECT user_id, email, email_norm FROM email_tokens
         WHERE token_hash = ? AND purpose = 'verify' AND expires_at > ?",
    )
    .bind(&token_hash)
    .bind(now_ms())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(expired());
    };
    let updated = sqlx::query("UPDATE users SET email = ?, email_norm = ? WHERE id = ?")
        .bind(&row.email)
        .bind(&row.email_norm)
        .bind(&row.user_id)
        .execute(&mut *tx)
        .await;
    match updated {
        Ok(_) => {}
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => return Err(taken()),
        Err(err) => return Err(err.into()),
    }
    sqlx::query("DELETE FROM email_tokens WHERE token_hash = ?")
        .bind(&token_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(row.email)
}

/// For `/v1/me`.
pub async fn account_email(pool: &SqlitePool, user_id: &str) -> Result<AccountEmail, ApiError> {
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    let pending: Option<String> = sqlx::query_scalar(
        "SELECT email FROM email_tokens WHERE user_id = ? AND purpose = 'verify' AND expires_at > ?",
    )
    .bind(user_id)
    .bind(now_ms())
    .fetch_optional(pool)
    .await?;
    Ok(AccountEmail { email, pending })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email("  Ada@Example.COM ").unwrap(),
            ("Ada@Example.COM".to_string(), "ada@example.com".to_string())
        );
        assert_eq!(
            normalize_email("a.b+c@mail.example").unwrap().1,
            "a.b+c@mail.example"
        );
    }

    #[test]
    fn normalize_email_rejects_anything_but_a_bare_address() {
        let long = format!("{}@example.com", "a".repeat(250));
        for raw in [
            "",
            "   ",
            "ada",
            "ada@",
            "@example.com",
            "Ada <ada@example.com>",
            "ada@example.com, bob@example.com",
            "ada @example.com",
            long.as_str(),
        ] {
            let err = normalize_email(raw).unwrap_err();
            assert_eq!(err.to_string(), "Enter a valid email.", "{raw:?}");
        }
    }
}
