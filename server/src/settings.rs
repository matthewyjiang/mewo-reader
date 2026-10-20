use crate::config::{Config, DEFAULT_MAX_EPUB_BYTES, MAX_EPUB_BYTES_CEILING, MIN_EPUB_BYTES};
use crate::db::now_ms;
use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use std::collections::HashSet;

/// Server-wide settings an admin edits from the web app. This struct is the
/// only list of settings. JSON keys, defaults, validation and the PATCH
/// unknown-key check all derive from it.
///
/// Adding a setting: add a field, add its default in `Default`, add its bounds
/// in `validate`. No migration, no new route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    #[serde(default = "default_allow_signup")]
    pub allow_signup: bool,
    #[serde(default = "default_max_epub_bytes")]
    pub max_epub_bytes: u64,
}

fn default_allow_signup() -> bool {
    true
}

fn default_max_epub_bytes() -> u64 {
    DEFAULT_MAX_EPUB_BYTES
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            allow_signup: default_allow_signup(),
            max_epub_bytes: default_max_epub_bytes(),
        }
    }
}

impl Settings {
    /// First-boot values from env (`Config`). Unset env gives today's defaults.
    pub fn seed(config: &Config) -> Self {
        Self {
            allow_signup: config.allow_signup,
            max_epub_bytes: config.max_epub_bytes,
        }
    }

    /// The message names the camelCase key and the bound, e.g.
    /// "maxEpubBytes must be 1048576 to 1073741824 (asked 10)."
    fn validate(&self) -> Result<(), ApiError> {
        if self.max_epub_bytes < MIN_EPUB_BYTES || self.max_epub_bytes > MAX_EPUB_BYTES_CEILING {
            return Err(ApiError::BadRequest(format!(
                "maxEpubBytes must be {MIN_EPUB_BYTES} to {MAX_EPUB_BYTES_CEILING} (asked {}).",
                self.max_epub_bytes
            )));
        }
        Ok(())
    }
}

/// Inserts the row from env seeds if it's missing (`INSERT OR IGNORE`). If the row
/// already exists and an env var is set to something different, logs one warn
/// line naming the var. Running it twice changes nothing.
pub async fn seed(pool: &SqlitePool, config: &Config) -> Result<(), sqlx::Error> {
    let seeded = Settings::seed(config);
    let doc = serde_json::to_string(&seeded).map_err(json_error)?;
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO settings (id, doc, updated_at, updated_by) VALUES (1, ?, ?, NULL)",
    )
    .bind(&doc)
    .bind(now_ms())
    .execute(pool)
    .await?;
    if inserted.rows_affected() > 0 {
        return Ok(());
    }
    let stored_doc: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await?;
    let stored = decode_settings(&stored_doc).map_err(json_error)?;
    for line in ignored_env_lines(
        &stored,
        &seeded,
        env_present("MEWO_ALLOW_SIGNUP"),
        env_present("MEWO_MAX_EPUB_BYTES"),
    ) {
        tracing::warn!("{line}");
    }
    Ok(())
}

/// The current settings. One indexed single-row read, no cache. The DB is the
/// only copy, so the web and the CLI can both write without invalidation.
/// Lenient: unknown keys (written by a newer binary) are ignored, and missing
/// keys take their default.
pub async fn load(pool: &SqlitePool) -> Result<Settings, ApiError> {
    let doc: Option<String> = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    match doc {
        Some(doc) => decode_settings(&doc),
        None => Ok(Settings::default()),
    }
}

