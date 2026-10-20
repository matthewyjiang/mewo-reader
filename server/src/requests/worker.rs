//! One in-process pass over due book requests.
//!
//! `spawn` is started from `build`. Tests call [`tick`] / [`tick_at`] and never
//! start the loop. Each row moves at most one state per pass, and every write
//! is a compare-and-set on the state we read, so a cancel that lands mid-tick
//! wins.

use crate::db::now_ms;
use crate::error::ApiError;
use crate::library::{self, NewId, Owner};
use crate::requests::http::{base_url, UpstreamError};
use crate::requests::pick::{self, Wanted};
use crate::requests::prowlarr::Prowlarr;
use crate::requests::sabnzbd::{folder_matches, job_folder, Job, Sabnzbd};
use crate::settings::{self, Settings};
use crate::AppState;
use sqlx::SqlitePool;

macro_rules! cas {
    ($pool:expr, $sql:expr, $($bind:expr),+ $(,)?) => {{
        sqlx::query($sql)
            $(.bind($bind))+
            .execute($pool)
            .await?
            .rows_affected()
            > 0
    }};
}
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::Notify;

const RESEARCH_MS: i64 = 6 * 60 * 60 * 1000;
const GIVE_UP_MS: i64 = 14 * 24 * 60 * 60 * 1000;
const POLL_MS: i64 = 30 * 1000;
const BACKOFF_MS: i64 = 5 * 60 * 1000;
const DOWNLOAD_TIMEOUT_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_ATTEMPTS: i64 = 3;
const DUE_LIMIT: i64 = 20;

const NO_RELEASE_YET: &str = "No release yet.";
const NO_RELEASE_FOUND: &str = "No release found.";
const NO_EPUB: &str = "No EPUB in the download.";
const UNEXPECTED_FOLDER: &str = "Unexpected download folder.";
const DOWNLOAD_TIMEOUT: &str = "Download timed out.";

/// Lets a test stop the worker after the EPUB is staged and before `import`.
/// Production never installs one. The map is keyed by `data_dir` so parallel
/// tests do not share a gate.
pub struct ImportGate {
    armed: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl Default for ImportGate {
    fn default() -> Self {
        Self {
            armed: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        }
    }
}

impl ImportGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Resolves when the worker reaches the gate. A notify that lands first is
    /// kept by `Notify`, so awaiting after the worker has arrived still returns.
    pub async fn entered(&self) {
        self.entered.notified().await;
    }

    pub fn go(&self) {
        self.release.notify_one();
    }

    pub(crate) async fn wait(&self) {
        if !self.armed.swap(false, Ordering::SeqCst) {
            return;
        }
        self.entered.notify_one();
        self.release.notified().await;
    }
}

fn gates() -> &'static Mutex<HashMap<PathBuf, std::sync::Arc<ImportGate>>> {
    static GATES: OnceLock<Mutex<HashMap<PathBuf, std::sync::Arc<ImportGate>>>> = OnceLock::new();
    GATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Removes the gate for `data_dir` when dropped.
pub struct ImportHold {
    data_dir: PathBuf,
}

impl Drop for ImportHold {
    fn drop(&mut self) {
        let mut map = gates().lock().unwrap_or_else(|err| err.into_inner());
        map.remove(&self.data_dir);
    }
}

/// Installs a gate for this data directory. Tests arm it. Production does not
/// call this, so the map stays empty.
pub fn install(data_dir: &Path) -> (std::sync::Arc<ImportGate>, ImportHold) {
    let gate = std::sync::Arc::new(ImportGate::new());
    let mut map = gates().lock().unwrap_or_else(|err| err.into_inner());
    map.insert(data_dir.to_path_buf(), gate.clone());
    (
        gate,
        ImportHold {
            data_dir: data_dir.to_path_buf(),
        },
    )
}

async fn wait_import(data_dir: &Path) {
    let gate = {
        let map = gates().lock().unwrap_or_else(|err| err.into_inner());
        map.get(data_dir).cloned()
    };
    if let Some(gate) = gate {
        gate.wait().await;
    }
}

/// Loop: tick, then 30s or `worker.notify_one()` from create and retry.
/// A tick error or panic is logged. The loop does not end.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let tick_state = state.clone();
            match tokio::spawn(async move { tick(&tick_state).await }).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::error!(error = %err, "book request tick failed");
                }
                Err(err) => {
                    tracing::error!(error = %err, "book request tick panicked");
                }
            }
            tokio::select! {
                () = state.worker.notified() => {}
                () = tokio::time::sleep(Duration::from_secs(30)) => {}
            }
        }
    });
}

