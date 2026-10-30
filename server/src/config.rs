use std::path::PathBuf;
use std::time::Duration;

/// Bundled sample.epub is 2_886 bytes. Gutenberg War and Peace (epub3.images)
/// is 1_824_099. Illustrated Pride and Prejudice (epub3.images) is 24_835_578.
/// 64 MiB leaves room for a heavy illustrated novel and still rejects a dump.
pub const DEFAULT_MAX_EPUB_BYTES: u64 = 64 * 1024 * 1024;

/// Lowest EPUB cap an admin can save.
pub const MIN_EPUB_BYTES: u64 = 1024 * 1024;

/// Highest EPUB cap an admin can save. Also the fixed `POST /v1/books` body
/// backstop: the live envelope is [`request_body_limit`] of the settings
/// value, which is read per upload and cannot be a router layer.
pub const MAX_EPUB_BYTES_CEILING: u64 = 1024 * 1024 * 1024;

/// Free bytes that must remain after an upload or a request download.
/// `0` turns the floor off.
pub const DEFAULT_MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;

/// JSON and other non-upload routes. Larger than a comment or a settings
/// patch, and small enough that an anonymous request is not buffered to the
/// EPUB ceiling.
pub const JSON_BODY_LIMIT: usize = 1024 * 1024;

/// Phone writes cover JPEGs at 85% quality. 4 MiB is above a large page scan
/// and still a hard stop if someone posts a dump as "cover".
pub const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

/// Title, author, handle, and multipart chrome on top of the two files.
pub const MAX_FORM_OVERHEAD_BYTES: u64 = 64 * 1024;

pub fn request_body_limit(max_epub_bytes: u64) -> usize {
    max_epub_bytes
        .saturating_add(MAX_COVER_BYTES)
        .saturating_add(MAX_FORM_OVERHEAD_BYTES) as usize
}

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub data_dir: PathBuf,
    /// Unset means API only. Docker sets this to /srv/web.
    pub web_dir: Option<PathBuf>,
    /// First-boot seed. Unset env means signups are off. After the settings row
    /// exists, the database wins and a set `MEWO_ALLOW_SIGNUP` is ignored.
    pub allow_signup: bool,
    /// First-boot seed for the upload cap. After the settings row exists, the
    /// database wins and a set `MEWO_MAX_EPUB_BYTES` is ignored.
    pub max_epub_bytes: u64,
    /// Completed SABnzbd category folder. Unset means requests can be configured
    /// but the worker will not import. Not a setting: a panel field would be an
    /// arbitrary-directory read.
    pub downloads_dir: Option<PathBuf>,
    /// Open Library origin. Tests point this at a local fake.
    pub openlibrary_url: String,
    /// Password hashing cap and auth rate limits. Limits are code constants.
    /// `trusted_proxy_hops` is `MEWO_TRUSTED_PROXY_HOPS`.
    pub auth: AuthConfig,
    /// Parse concurrency and the free-disk floor. `min_free_bytes` is
    /// `MEWO_MIN_FREE_BYTES`. The parse cap and wait are code constants.
    pub limits: LimitsConfig,
}

/// How many EPUB parses may run at once, and how long a caller waits for one.
#[derive(Clone, Copy, Debug)]
pub struct LimitsConfig {
    pub max_parses: usize,
    pub parse_wait: Duration,
    pub min_free_bytes: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_parses: 2,
            parse_wait: Duration::from_secs(30),
            min_free_bytes: DEFAULT_MIN_FREE_BYTES,
        }
    }
}

/// How many attempts a bucket holds, and how often one token comes back.
#[derive(Clone, Copy, Debug)]
pub struct Rate {
    pub burst: u32,
    pub every: Duration,
}

