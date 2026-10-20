use crate::config::{Config, DEFAULT_MAX_EPUB_BYTES, MAX_EPUB_BYTES_CEILING, MIN_EPUB_BYTES};
use crate::db::now_ms;
use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashSet};

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
    /// Master switch for catalog search and requests. Default false.
    #[serde(default = "default_requests_enabled")]
    pub requests_enabled: bool,
    /// "" = not configured. http/https only, no userinfo/query/fragment, ≤ 512 chars.
    #[serde(default = "default_url")]
    pub prowlarr_url: String,
    #[serde(default = "default_url")]
    pub sabnzbd_url: String,
    /// SAB category; its folder under complete_dir must be the MEWO_DOWNLOADS_DIR mount.
    #[serde(default = "default_sabnzbd_category")]
    pub sabnzbd_category: String,
}

fn default_requests_enabled() -> bool {
    false
}

fn default_url() -> String {
    String::new()
}

fn default_sabnzbd_category() -> String {
    "books".into()
}

/// Credentials an admin can set but never read back. The variants are the only
/// list of secret keys: PATCH accepts their camelCase names, GET reports
/// `secrets: { name: is_set }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Secret {
    ProwlarrApiKey,
    SabnzbdApiKey,
}

impl Secret {
    pub const ALL: [Secret; 2] = [Secret::ProwlarrApiKey, Secret::SabnzbdApiKey];

    pub fn key(self) -> &'static str {
        match self {
            Secret::ProwlarrApiKey => "prowlarrApiKey",
            Secret::SabnzbdApiKey => "sabnzbdApiKey",
        }
    }
}

/// Wire type for GET/PATCH /v1/admin/settings. Settings flattened, plus which
/// secrets are set. No secret value is reachable from this type.
///
/// `downloads_dir` is the `MEWO_DOWNLOADS_DIR` path. It is not stored and PATCH
/// rejects it: a writable path would let an admin point the importer at any
/// directory. The handler fills it in after load/update.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminSettings {
    #[serde(flatten)]
    pub settings: Settings,
    pub secrets: BTreeMap<&'static str, bool>,
    pub downloads_dir: Option<String>,
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
            requests_enabled: default_requests_enabled(),
            prowlarr_url: default_url(),
            sabnzbd_url: default_url(),
            sabnzbd_category: default_sabnzbd_category(),
        }
    }
}

impl Settings {
    /// First-boot values from env (`Config`). Unset env gives today's defaults.
    pub fn seed(config: &Config) -> Self {
        Self {
            allow_signup: config.allow_signup,
            max_epub_bytes: config.max_epub_bytes,
            ..Self::default()
        }
    }

    /// The message names the camelCase key and the bound in megabytes, e.g.
    /// "maxEpubBytes must be 1 to 1024 MB."
    fn validate(&self) -> Result<(), ApiError> {
        if self.max_epub_bytes < MIN_EPUB_BYTES || self.max_epub_bytes > MAX_EPUB_BYTES_CEILING {
            return Err(ApiError::BadRequest(max_epub_message()));
        }
        check_base("prowlarrUrl", &self.prowlarr_url)?;
        check_base("sabnzbdUrl", &self.sabnzbd_url)?;
        check_category(&self.sabnzbd_category)?;
        Ok(())
    }
}

fn check_base(key: &str, raw: &str) -> Result<(), ApiError> {
    if raw.is_empty() {
        return Ok(());
    }
    if raw.chars().count() > 512 {
        return Err(ApiError::BadRequest(format!(
            "{key} must be 512 characters or fewer."
        )));
    }
    if crate::requests::http::base_url(raw).is_err() {
        return Err(ApiError::BadRequest(format!(
            "{key} must be an http or https URL."
        )));
    }
    Ok(())
}

fn check_category(raw: &str) -> Result<(), ApiError> {
    let ok = (1..=32).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "sabnzbdCategory must be 1 to 32 of a-z, 0-9, \"_\" or \"-\".".into(),
        ))
    }
}

fn max_epub_message() -> String {
    let mb = 1024 * 1024;
    format!(
        "maxEpubBytes must be {} to {} MB.",
        MIN_EPUB_BYTES / mb,
        MAX_EPUB_BYTES_CEILING / mb
    )
}

fn clamp_epub_bytes(bytes: u64) -> u64 {
    bytes.clamp(MIN_EPUB_BYTES, MAX_EPUB_BYTES_CEILING)
}