/// One pass over due rows. `now` is the database clock.
pub async fn tick(state: &AppState) -> Result<(), ApiError> {
    tick_at(state, now_ms()).await
}

/// [`tick`] with an injected clock. Integration tests live in another crate,
/// so the clock has to be a public argument rather than a closure inside the
/// server crate.
pub async fn tick_at(state: &AppState, now_ms: i64) -> Result<(), ApiError> {
    let Some(ready) = ready(state).await? else {
        return Ok(());
    };
    let rows = due(&state.pool, now_ms).await?;
    for row in rows {
        let request_id = row.id.clone();
        let task_state = state.clone();
        let task_ready = ready.clone();
        let joined =
            tokio::spawn(async move { step(&task_state, &task_ready, &row, now_ms).await }).await;
        report_step(&request_id, joined);
    }
    Ok(())
}

fn report_step(request_id: &str, joined: Result<Result<(), ApiError>, tokio::task::JoinError>) {
    match joined {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            tracing::error!(request_id = %request_id, error = %err, "book request step failed");
        }
        Err(err) => {
            tracing::error!(request_id = %request_id, error = %err, "book request step panicked");
        }
    }
}

#[derive(Clone)]
struct Ready {
    settings: Settings,
    prowlarr_key: String,
    sab_key: String,
}

async fn ready(state: &AppState) -> Result<Option<Ready>, ApiError> {
    let settings = settings::load(&state.pool).await?;
    if !settings.requests_enabled || state.downloads_dir.is_none() {
        return Ok(None);
    }
    if settings.prowlarr_url.trim().is_empty() || settings.sabnzbd_url.trim().is_empty() {
        return Ok(None);
    }
    let Some(prowlarr_key) =
        settings::secret(&state.pool, settings::Secret::ProwlarrApiKey).await?
    else {
        return Ok(None);
    };
    let Some(sab_key) = settings::secret(&state.pool, settings::Secret::SabnzbdApiKey).await?
    else {
        return Ok(None);
    };
    Ok(Some(Ready {
        settings,
        prowlarr_key,
        sab_key,
    }))
}

#[derive(Debug, sqlx::FromRow)]
struct Due {
    id: String,
    user_id: String,
    username: String,
    title: String,
    author: String,
    state: String,
    download_id: Option<String>,
    tried_guids: String,
    attempts: i64,
    progress: i64,
    created_at: i64,
    updated_at: i64,
}

async fn due(pool: &SqlitePool, now_ms: i64) -> Result<Vec<Due>, ApiError> {
    Ok(sqlx::query_as::<_, Due>(
        "SELECT r.id, r.user_id, u.username, r.title, r.author, r.state,
                r.download_id, r.tried_guids, r.attempts, r.progress,
                r.created_at, r.updated_at
         FROM book_requests r
         JOIN users u ON u.id = r.user_id
         WHERE r.state IN ('requested', 'downloading') AND r.next_check_at <= ?
         ORDER BY r.next_check_at ASC
         LIMIT ?",
    )
    .bind(now_ms)
    .bind(DUE_LIMIT)
    .fetch_all(pool)
    .await?)
}

struct Clients<'a> {
    prowlarr: Prowlarr<'a>,
    sab: Sabnzbd<'a>,
}

fn clients<'a>(state: &'a AppState, ready: &Ready) -> Result<Clients<'a>, UpstreamError> {
    Ok(Clients {
        prowlarr: Prowlarr {
            up: &state.upstream,
            base: base_url(&ready.settings.prowlarr_url)?,
            key: ready.prowlarr_key.clone(),
        },
        sab: Sabnzbd {
            up: &state.upstream,
            base: base_url(&ready.settings.sabnzbd_url)?,
            key: ready.sab_key.clone(),
            category: ready.settings.sabnzbd_category.clone(),
        },
    })
}

async fn step(state: &AppState, ready: &Ready, row: &Due, now_ms: i64) -> Result<(), ApiError> {
    let clients = match clients(state, ready) {
        Ok(clients) => clients,
        Err(err) => {
            tracing::error!(error = %err, "book request upstream is not usable");
            return Ok(());
        }
    };
    match row.state.as_str() {
        "requested" => requested(state, &clients, row, ready.settings.max_epub_bytes, now_ms).await,
        "downloading" => downloading(state, ready, &clients, row, now_ms).await,
        _ => Ok(()),
    }
}

