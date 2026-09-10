use std::path::PathBuf;

/// Bundled sample.epub is 2_886 bytes. Gutenberg War and Peace (epub3.images)
/// is 1_824_099. Illustrated Pride and Prejudice (epub3.images) is 24_835_578.
/// 64 MiB leaves room for a heavy illustrated novel and still rejects a dump.
pub const DEFAULT_MAX_EPUB_BYTES: u64 = 64 * 1024 * 1024;

/// Phone writes cover JPEGs at 85% quality. 4 MiB is above a large page scan
/// and still a hard stop if someone posts a dump as "cover".
pub const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

/// Title, author, handle, and multipart chrome on top of the two files.
pub const MAX_FORM_OVERHEAD_BYTES: u64 = 64 * 1024;

pub fn request_body_limit(max_epub_bytes: u64) -> usize {
    max_epub_bytes
        .saturating_add(MAX_COVER_BYTES)
        .saturating_add(MAX_FORM_OVERHEAD_BYTES)
        as usize
}

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub data_dir: PathBuf,
    pub allow_signup: bool,
    pub max_epub_bytes: u64,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = PathBuf::from(
            std::env::var("MEWO_DATA_DIR").unwrap_or_else(|_| "data".into()),
        );
        let allow_signup = match std::env::var("MEWO_ALLOW_SIGNUP") {
            Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
            Err(_) => true,
        };
        let max_epub_bytes = std::env::var("MEWO_MAX_EPUB_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_EPUB_BYTES);
        let listen = std::env::var("MEWO_LISTEN").unwrap_or_else(|_| "0.0.0.0:8787".into());
        Self {
            listen,
            data_dir,
            allow_signup,
            max_epub_bytes,
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
