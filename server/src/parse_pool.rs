//! Cap on concurrent EPUB parses.
//!
//! The permit moves into the blocking task, so a dropped request cannot free
//! it while `parse_file` is still running. Callers wait `parse_wait`, then
//! [`ApiError::Busy`].

use crate::config::{LimitsConfig, MAX_COVER_BYTES};
use crate::error::ApiError;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy, Debug, Default)]
pub struct ParseStats {
    pub parses: u64,
    pub busy: u64,
    pub peak_in_flight: usize,
}

#[derive(Clone)]
pub struct ParsePool {
    inner: Arc<Inner>,
}

struct Inner {
    sem: Arc<Semaphore>,
    wait: std::time::Duration,
    parses: AtomicU64,
    busy: AtomicU64,
    in_flight: AtomicUsize,
    peak_in_flight: AtomicUsize,
}

/// Decrements the in-flight count when the blocking task finishes, including
/// a panic. Declared after the permit so the permit is released second.
struct Flight<'a> {
    in_flight: &'a AtomicUsize,
}

impl<'a> Flight<'a> {
    fn enter(inner: &'a Inner) -> Self {
        let now = inner.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        inner.peak_in_flight.fetch_max(now, Ordering::AcqRel);
        Self {
            in_flight: &inner.in_flight,
        }
    }
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ParsePool {
    pub fn new(cfg: &LimitsConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                sem: Arc::new(Semaphore::new(cfg.max_parses)),
                wait: cfg.parse_wait,
                parses: AtomicU64::new(0),
                busy: AtomicU64::new(0),
                in_flight: AtomicUsize::new(0),
                peak_in_flight: AtomicUsize::new(0),
            }),
        }
    }

    pub fn stats(&self) -> ParseStats {
        ParseStats {
            parses: self.inner.parses.load(Ordering::Relaxed),
            busy: self.inner.busy.load(Ordering::Relaxed),
            peak_in_flight: self.inner.peak_in_flight.load(Ordering::Relaxed),
        }
    }

    /// Same error mapping as the old free `parse_epub`: a bad file is 400,
    /// a panicked task is 500, and a full queue is [`ApiError::Busy`].
    pub(crate) async fn parse(
        &self,
        path: std::path::PathBuf,
    ) -> Result<crate::epub::ParsedEpub, ApiError> {
        let permit = self.acquire().await?;
        let inner = Arc::clone(&self.inner);
        let joined = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _flight = Flight::enter(&inner);
            let parsed = crate::epub::parse_file(&path, MAX_COVER_BYTES).map_err(|reason| {
                ApiError::BadRequest(format!("Could not read that EPUB: {reason}"))
            })?;
            inner.parses.fetch_add(1, Ordering::Relaxed);
            Ok(parsed)
        });
        match joined.await {
            Ok(result) => result,
            Err(err) => {
                tracing::error!(error = %err, "EPUB parser task failed");
                Err(ApiError::Internal)
            }
        }
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        match tokio::time::timeout(self.inner.wait, Arc::clone(&self.inner.sem).acquire_owned())
            .await
        {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => {
                tracing::error!("parse semaphore closed");
                Err(ApiError::Internal)
            }
            Err(_) => {
                self.inner.busy.fetch_add(1, Ordering::Relaxed);
                Err(ApiError::Busy)
            }
        }
    }
}