async fn requested(
    state: &AppState,
    clients: &Clients<'_>,
    row: &Due,
    max_bytes: u64,
    now_ms: i64,
) -> Result<(), ApiError> {
    let job = job_name(&row.id);
    let existing = match clients.sab.find_by_name(&job).await {
        Ok(existing) => existing,
        Err(err) => return hold(&state.pool, &row.id, "requested", "SABnzbd", &err, now_ms).await,
    };
    if let Some(nzo) = existing {
        adopt(&state.pool, &row.id, &nzo, now_ms).await?;
        return Ok(());
    }

    let wanted = Wanted {
        title: &row.title,
        author: &row.author,
    };
    let releases = match clients.prowlarr.search(&pick::query(&wanted)).await {
        Ok(releases) => releases,
        Err(err) => return hold(&state.pool, &row.id, "requested", "Prowlarr", &err, now_ms).await,
    };
    let tried = guids(&row.tried_guids);
    let Some(release) = pick::pick(&wanted, &releases, &tried, max_bytes) else {
        if row.created_at < now_ms.saturating_sub(GIVE_UP_MS) {
            return fail_from(&state.pool, &row.id, "requested", NO_RELEASE_FOUND, now_ms).await;
        }
        return note(
            &state.pool,
            &row.id,
            "requested",
            NO_RELEASE_YET,
            now_ms.saturating_add(RESEARCH_MS),
        )
        .await;
    };
    let Some(url) = release.download_url.as_deref() else {
        return note(
            &state.pool,
            &row.id,
            "requested",
            NO_RELEASE_YET,
            now_ms.saturating_add(RESEARCH_MS),
        )
        .await;
    };
    let nzo = match clients.sab.add_url(url, &job).await {
        Ok(nzo) => nzo,
        Err(err) => return hold(&state.pool, &row.id, "requested", "SABnzbd", &err, now_ms).await,
    };
    let mut tried = tried;
    tried.push(release.guid.clone());
    let encoded = serde_json::to_string(&tried).unwrap_or_else(|_| "[]".into());
    let won = cas!(
        &state.pool,
        "UPDATE book_requests
         SET state = 'downloading', download_id = ?, tried_guids = ?, attempts = ?,
             release_title = ?, release_size = ?, indexer = ?, progress = 0, error = NULL,
             next_check_at = ?, updated_at = ?
         WHERE id = ? AND state = 'requested'",
        &nzo,
        &encoded,
        row.attempts.saturating_add(1),
        &release.title,
        size_i64(release.size),
        &release.indexer,
        now_ms.saturating_add(POLL_MS),
        now_ms,
        &row.id,
    );
    if !won {
        discard(&clients.sab, &nzo).await;
    }
    Ok(())
}

async fn downloading(
    state: &AppState,
    ready: &Ready,
    clients: &Clients<'_>,
    row: &Due,
    now_ms: i64,
) -> Result<(), ApiError> {
    let Some(nzo) = row.download_id.clone() else {
        return retry_or_fail(
            state,
            &clients.sab,
            row,
            "",
            &sab_error("missing"),
            None,
            now_ms,
        )
        .await;
    };
    let job = match clients.sab.job(&nzo).await {
        Ok(job) => job,
        Err(err) => {
            return hold(&state.pool, &row.id, "downloading", "SABnzbd", &err, now_ms).await
        }
    };
    match job {
        Job::Queued { progress } => {
            poll_or_time_out(state, &clients.sab, row, &nzo, i64::from(progress), now_ms).await
        }
        Job::Processing => {
            poll_or_time_out(state, &clients.sab, row, &nzo, row.progress, now_ms).await
        }
        Job::Failed { message } => {
            retry_or_fail(
                state,
                &clients.sab,
                row,
                &nzo,
                &sab_error(&message),
                None,
                now_ms,
            )
            .await
        }
        Job::Missing => {
            retry_or_fail(
                state,
                &clients.sab,
                row,
                &nzo,
                &sab_error("missing"),
                None,
                now_ms,
            )
            .await
        }
        Job::Completed { storage } => {
            completed(state, ready, &clients.sab, row, &nzo, &storage, now_ms).await
        }
    }
}

async fn poll_or_time_out(
    state: &AppState,
    sab: &Sabnzbd<'_>,
    row: &Due,
    nzo: &str,
    progress: i64,
    now_ms: i64,
) -> Result<(), ApiError> {
    if now_ms.saturating_sub(row.updated_at) > DOWNLOAD_TIMEOUT_MS {
        discard(sab, nzo).await;
        return fail_from(
            &state.pool,
            &row.id,
            "downloading",
            DOWNLOAD_TIMEOUT,
            now_ms,
        )
        .await;
    }
    let _ = cas!(
        &state.pool,
        "UPDATE book_requests
         SET progress = ?, next_check_at = ?, error = NULL
         WHERE id = ? AND state = 'downloading'",
        progress,
        now_ms.saturating_add(POLL_MS),
        &row.id,
    );
    Ok(())
}