fn clamp_seed_line(asked: u64, stored: u64) -> String {
    format!("MEWO_MAX_EPUB_BYTES is {asked}; the EPUB size limit is clamped to {stored}.")
}

/// Inserts the row from env seeds if it's missing (`INSERT OR IGNORE`). If the row
/// already exists and an env var is set to something different, logs one warn
/// line naming the var. Running it twice changes nothing.
pub async fn seed(pool: &SqlitePool, config: &Config) -> Result<(), sqlx::Error> {
    let raw = Settings::seed(config);
    let seeded = Settings {
        max_epub_bytes: clamp_epub_bytes(raw.max_epub_bytes),
        ..raw.clone()
    };
    let doc = serde_json::to_string(&seeded).map_err(json_error)?;
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO settings (id, doc, updated_at, updated_by) VALUES (1, ?, ?, NULL)",
    )
    .bind(&doc)
    .bind(now_ms())
    .execute(pool)
    .await?;
    if inserted.rows_affected() > 0 {
        if raw.max_epub_bytes != seeded.max_epub_bytes {
            tracing::warn!(
                "{}",
                clamp_seed_line(raw.max_epub_bytes, seeded.max_epub_bytes)
            );
        }
        return Ok(());
    }
    let stored_doc: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
        .fetch_one(pool)
        .await?;
    // Compare the raw env seed. A later boot with an out-of-range value still
    // disagrees with the clamped row and logs the ignored-env line.
    let stored = decode_settings(&stored_doc).map_err(json_error)?;
    for line in ignored_env_lines(
        &stored,
        &raw,
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
        Some(doc) => {
            let mut settings = decode_settings(&doc)?;
            // A row written before the bounds existed must not make every upload
            // or the admin form use 0 or 5 GiB. The row itself is healed on the
            // next patch that does not set maxEpubBytes.
            settings.max_epub_bytes = clamp_epub_bytes(settings.max_epub_bytes);
            Ok(settings)
        }
        None => Ok(Settings::default()),
    }
}