/// Argon2 concurrency and the auth throttles. Tests build their own.
#[derive(Clone, Copy, Debug)]
pub struct AuthConfig {
    pub max_concurrent: usize,
    pub queue_wait: Duration,
    pub login_ip: Rate,
    pub login_user: Rate,
    pub register_ip: Rate,
    pub register_user: Rate,
    pub max_keys: usize,
    /// `MEWO_TRUSTED_PROXY_HOPS`. `0` ignores `X-Forwarded-For`.
    pub trusted_proxy_hops: u8,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 2,
            queue_wait: Duration::from_secs(1),
            login_ip: Rate {
                burst: 10,
                every: Duration::from_secs(30),
            },
            login_user: Rate {
                burst: 5,
                every: Duration::from_secs(60),
            },
            register_ip: Rate {
                burst: 5,
                every: Duration::from_secs(10 * 60),
            },
            register_user: Rate {
                burst: 3,
                every: Duration::from_secs(10 * 60),
            },
            max_keys: 100_000,
            trusted_proxy_hops: 0,
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir =
            PathBuf::from(std::env::var("MEWO_DATA_DIR").unwrap_or_else(|_| "data".into()));
        let allow_signup = parse_allow_signup(std::env::var("MEWO_ALLOW_SIGNUP").ok().as_deref());
        let max_epub_bytes = std::env::var("MEWO_MAX_EPUB_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_EPUB_BYTES);
        let listen = std::env::var("MEWO_LISTEN").unwrap_or_else(|_| "0.0.0.0:8787".into());
        let openlibrary_url = std::env::var("MEWO_OPENLIBRARY_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://openlibrary.org".into());
        let auth = AuthConfig {
            trusted_proxy_hops: trusted_proxy_hops(),
            ..AuthConfig::default()
        };
        Self {
            listen,
            data_dir,
            web_dir: std::env::var_os("MEWO_WEB_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            allow_signup,
            max_epub_bytes,
            downloads_dir: std::env::var_os("MEWO_DOWNLOADS_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            openlibrary_url,
            auth,
            limits: LimitsConfig {
                min_free_bytes: parse_min_free_bytes(
                    std::env::var("MEWO_MIN_FREE_BYTES").ok().as_deref(),
                ),
                ..LimitsConfig::default()
            },
        }
    }

    pub fn database_url(&self) -> String {
        if let Ok(url) = std::env::var("MEWO_DATABASE_URL") {
            return url;
        }
        let db = self.data_dir.join("mewo.db");
        format!("sqlite://{}?mode=rwc", db.display())
    }
}

/// `None` and anything other than `1`, `true`, `yes`, or `on` are off.
pub fn parse_allow_signup(raw: Option<&str>) -> bool {
    match raw {
        Some(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => false,
    }
}

/// Unset is [`DEFAULT_MIN_FREE_BYTES`]. `0` is a real value and disables the
/// floor. Anything that is not a `u64` is the default, with one warning.
pub fn parse_min_free_bytes(raw: Option<&str>) -> u64 {
    let Some(value) = raw else {
        return DEFAULT_MIN_FREE_BYTES;
    };
    match value.parse::<u64>() {
        Ok(bytes) => bytes,
        Err(_) => {
            tracing::warn!(
                "MEWO_MIN_FREE_BYTES={value} is not a number; using {DEFAULT_MIN_FREE_BYTES}"
            );
            DEFAULT_MIN_FREE_BYTES
        }
    }
}

fn trusted_proxy_hops() -> u8 {
    let Ok(value) = std::env::var("MEWO_TRUSTED_PROXY_HOPS") else {
        return 0;
    };
    match value.parse::<u8>() {
        Ok(hops) => hops,
        Err(_) => {
            tracing::warn!(
                "MEWO_TRUSTED_PROXY_HOPS={value} is not a number from 0 to 255; using 0"
            );
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signup_env_defaults_off() {
        assert!(!parse_allow_signup(None));
        for on in ["1", "true", "YES", "on"] {
            assert!(parse_allow_signup(Some(on)), "{on}");
        }
        for off in ["0", "", "off", "garbage"] {
            assert!(!parse_allow_signup(Some(off)), "{off}");
        }
    }

    #[test]
    fn min_free_env_defaults_to_one_gib() {
        assert_eq!(parse_min_free_bytes(None), DEFAULT_MIN_FREE_BYTES);
        assert_eq!(parse_min_free_bytes(Some("0")), 0);
        assert_eq!(parse_min_free_bytes(Some("abc")), DEFAULT_MIN_FREE_BYTES);
        assert_eq!(parse_min_free_bytes(Some("4096")), 4096);
    }
}
