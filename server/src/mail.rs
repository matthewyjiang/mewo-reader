//! Outgoing mail. The only SMTP code in the server.
//!
//! The transport is built per send from the settings row, so a saved change
//! applies to the next message and nothing is cached. Logs name the user, the
//! kind of letter and the error class, never the address, the token, the link
//! or the SMTP password. lettre's `tracing` feature stays off because it logs
//! the AUTH exchange.

use crate::error::ApiError;
use crate::requests::Check;
use crate::settings::{self, Secret, SmtpTls};
use crate::AppState;
use lettre::message::header::ContentType;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::future::Future;
use std::time::Duration;
use tokio_util::task::TaskTracker;

const TIMEOUT: Duration = Duration::from_secs(15);

/// Background sends. Tests call [`Mailer::settle`] to wait for them.
#[derive(Clone, Default)]
pub struct Mailer {
    tasks: TaskTracker,
}

impl Mailer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits for every send started so far. Tests only.
    pub async fn settle(&self) {
        self.tasks.close();
        self.tasks.wait().await;
        self.tasks.reopen();
    }
}

/// One plain-text message.
#[derive(Debug, Clone)]
pub(crate) struct Letter {
    pub to: Mailbox,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MailError {
    #[error("Mail isn't set up on this server.")]
    NotConfigured,
    #[error("{0}")]
    Message(#[from] lettre::error::Error),
    #[error("{0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("{0}")]
    Api(#[from] ApiError),
}

impl MailError {
    /// For logs: the error class and SMTP code. An SMTP reply's text can
    /// echo the recipient's address, so it stays out.
    pub(crate) fn brief(&self) -> String {
        let MailError::Smtp(err) = self else {
            return self.to_string();
        };
        let class = if err.is_permanent() {
            "permanent"
        } else if err.is_transient() {
            "transient"
        } else if err.is_response() {
            "bad response"
        } else if err.is_timeout() {
            "timeout"
        } else if err.is_tls() {
            "tls"
        } else if err.is_client() {
            "client"
        } else {
            "connection"
        };
        match err.status() {
            Some(code) => format!("smtp {class} ({code})"),
            None => format!("smtp {class}"),
        }
    }
}

/// Reset and verify mail can go out: an SMTP host, a From address and a
/// Public URL are saved, and this server serves the web app the links open.
pub async fn available(state: &AppState) -> Result<bool, ApiError> {
    let settings = settings::load(&state.pool).await?;
    Ok(!settings.smtp_host.is_empty()
        && !settings.mail_from.is_empty()
        && !settings.public_url.is_empty()
        && state
            .web_dir
            .as_deref()
            .is_some_and(std::path::Path::is_dir))
}

/// Sends one letter now with the saved settings.
pub(crate) async fn send(state: &AppState, letter: Letter) -> Result<(), MailError> {
    let settings = settings::load(&state.pool).await?;
    if settings.smtp_host.is_empty() || settings.mail_from.is_empty() {
        return Err(MailError::NotConfigured);
    }
    let from: Mailbox = settings
        .mail_from
        .parse()
        .map_err(|_| MailError::NotConfigured)?;
    let message = Message::builder()
        .from(from)
        .to(letter.to)
        .subject(letter.subject)
        .header(ContentType::TEXT_PLAIN)
        .body(letter.body)?;
    let mut builder = match settings.smtp_tls {
        SmtpTls::Starttls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_host)?
        }
        SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host)?,
        SmtpTls::None => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.smtp_host)
        }
    }
    .port(settings.smtp_port)
    .timeout(Some(TIMEOUT));
    if !settings.smtp_username.is_empty() {
        let password = settings::secret(&state.pool, Secret::SmtpPassword)
            .await?
            .unwrap_or_default();
        builder = builder.credentials(Credentials::new(settings.smtp_username, password));
    }
    builder.build().send(message).await?;
    Ok(())
}

/// Runs `fut` in the background, tracked by [`Mailer`].
pub(crate) fn spawn<F>(state: &AppState, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    state.mail.tasks.spawn(fut);
}

/// `{public_url}{path}#token={token}`. The token sits after `#`, so it never
/// reaches a server log or a `Referer`.
pub(crate) fn link(public_url: &str, path: &str, token: &str) -> String {
    let base = public_url.trim().trim_end_matches('/');
    format!("{base}{path}#token={token}")
}

fn host(public_url: &str) -> String {
    reqwest::Url::parse(public_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| public_url.trim().to_string())
}

pub(crate) fn reset_letter(to: Mailbox, username: &str, public_url: &str, token: &str) -> Letter {
    Letter {
        to,
        subject: "Reset your Mewo password".into(),
        body: format!(
            "Someone asked to reset the password for @{username} on {}.\n\n\
             Open this link within 30 minutes to choose a new one:\n\n\
             {}\n\n\
             If this wasn't you, ignore this email.\n",
            host(public_url),
            link(public_url, "/reset", token),
        ),
    }
}

pub(crate) fn verify_letter(to: Mailbox, username: &str, public_url: &str, token: &str) -> Letter {
    Letter {
        to,
        subject: "Confirm your Mewo email".into(),
        body: format!(
            "Someone added this email to @{username} on {}.\n\n\
             Open this link within 24 hours to confirm it:\n\n\
             {}\n\n\
             If this wasn't you, ignore this email.\n",
            host(public_url),
            link(public_url, "/verify-email", token),
        ),
    }
}

/// Admin test. Sends now and reports `Sent` or the lettre error text. The
/// error text names the server's reply, never the password.
pub(crate) async fn test(state: &AppState, to: &str) -> Result<Check, ApiError> {
    let (email, _) = crate::auth::email::normalize_email(to)?;
    let to: Mailbox = email
        .parse()
        .map_err(|_| ApiError::BadRequest("Enter a valid email.".into()))?;
    let letter = Letter {
        to,
        subject: "Mewo test email".into(),
        body: "Mail from this Mewo server works.\n".into(),
    };
    let (ok, detail) = match send(state, letter).await {
        Ok(()) => (true, "Sent".to_string()),
        Err(MailError::Api(err)) => return Err(err),
        Err(err) => {
            tracing::warn!(kind = "test", error = %err, "mail not sent");
            (false, err.to_string())
        }
    };
    Ok(Check {
        name: "mail".into(),
        ok,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_puts_the_token_after_the_hash() {
        assert_eq!(
            link("https://mewo.example/", "/reset", "abc"),
            "https://mewo.example/reset#token=abc"
        );
        assert_eq!(
            link("https://mewo.example", "/verify-email", "abc"),
            "https://mewo.example/verify-email#token=abc"
        );
    }

    #[test]
    fn reset_letter_names_the_account_and_host() {
        let to: Mailbox = "ada@example.com".parse().unwrap();
        let letter = reset_letter(to, "ada", "https://mewo.example", "tok");
        assert_eq!(letter.subject, "Reset your Mewo password");
        assert!(letter
            .body
            .starts_with("Someone asked to reset the password for @ada on mewo.example."));
        assert!(letter
            .body
            .contains("Open this link within 30 minutes to choose a new one:"));
        assert!(letter.body.contains("https://mewo.example/reset#token=tok"));
        assert!(letter
            .body
            .contains("If this wasn't you, ignore this email."));
    }
}