/// JSON merge patch over the stored doc and the secrets column, in one
/// transaction. Settings keys merge into `doc` as before. Secret keys (a string
/// of at most 256 characters, or null to clear) merge into `secrets`. An unknown
/// key is 400 and names neither set. An empty patch is a no-op that still
/// returns the current settings (an out-of-range max is clamped in the response
/// and not written).
///
/// `BEGIN IMMEDIATE` takes the write lock up front. A deferred transaction can
/// lose the upgrade to a concurrent CLI write and surface as 500.
pub async fn update(
    pool: &SqlitePool,
    patch: Map<String, Value>,
    by_user_id: &str,
) -> Result<AdminSettings, ApiError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let doc: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    let secrets_raw: String = sqlx::query_scalar("SELECT secrets FROM settings WHERE id = 1")
        .fetch_one(&mut *tx)
        .await?;
    if patch.is_empty() {
        let mut settings = decode_settings(&doc)?;
        settings.max_epub_bytes = clamp_epub_bytes(settings.max_epub_bytes);
        return Ok(admin_settings(settings, &secrets_raw));
    }
    let (doc_patch, secret_patch) = split_patch(&patch)?;
    let (settings, next_doc) = if doc_patch.is_empty() {
        let mut settings = decode_settings(&doc)?;
        settings.max_epub_bytes = clamp_epub_bytes(settings.max_epub_bytes);
        (settings, None)
    } else {
        let (settings, next) = apply_patch(&doc, &doc_patch)?;
        (settings, Some(next))
    };
    let next_secrets = apply_secrets(&secrets_raw, &secret_patch)?;
    if next_doc.is_some() || !secret_patch.is_empty() {
        sqlx::query(
            "UPDATE settings
             SET doc = COALESCE(?, doc), secrets = ?, updated_at = ?, updated_by = ?
             WHERE id = 1",
        )
        .bind(next_doc.as_deref())
        .bind(&next_secrets)
        .bind(now_ms())
        .bind(by_user_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(admin_settings(settings, &next_secrets))
}

/// The admin wire view. `downloads_dir` is left empty; the route fills it from
/// the process env so this column never becomes a setting.
pub async fn load_admin(pool: &SqlitePool) -> Result<AdminSettings, ApiError> {
    let row =
        sqlx::query_as::<_, (String, String)>("SELECT doc, secrets FROM settings WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    let Some((doc, secrets_raw)) = row else {
        return Ok(admin_settings(Settings::default(), "{}"));
    };
    let mut settings = decode_settings(&doc)?;
    settings.max_epub_bytes = clamp_epub_bytes(settings.max_epub_bytes);
    Ok(admin_settings(settings, &secrets_raw))
}

/// For the worker and the test route only. `None` means not set.
pub(crate) async fn secret(pool: &SqlitePool, which: Secret) -> Result<Option<String>, ApiError> {
    let raw: Option<String> = sqlx::query_scalar("SELECT secrets FROM settings WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    Ok(secret_value(&raw, which))
}

fn admin_settings(settings: Settings, secrets_raw: &str) -> AdminSettings {
    AdminSettings {
        settings,
        secrets: secret_flags(secrets_raw),
        downloads_dir: None,
    }
}

fn secret_flags(raw: &str) -> BTreeMap<&'static str, bool> {
    Secret::ALL
        .into_iter()
        .map(|secret| {
            let set = secret_value(raw, secret).is_some();
            (secret.key(), set)
        })
        .collect()
}

fn secret_value(raw: &str, which: Secret) -> Option<String> {
    let Ok(value) = serde_json::from_str::<Map<String, Value>>(raw) else {
        return None;
    };
    value
        .get(which.key())
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

type PatchMaps = (Map<String, Value>, Map<String, Value>);

fn split_patch(patch: &Map<String, Value>) -> Result<PatchMaps, ApiError> {
    let known = known_keys()?;
    let mut doc = Map::new();
    let mut secrets = Map::new();
    for (key, value) in patch {
        if Secret::ALL.iter().any(|secret| secret.key() == key) {
            secrets.insert(key.clone(), value.clone());
        } else if known.contains(key) {
            doc.insert(key.clone(), value.clone());
        } else {
            return Err(ApiError::BadRequest(format!("Unknown setting \"{key}\".")));
        }
    }
    Ok((doc, secrets))
}

fn apply_secrets(raw: &str, patch: &Map<String, Value>) -> Result<String, ApiError> {
    let mut stored: Map<String, Value> = if raw.trim().is_empty() {
        Map::new()
    } else {
        serde_json::from_str(raw).map_err(|err| {
            tracing::error!(error = %err, "settings secrets are not json");
            ApiError::Internal
        })?
    };
    for (key, value) in patch {
        match value {
            Value::Null => {
                stored.remove(key);
            }
            Value::String(text) => {
                let text = text.trim();
                if text.chars().count() > 256 {
                    return Err(ApiError::BadRequest(format!("{key} is too long.")));
                }
                if text.is_empty() {
                    stored.remove(key);
                } else {
                    stored.insert(key.clone(), Value::String(text.to_string()));
                }
            }
            _ => {
                return Err(ApiError::BadRequest(format!("{key} is the wrong type.")));
            }
        }
    }
    // Drop anything that is not a known secret string, so a bad row can't grow.
    stored.retain(|key, value| {
        Secret::ALL.iter().any(|secret| secret.key() == key) && value.is_string()
    });
    serde_json::to_string(&stored).map_err(|err| {
        tracing::error!(error = %err, "settings secrets could not be encoded");
        ApiError::Internal
    })
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
    // An unrelated key must still save when a previous binary stored a max
    // outside the current bounds. Setting maxEpubBytes itself still validates.
    if !patch.contains_key("maxEpubBytes") {
        if let Some(current) = obj.get("maxEpubBytes").and_then(Value::as_u64) {
            let clamped = clamp_epub_bytes(current);
            if clamped != current {
                tracing::warn!(
                    "maxEpubBytes {current} is outside 1 to 1024 MB; clamped to {clamped}."
                );
                obj.insert("maxEpubBytes".to_string(), Value::from(clamped));
            }
        }
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
        assert_eq!(err.to_string(), "maxEpubBytes must be 1 to 1024 MB.");
    }

    #[test]
    fn clamp_seed_line_names_the_var() {
        assert_eq!(
            clamp_seed_line(0, MIN_EPUB_BYTES),
            format!(
                "MEWO_MAX_EPUB_BYTES is 0; the EPUB size limit is clamped to {MIN_EPUB_BYTES}."
            )
        );
        assert_eq!(clamp_epub_bytes(0), MIN_EPUB_BYTES);
        assert_eq!(
            clamp_epub_bytes(5 * 1024 * 1024 * 1024),
            MAX_EPUB_BYTES_CEILING
        );
        assert_eq!(
            clamp_epub_bytes(DEFAULT_MAX_EPUB_BYTES),
            DEFAULT_MAX_EPUB_BYTES
        );
    }

    #[test]
    fn ignored_env_warns_once_per_set_var_that_disagrees() {
        let stored = Settings {
            allow_signup: true,
            max_epub_bytes: DEFAULT_MAX_EPUB_BYTES,
            ..Settings::default()
        };
        let seeded = Settings {
            allow_signup: false,
            max_epub_bytes: 32 * 1024 * 1024,
            ..Settings::default()
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
        let huge = Settings {
            max_epub_bytes: 5 * 1024 * 1024 * 1024,
            ..stored.clone()
        };
        assert_eq!(
            ignored_env_lines(&stored, &huge, false, true),
            vec!["MEWO_MAX_EPUB_BYTES is ignored; the EPUB size limit is set in the web app."]
        );
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
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
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

    async fn pool() -> (tempfile::TempDir, SqlitePool) {
        let tmp = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = crate::db::connect(&url, tmp.path()).await.unwrap();
        (tmp, pool)
    }

    fn config(tmp: &std::path::Path, max_epub_bytes: u64) -> Config {
        Config {
            listen: "127.0.0.1:0".into(),
            data_dir: tmp.to_path_buf(),
            web_dir: None,
            allow_signup: true,
            max_epub_bytes,
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
        }
    }

    #[tokio::test]
    async fn seed_clamps_out_of_range_max() {
        for asked in [0, 5 * 1024 * 1024 * 1024] {
            let (tmp, pool) = pool().await;
            seed(&pool, &config(tmp.path(), asked)).await.unwrap();
            let expected = clamp_epub_bytes(asked);
            let doc: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(
                doc.contains(&format!("\"maxEpubBytes\":{expected}")),
                "{doc}"
            );
            assert!(!doc.contains(&format!("\"maxEpubBytes\":{asked}")), "{doc}");
            assert_eq!(load(&pool).await.unwrap().max_epub_bytes, expected);

            let again = config(tmp.path(), asked.saturating_add(1));
            seed(&pool, &again).await.unwrap();
            let second: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(doc, second);
        }
    }

    #[tokio::test]
    async fn out_of_range_stored_max_does_not_block_other_patches() {
        let (tmp, pool) = pool().await;
        seed(&pool, &config(tmp.path(), DEFAULT_MAX_EPUB_BYTES))
            .await
            .unwrap();
        sqlx::query("UPDATE settings SET doc = ? WHERE id = 1")
            .bind(r#"{"allowSignup":true,"maxEpubBytes":0}"#)
            .execute(&pool)
            .await
            .unwrap();

        let loaded = load(&pool).await.unwrap();
        assert_eq!(loaded.max_epub_bytes, MIN_EPUB_BYTES);
        let still: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(still.contains("\"maxEpubBytes\":0"), "{still}");

        sqlx::query(
            "INSERT INTO users (id, username, username_norm, password_hash, created_at)
             VALUES ('user', 'user', 'user', 'x', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut signup = Map::new();
        signup.insert("allowSignup".to_string(), Value::Bool(false));
        let saved = update(&pool, signup, "user").await.unwrap();
        assert!(!saved.settings.allow_signup);
        assert_eq!(saved.settings.max_epub_bytes, MIN_EPUB_BYTES);
        let healed: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            healed.contains(&format!("\"maxEpubBytes\":{MIN_EPUB_BYTES}")),
            "{healed}"
        );
        assert!(healed.contains("\"allowSignup\":false"), "{healed}");

        sqlx::query("UPDATE settings SET doc = ? WHERE id = 1")
            .bind(r#"{"allowSignup":false,"maxEpubBytes":0}"#)
            .execute(&pool)
            .await
            .unwrap();
        let mut bad = Map::new();
        bad.insert("maxEpubBytes".to_string(), Value::from(10));
        let err = update(&pool, bad, "user").await.unwrap_err();
        assert_eq!(err.to_string(), "maxEpubBytes must be 1 to 1024 MB.");
        let unchanged: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(unchanged.contains("\"maxEpubBytes\":0"), "{unchanged}");

        let empty = update(&pool, Map::new(), "user").await.unwrap();
        assert_eq!(empty.settings.max_epub_bytes, MIN_EPUB_BYTES);
        let after_empty: String = sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(after_empty, unchanged);
    }
}