fn sab_error(message: &str) -> String {
    format!("Download failed: {message}")
}

async fn completed(
    state: &AppState,
    ready: &Ready,
    sab: &Sabnzbd<'_>,
    row: &Due,
    nzo: &str,
    storage: &str,
    now_ms: i64,
) -> Result<(), ApiError> {
    let job = job_name(&row.id);
    let folder = job_folder(storage, &job);
    if !folder_ok(&job, &folder) {
        discard(sab, nzo).await;
        return fail_from(
            &state.pool,
            &row.id,
            "downloading",
            UNEXPECTED_FOLDER,
            now_ms,
        )
        .await;
    }
    let Some(downloads) = state.downloads_dir.as_deref() else {
        discard(sab, nzo).await;
        return fail_from(
            &state.pool,
            &row.id,
            "downloading",
            UNEXPECTED_FOLDER,
            now_ms,
        )
        .await;
    };
    let Some(dir) = confined(downloads, &folder).await else {
        discard(sab, nzo).await;
        return fail_from(
            &state.pool,
            &row.id,
            "downloading",
            UNEXPECTED_FOLDER,
            now_ms,
        )
        .await;
    };
    let wanted = Wanted {
        title: &row.title,
        author: &row.author,
    };
    let files = list_epubs(dir.clone()).await?;
    let Some(epub) = pick::choose_epub(&wanted, &files) else {
        return retry_or_fail(state, sab, row, nzo, NO_EPUB, Some(&dir), now_ms).await;
    };
    let staged = match library::stage_file(state, &epub, ready.settings.max_epub_bytes).await {
        Ok(staged) => staged,
        Err(err) => {
            return retry_or_fail(state, sab, row, nzo, &err.to_string(), Some(&dir), now_ms).await
        }
    };
    let existed = book_owned(&state.pool, &row.user_id, &row.id).await?;
    wait_import(&state.data_dir).await;
    let owner = Owner {
        user_id: &row.user_id,
        username: &row.username,
    };
    let book = match library::import(
        state,
        &owner,
        NewId::Fixed(row.id.clone()),
        staged.with_titles(&row.title, &row.author),
    )
    .await
    {
        Ok(book) => book,
        Err(err) => {
            return retry_or_fail(state, sab, row, nzo, &err.to_string(), Some(&dir), now_ms).await
        }
    };
    let won = cas!(
        &state.pool,
        "UPDATE book_requests
         SET state = 'imported', book_id = ?, progress = 100, error = NULL,
             download_id = NULL, next_check_at = ?, updated_at = ?
         WHERE id = ? AND state = 'downloading'",
        &book.id,
        now_ms,
        now_ms,
        &row.id,
    );
    if !won {
        if !existed {
            library::remove(state, &row.user_id, &book.id).await?;
        }
        return Ok(());
    }
    remove_download(&dir).await;
    discard(sab, nzo).await;
    Ok(())
}

/// `message` is the text stored on the row. Callers prefix SAB failures.
async fn retry_or_fail(
    state: &AppState,
    sab: &Sabnzbd<'_>,
    row: &Due,
    nzo: &str,
    message: &str,
    download: Option<&Path>,
    now_ms: i64,
) -> Result<(), ApiError> {
    if !nzo.is_empty() {
        if let Err(err) = sab.delete(nzo).await {
            tracing::error!(error = %err, "sab delete failed");
            return hold(&state.pool, &row.id, "downloading", "SABnzbd", &err, now_ms).await;
        }
    }
    if let Some(dir) = download {
        remove_download(dir).await;
    }
    if row.attempts < MAX_ATTEMPTS {
        let _ = cas!(
            &state.pool,
            "UPDATE book_requests
             SET state = 'requested', error = ?, download_id = NULL, progress = 0,
                 next_check_at = ?, updated_at = ?
             WHERE id = ? AND state = 'downloading'",
            message,
            now_ms,
            now_ms,
            &row.id,
        );
    } else {
        fail_from(&state.pool, &row.id, "downloading", message, now_ms).await?;
    }
    Ok(())
}

async fn list_epubs(dir: PathBuf) -> Result<Vec<(PathBuf, u64)>, ApiError> {
    tokio::task::spawn_blocking(move || pick::list_epubs(&dir))
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "epub walk failed");
            ApiError::Internal
        })
}

