//! Free-space floor for uploads and the request worker.
//!
//! `min_free == 0` skips the probe. A probe error is a warning and a pass:
//! an odd filesystem must not brick uploads. Reads are not checked.

use crate::error::ApiError;
use std::io;
use std::path::Path;
use std::sync::Arc;

type Probe = dyn Fn(&Path) -> io::Result<u64> + Send + Sync;

#[derive(Clone)]
pub struct DiskGuard {
    min_free: u64,
    probe: Arc<Probe>,
}

impl DiskGuard {
    pub fn new(min_free: u64) -> Self {
        Self::with_probe(min_free, free_bytes)
    }

    pub fn with_probe(
        min_free: u64,
        probe: impl Fn(&Path) -> io::Result<u64> + Send + Sync + 'static,
    ) -> Self {
        Self {
            min_free,
            probe: Arc::new(probe),
        }
    }

    /// `Ok` when the floor is off, the probe fails, or `free >= min_free + need`.
    pub fn require(&self, dir: &Path, need: u64) -> Result<(), ApiError> {
        if self.min_free == 0 {
            return Ok(());
        }
        let free = match (self.probe)(dir) {
            Ok(free) => free,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    dir = %dir.display(),
                    "free-disk probe failed; allowing the write"
                );
                return Ok(());
            }
        };
        if free >= self.min_free.saturating_add(need) {
            Ok(())
        } else {
            Err(ApiError::DiskLow)
        }
    }
}

fn free_bytes(path: &Path) -> io::Result<u64> {
    let stat = rustix::fs::statvfs(path)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, SubscriberExt};
    use tracing_subscriber::Layer;

    struct WarnSeen(Arc<AtomicBool>);

    impl<S> Layer<S> for WarnSeen
    where
        S: Subscriber,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            if *event.metadata().level() == tracing::Level::WARN {
                self.0.store(true, Ordering::SeqCst);
            }
        }
    }

    #[test]
    fn zero_floor_does_not_probe() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let guard = DiskGuard::with_probe(0, move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        });
        guard.require(Path::new("."), u64::MAX).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn boundary_is_free_at_least_floor_plus_need() {
        let guard = DiskGuard::with_probe(100, |_| Ok(150));
        assert!(guard.require(Path::new("."), 50).is_ok());
        let guard = DiskGuard::with_probe(100, |_| Ok(149));
        let err = guard.require(Path::new("."), 50).unwrap_err();
        assert_eq!(
            err.to_string(),
            "The server is low on disk space. Try again later."
        );
    }

    #[test]
    fn a_probe_error_fails_open_and_warns() {
        let seen = Arc::new(AtomicBool::new(false));
        let subscriber = tracing_subscriber::registry().with(WarnSeen(Arc::clone(&seen)));
        tracing::subscriber::with_default(subscriber, || {
            let guard = DiskGuard::with_probe(100, |_| Err(io::Error::other("statvfs")));
            assert!(guard.require(Path::new("."), 1).is_ok());
        });
        assert!(seen.load(Ordering::SeqCst), "probe error did not warn");
    }

    #[test]
    fn statvfs_sees_free_space_on_the_temp_dir() {
        let dir = std::env::temp_dir();
        let guard = DiskGuard::new(1);
        guard
            .require(&dir, 0)
            .unwrap_or_else(|err| panic!("temp dir {dir:?} should clear a 1-byte floor: {err}"));
    }
}