/// JSON merge patch over the stored doc, applied in one transaction:
///   1. reject keys not in `serde_json::to_value(Settings::default())` → 400
///   2. merge into the current doc, deserialize (type errors → 400 naming the key)
///   3. validate → 400
///   4. UPDATE settings SET doc, updated_at, updated_by WHERE id = 1
///
/// Returns the saved Settings. An empty patch is a no-op that still returns the
/// current settings.
pub async fn update(
    pool: &SqlitePool,
    patch: Map<String, Value>,
    by_user_id: &str,
) -> Result<Settings, ApiError> {
    let mut tx = pool.begin().await?;
    let doc: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    if patch.is_empty() {
        return decode_settings(&doc);
    }
    let (settings, next) = apply_patch(&doc, &patch)?;
    sqlx::query("UPDATE settings SET doc = ?, updated_at = ?, updated_by = ? WHERE id = 1")
        .bind(&next)
        .bind(now_ms())
        .bind(by_user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(settings)
}

fn apply_patch(doc: &str, patch: &Map<String, Value>) -> Result<(Settings, String), ApiError> {
    let mut stored: Value = serde_json::from_str(doc).map_err(|err| {
        tracing::error!(error = %err, "settings doc is not json");
        ApiError::Internal
    })?;
    let known = known_keys()?;
    let obj = stored.as_object_mut().ok_or_else(|| {
        tracing::error!("settings doc is not an object");
        ApiError::Internal
    })?;
    for (key, value) in patch {
        if !known.contains(key) {
            return Err(ApiError::BadRequest(format!("Unknown setting \"{key}\".")));
        }
        if !value_matches_field(key, value) {
            return Err(ApiError::BadRequest(format!("{key} is the wrong type.")));
        }
        obj.insert(key.clone(), value.clone());
    }
    let settings = decode_value(stored.clone())?;
    settings.validate()?;
    let next = serde_json::to_string(&stored).map_err(|err| {
        tracing::error!(error = %err, "settings doc could not be encoded");
        ApiError::Internal
    })?;
    Ok((settings, next))
}

fn known_keys() -> Result<HashSet<String>, ApiError> {
    let value = serde_json::to_value(Settings::default()).map_err(|err| {
        tracing::error!(error = %err, "settings default could not be encoded");
        ApiError::Internal
    })?;
    let obj = value.as_object().ok_or(ApiError::Internal)?;
    Ok(obj.keys().cloned().collect())
}

fn value_matches_field(key: &str, value: &Value) -> bool {
    let mut probe = Map::new();
    probe.insert(key.to_string(), value.clone());
    serde_json::from_value::<Settings>(Value::Object(probe)).is_ok()
}

fn decode_settings(doc: &str) -> Result<Settings, ApiError> {
    let value: Value = serde_json::from_str(doc).map_err(|err| {
        tracing::error!(error = %err, "settings doc is not json");
        ApiError::Internal
    })?;
    decode_value(value)
}

fn decode_value(value: Value) -> Result<Settings, ApiError> {
    serde_json::from_value(value).map_err(|err| {
        tracing::error!(error = %err, "settings doc does not match Settings");
        ApiError::Internal
    })
}

fn ignored_env_lines(
    stored: &Settings,
    seeded: &Settings,
    allow_var_set: bool,
    max_var_set: bool,
) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if allow_var_set && stored.allow_signup != seeded.allow_signup {
        lines.push("MEWO_ALLOW_SIGNUP is ignored; signups are set in the web app.");
    }
    if max_var_set && stored.max_epub_bytes != seeded.max_epub_bytes {
        lines.push("MEWO_MAX_EPUB_BYTES is ignored; the EPUB size limit is set in the web app.");
    }
    lines
}

fn env_present(name: &str) -> bool {
    std::env::var(name).is_ok()
}

fn json_error(err: impl std::error::Error + Send + Sync + 'static) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_message_names_the_key() {
        let settings = Settings {
            max_epub_bytes: 10,
            ..Settings::default()
        };
        let err = settings.validate().unwrap_err();
        assert_eq!(
            err.to_string(),
            "maxEpubBytes must be 1048576 to 1073741824 (asked 10)."
        );
    }

    #[test]
    fn ignored_env_warns_once_per_set_var_that_disagrees() {
        let stored = Settings {
            allow_signup: true,
            max_epub_bytes: DEFAULT_MAX_EPUB_BYTES,
        };
        let seeded = Settings {
            allow_signup: false,
            max_epub_bytes: 32 * 1024 * 1024,
        };
        assert!(ignored_env_lines(&stored, &stored, true, true).is_empty());
        assert_eq!(
            ignored_env_lines(&stored, &seeded, true, false),
            vec!["MEWO_ALLOW_SIGNUP is ignored; signups are set in the web app."]
        );
        assert_eq!(
            ignored_env_lines(&stored, &seeded, true, true),
            vec![
                "MEWO_ALLOW_SIGNUP is ignored; signups are set in the web app.",
                "MEWO_MAX_EPUB_BYTES is ignored; the EPUB size limit is set in the web app.",
            ]
        );
        assert!(ignored_env_lines(&stored, &seeded, false, false).is_empty());
    }

    #[tokio::test]
    async fn seed_inserts_once_and_a_second_call_leaves_the_row() {
        let tmp = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = crate::db::connect(&url, tmp.path()).await.unwrap();
        let config = Config {
            listen: "127.0.0.1:0".into(),
            data_dir: tmp.path().to_path_buf(),
            web_dir: None,
            allow_signup: false,
            max_epub_bytes: 5_000_000,
        };
        seed(&pool, &config).await.unwrap();
        let first: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let loaded = load(&pool).await.unwrap();
        assert!(!loaded.allow_signup);
        assert_eq!(loaded.max_epub_bytes, 5_000_000);

        let again = Config {
            allow_signup: true,
            max_epub_bytes: DEFAULT_MAX_EPUB_BYTES,
            ..config
        };
        seed(&pool, &again).await.unwrap();
        let second: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(load(&pool).await.unwrap(), loaded);
    }
}