async fn book_owned(pool: &SqlitePool, user_id: &str, book_id: &str) -> Result<bool, ApiError> {
    let found: Option<i64> = sqlx::query_scalar("SELECT 1 FROM books WHERE id = ? AND user_id = ?")
        .bind(book_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(found.is_some())
}

async fn adopt(pool: &SqlitePool, id: &str, nzo: &str, now_ms: i64) -> Result<(), ApiError> {
    let _ = cas!(
        pool,
        "UPDATE book_requests
         SET state = 'downloading', download_id = ?, attempts = attempts + 1,
             progress = 0, error = NULL, next_check_at = ?, updated_at = ?
         WHERE id = ? AND state = 'requested'",
        nzo,
        now_ms.saturating_add(POLL_MS),
        now_ms,
        id,
    );
    Ok(())
}

async fn hold(
    pool: &SqlitePool,
    id: &str,
    state: &str,
    who: &str,
    err: &UpstreamError,
    now_ms: i64,
) -> Result<(), ApiError> {
    let error = format!("{who}: {err}");
    note(pool, id, state, &error, now_ms.saturating_add(BACKOFF_MS)).await
}

async fn note(
    pool: &SqlitePool,
    id: &str,
    state: &str,
    error: &str,
    next_check: i64,
) -> Result<(), ApiError> {
    let sql = if state == "downloading" {
        "UPDATE book_requests SET error = ?, next_check_at = ? WHERE id = ? AND state = 'downloading'"
    } else {
        "UPDATE book_requests SET error = ?, next_check_at = ? WHERE id = ? AND state = 'requested'"
    };
    let _ = cas!(pool, sql, error, next_check, id);
    Ok(())
}

async fn fail_from(
    pool: &SqlitePool,
    id: &str,
    from: &str,
    error: &str,
    now_ms: i64,
) -> Result<(), ApiError> {
    let sql = if from == "downloading" {
        "UPDATE book_requests
         SET state = 'failed', error = ?, download_id = NULL, next_check_at = ?, updated_at = ?
         WHERE id = ? AND state = 'downloading'"
    } else {
        "UPDATE book_requests
         SET state = 'failed', error = ?, next_check_at = ?, updated_at = ?
         WHERE id = ? AND state = 'requested'"
    };
    let _ = cas!(pool, sql, error, now_ms, now_ms, id);
    Ok(())
}

async fn discard(sab: &Sabnzbd<'_>, nzo: &str) {
    if nzo.is_empty() {
        return;
    }
    if let Err(err) = sab.delete(nzo).await {
        tracing::error!(error = %err, "sab delete failed");
    }
}

fn job_name(id: &str) -> String {
    format!("mewo-{id}")
}

fn folder_ok(job: &str, folder: &str) -> bool {
    folder_matches(job, folder)
}

async fn remove_download(dir: &Path) {
    let folder = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    if let Err(err) = tokio::fs::remove_dir_all(dir).await {
        tracing::warn!(folder, error = %err, "could not remove download folder");
    }
}

async fn confined(downloads: &Path, folder: &str) -> Option<PathBuf> {
    if folder.is_empty() || folder.contains('/') || folder.contains('\\') || folder.contains('\0') {
        return None;
    }
    let candidate = downloads.join(folder);
    let meta = tokio::fs::symlink_metadata(&candidate).await.ok()?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return None;
    }
    let base = tokio::fs::canonicalize(downloads).await.ok()?;
    let dir = tokio::fs::canonicalize(&candidate).await.ok()?;
    if dir.starts_with(&base) {
        Some(dir)
    } else {
        None
    }
}

fn guids(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn size_i64(size: u64) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_name_is_the_job_or_a_numeric_suffix() {
        let job = "mewo-abc";
        assert!(folder_ok(job, "mewo-abc"));
        assert!(folder_ok(job, "mewo-abc.1"));
        assert!(folder_ok(job, "mewo-abc.02"));
        assert!(!folder_ok(job, "mewo-abc."));
        assert!(!folder_ok(job, "mewo-abc.1a"));
        assert!(!folder_ok(job, "etc"));
        assert!(!folder_ok(job, "mewo-abcd"));
    }

    #[tokio::test]
    async fn a_panicked_step_does_not_stop_the_next() {
        let joined = tokio::spawn(async {
            panic!("boom");
            #[allow(unreachable_code)]
            Ok::<(), ApiError>(())
        })
        .await;
        assert!(joined.is_err());
        report_step("req-1", joined);
        let next = tokio::spawn(async { Ok::<(), ApiError>(()) }).await;
        assert!(next.expect("next step ran").is_ok());
    }
}
