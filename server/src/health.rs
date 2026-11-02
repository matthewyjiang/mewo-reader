//! Unauthenticated liveness and readiness. No secrets, no settings, no user data.

use crate::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use sqlx::SqlitePool;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const CACHE_FOR: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub status: Status,
    pub checks: Checks,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Checks {
    pub db: Check,
    pub data_dir: Check,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Check {
    Ok,
    Fail,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Fail,
}

trait Probe: Send + Sync {
    fn run(&self) -> Pin<Box<dyn Future<Output = Report> + Send + '_>>;
}

struct FnProbe<F>(F);

impl<F, Fut> Probe for FnProbe<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = Report> + Send + 'static,
{
    fn run(&self) -> Pin<Box<dyn Future<Output = Report> + Send + '_>> {
        Box::pin((self.0)())
    }
}

struct RealProbe {
    pool: SqlitePool,
    data_dir: PathBuf,
}

impl Probe for RealProbe {
    fn run(&self) -> Pin<Box<dyn Future<Output = Report> + Send + '_>> {
        Box::pin(async move {
            // Each check is capped at PROBE_TIMEOUT, and they run together, so
            // the handler waits out one budget. A stuck data directory still
            // returns inside a 3s kube timeout.
            let (db, data_dir) = tokio::join!(
                db_ok(&self.pool),
                within_timeout(PROBE_TIMEOUT, dir_writable(&self.data_dir))
            );
            report(db, data_dir)
        })
    }
}

async fn within_timeout(timeout: Duration, check: impl Future<Output = Check>) -> Check {
    match tokio::time::timeout(timeout, check).await {
        Ok(check) => check,
        Err(_) => {
            tracing::warn!("healthz data dir check timed out");
            Check::Fail
        }
    }
}

fn report(db: Check, data_dir: Check) -> Report {
    let status = if db == Check::Ok && data_dir == Check::Ok {
        Status::Ok
    } else {
        Status::Fail
    };
    Report {
        status,
        checks: Checks { db, data_dir },
    }
}

#[derive(Clone)]
pub struct Health {
    cached: Arc<Mutex<Option<(Instant, Report)>>>,
    /// Held across a refresh so concurrent callers share one probe.
    flight: Arc<tokio::sync::Mutex<()>>,
    probe: Arc<dyn Probe>,
}

impl Health {
    pub fn new(pool: SqlitePool, data_dir: PathBuf) -> Self {
        Self::from_probe(RealProbe { pool, data_dir })
    }

    /// Tests inject a probe. chmod is not a substitute: CI may be root.
    pub fn with_probe<F, Fut>(probe: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Report> + Send + 'static,
    {
        Self::from_probe(FnProbe(probe))
    }

    fn from_probe(probe: impl Probe + 'static) -> Self {
        Self {
            cached: Arc::new(Mutex::new(None)),
            flight: Arc::new(tokio::sync::Mutex::new(())),
            probe: Arc::new(probe),
        }
    }

    /// Cached for [`CACHE_FOR`] so an unauthenticated caller cannot turn each
    /// hit into a database round-trip and a file create. A refresh is one
    /// flight: the lock is held across the probe, and the cache is checked
    /// again after the lock so waiters reuse that result.
    pub async fn ready(&self) -> Report {
        if let Some(report) = self.fresh() {
            return report;
        }
        let _flight = self.flight.lock().await;
        if let Some(report) = self.fresh() {
            return report;
        }
        let report = self.probe.run().await;
        *self.lock() = Some((Instant::now(), report.clone()));
        report
    }

    fn fresh(&self) -> Option<Report> {
        self.lock()
            .as_ref()
            .and_then(|(at, report)| (at.elapsed() < CACHE_FOR).then(|| report.clone()))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Instant, Report)>> {
        self.cached.lock().unwrap_or_else(|err| err.into_inner())
    }
}

async fn db_ok(pool: &SqlitePool) -> Check {
    match tokio::time::timeout(PROBE_TIMEOUT, sqlx::query("SELECT 1").execute(pool)).await {
        Ok(Ok(_)) => Check::Ok,
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "healthz database check failed");
            Check::Fail
        }
        Err(_) => {
            tracing::warn!("healthz database check timed out");
            Check::Fail
        }
    }
}

/// Create `.healthz-<pid>-<uuid>`, write one byte, and remove it.
/// The pid alone collides when two probes overlap before the cache fills,
/// and `create_new` would then report a writable directory as down.
async fn dir_writable(dir: &Path) -> Check {
    let path = dir.join(format!(
        ".healthz-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ));
    let wrote = match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
    {
        Ok(mut file) => file.write_all(b"x").await,
        Err(err) => Err(err),
    };
    let removed = tokio::fs::remove_file(&path).await;
    match (wrote, removed) {
        (Ok(()), Ok(())) => Check::Ok,
        (Ok(()), Err(err)) => {
            tracing::warn!(error = %err, "healthz could not remove its probe file");
            Check::Fail
        }
        (Err(err), _) => {
            tracing::warn!(error = %err, "healthz data dir probe failed");
            Check::Fail
        }
    }
}

#[derive(Serialize)]
struct LiveBody {
    status: Status,
}

fn json_response(status: StatusCode, body: impl Serialize) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

pub async fn live() -> Response {
    json_response(StatusCode::OK, LiveBody { status: Status::Ok })
}

pub async fn ready(State(state): State<AppState>) -> Response {
    let report = state.health.ready().await;
    let status = match report.status {
        Status::Ok => StatusCode::OK,
        Status::Fail => StatusCode::SERVICE_UNAVAILABLE,
    };
    json_response(status, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ok_report() -> Report {
        report(Check::Ok, Check::Ok)
    }

    #[tokio::test]
    async fn ready_is_cached() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let health = Health::with_probe(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            async { ok_report() }
        });
        let first = health.ready().await;
        let second = health.ready().await;
        assert_eq!(first, ok_report());
        assert_eq!(second, first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ready_refresh_is_single_flight() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let health = Health::with_probe(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                ok_report()
            }
        });
        let mut tasks = Vec::new();
        for _ in 0..32 {
            let health = health.clone();
            tasks.push(tokio::spawn(async move { health.ready().await }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap(), ok_report());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_stalled_dir_probe_fails_inside_its_budget() {
        let started = Instant::now();
        let check = within_timeout(Duration::from_millis(30), async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Check::Ok
        })
        .await;
        assert_eq!(check, Check::Fail);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn ready_fails_when_data_dir_is_not_writable() {
        let health = Health::with_probe(|| async { report(Check::Ok, Check::Fail) });
        let report = health.ready().await;
        assert_eq!(report.status, Status::Fail);
        assert_eq!(report.checks.db, Check::Ok);
        assert_eq!(report.checks.data_dir, Check::Fail);
    }

    #[tokio::test]
    async fn dir_probe_writes_one_byte_and_removes_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(dir_writable(tmp.path()).await, Check::Ok);
        let mut left = tokio::fs::read_dir(tmp.path()).await.unwrap();
        assert!(left.next_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn dir_probe_fails_when_the_path_is_not_a_directory() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(dir_writable(tmp.path()).await, Check::Fail);
    }
}
