use crate::config::AuthConfig;
use crate::error::ApiError;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Argon2id hash of 32 random bytes, using the same default params as stored
/// passwords. Missing-user logins verify against this so they cost the same
/// work as a real hash.
static DUMMY_HASH: OnceLock<String> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default)]
pub struct PasswordStats {
    pub hashes: u64,
    pub verifies: u64,
    pub dummy_verifies: u64,
    pub peak_in_flight: usize,
}

#[derive(Clone)]
pub struct Passwords {
    inner: Arc<Inner>,
}

struct Inner {
    argon: Argon2<'static>,
    sem: Arc<Semaphore>,
    queue_wait: std::time::Duration,
    hashes: AtomicU64,
    verifies: AtomicU64,
    dummy_verifies: AtomicU64,
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

impl Passwords {
    pub fn new(cfg: &AuthConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                argon: Argon2::default(),
                sem: Arc::new(Semaphore::new(cfg.max_concurrent)),
                queue_wait: cfg.queue_wait,
                hashes: AtomicU64::new(0),
                verifies: AtomicU64::new(0),
                dummy_verifies: AtomicU64::new(0),
                in_flight: AtomicUsize::new(0),
                peak_in_flight: AtomicUsize::new(0),
            }),
        }
    }

    /// New PHC string. Busy → [`ApiError::Busy`]. A panicked task or an
    /// argon2 error → [`ApiError::Internal`].
    pub async fn hash(&self, password: &str) -> Result<String, ApiError> {
        let permit = self.acquire().await?;
        let password = password.to_owned();
        let inner = Arc::clone(&self.inner);
        let argon = self.inner.argon.clone();
        Self::finish(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _flight = Flight::enter(&inner);
            let salt = SaltString::generate(&mut OsRng);
            match argon.hash_password(password.as_bytes(), &salt) {
                Ok(hash) => {
                    inner.hashes.fetch_add(1, Ordering::Relaxed);
                    Ok(hash.to_string())
                }
                Err(err) => {
                    tracing::error!(error = %err, "password hash failed");
                    Err(ApiError::Internal)
                }
            }
        }))
        .await
    }

    /// `stored = None` verifies against [`DUMMY_HASH`] and returns false.
    /// Same permit, same params, same thread pool as a real password.
    pub async fn verify_login(
        &self,
        password: &str,
        stored: Option<&str>,
    ) -> Result<bool, ApiError> {
        let permit = self.acquire().await?;
        let password = password.to_owned();
        let stored = stored.map(str::to_owned);
        let inner = Arc::clone(&self.inner);
        let argon = self.inner.argon.clone();
        Self::finish(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _flight = Flight::enter(&inner);
            let dummy = stored.is_none();
            let hash = match &stored {
                Some(hash) => hash.clone(),
                None => dummy_hash(),
            };
            let parsed = match PasswordHash::new(&hash) {
                Ok(parsed) => parsed,
                Err(err) => {
                    tracing::error!(error = %err, "stored hash is not parseable");
                    return Err(ApiError::Internal);
                }
            };
            let ok = argon.verify_password(password.as_bytes(), &parsed).is_ok();
            inner.verifies.fetch_add(1, Ordering::Relaxed);
            if dummy {
                inner.dummy_verifies.fetch_add(1, Ordering::Relaxed);
                return Ok(false);
            }
            Ok(ok)
        }))
        .await
    }

    /// Fills [`DUMMY_HASH`] off the request path so the first missing-user
    /// login is not also paying to create the dummy.
    pub async fn warm(&self) {
        if DUMMY_HASH.get().is_some() {
            return;
        }
        let argon = self.inner.argon.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let _ = dummy_hash_with(&argon);
        })
        .await;
        if let Err(err) = joined {
            tracing::error!(error = %err, "password warm failed");
        }
    }

    pub fn stats(&self) -> PasswordStats {
        PasswordStats {
            hashes: self.inner.hashes.load(Ordering::Relaxed),
            verifies: self.inner.verifies.load(Ordering::Relaxed),
            dummy_verifies: self.inner.dummy_verifies.load(Ordering::Relaxed),
            peak_in_flight: self.inner.peak_in_flight.load(Ordering::Relaxed),
        }
    }

    async fn acquire(&self) -> Result<OwnedSemaphorePermit, ApiError> {
        match tokio::time::timeout(
            self.inner.queue_wait,
            Arc::clone(&self.inner.sem).acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => {
                tracing::error!("password semaphore closed");
                Err(ApiError::Internal)
            }
            Err(_) => Err(ApiError::Busy),
        }
    }

    async fn finish<T>(
        joined: tokio::task::JoinHandle<Result<T, ApiError>>,
    ) -> Result<T, ApiError> {
        match joined.await {
            Ok(result) => result,
            Err(err) => {
                tracing::error!(error = %err, "password task failed");
                Err(ApiError::Internal)
            }
        }
    }
}

fn dummy_hash() -> String {
    dummy_hash_with(&Argon2::default())
}

fn dummy_hash_with(argon: &Argon2<'static>) -> String {
    DUMMY_HASH
        .get_or_init(|| {
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            let salt = SaltString::generate(&mut OsRng);
            argon
                .hash_password(&bytes, &salt)
                .expect("dummy password hash")
                .to_string()
        })
        .clone()
}
