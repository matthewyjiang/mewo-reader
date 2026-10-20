use crate::config::{AuthConfig, Rate};
use crate::error::ApiError;
use crate::AppState;
use axum::extract::ConnectInfo;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Socket peer, or the address a trusted proxy appended. `0.0.0.0` when the
/// request has no [`ConnectInfo`] (in-process tests).
#[derive(Clone, Copy, Debug)]
pub struct ClientIp(pub IpAddr);

#[derive(Clone)]
pub struct Throttle {
    inner: Arc<Inner>,
}

struct Inner {
    login_ip: Mutex<Table>,
    login_user: Mutex<Table>,
    register_ip: Mutex<Table>,
    register_user: Mutex<Table>,
    trusted_proxy_hops: u8,
}

struct Table {
    rate: Rate,
    max_keys: usize,
    map: HashMap<Key, Bucket>,
}

struct Bucket {
    tokens: f64,
    updated: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    V4(Ipv4Addr),
    /// First 64 bits. One household prefix shares a bucket.
    V6Net([u8; 8]),
    User(String),
}

/// A login that spent a per-username token. Dropping it keeps the charge.
/// [`LoginAttempt::succeeded`] gives that token back.
#[must_use]
pub struct LoginAttempt<'a> {
    throttle: &'a Throttle,
    user: Key,
}

impl Throttle {
    pub fn new(cfg: &AuthConfig) -> Self {
        let table = |rate| Mutex::new(Table::new(rate, cfg.max_keys));
        Self {
            inner: Arc::new(Inner {
                login_ip: table(cfg.login_ip),
                login_user: table(cfg.login_user),
                register_ip: table(cfg.register_ip),
                register_user: table(cfg.register_user),
                trusted_proxy_hops: cfg.trusted_proxy_hops,
            }),
        }
    }

    pub(crate) fn trusted_proxy_hops(&self) -> u8 {
        self.inner.trusted_proxy_hops
    }

    /// Charges the IP (when it is globally routable) and the username.
    /// The per-username token is refunded by [`LoginAttempt::succeeded`].
    pub fn login(&self, ip: ClientIp, norm: &str) -> Result<LoginAttempt<'_>, ApiError> {
        let now = Instant::now();
        let user = Key::User(norm.to_owned());
        charge_pair(
            &self.inner.login_ip,
            &self.inner.login_user,
            ip_key(ip.0),
            user.clone(),
            now,
        )?;
        Ok(LoginAttempt {
            throttle: self,
            user,
        })
    }

    /// Charges the IP (when it is globally routable) and the username.
    /// Register has no refund: a rejected signup still spent the attempt.
    pub fn register(&self, ip: ClientIp, norm: &str) -> Result<(), ApiError> {
        charge_pair(
            &self.inner.register_ip,
            &self.inner.register_user,
            ip_key(ip.0),
            Key::User(norm.to_owned()),
            Instant::now(),
        )
    }
}

impl LoginAttempt<'_> {
    pub fn succeeded(self) {
        let mut users = lock(&self.throttle.inner.login_user);
        users.refund(&self.user, Instant::now());
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let hops = state.throttle.trusted_proxy_hops();
        Ok(ClientIp(resolve_client_ip(
            peer,
            hops,
            &forwarded_for(parts),
        )))
    }
}

/// `hops == 0` ignores `X-Forwarded-For`. Each trusted proxy appends the peer
/// it saw, so the n-th entry from the right is the client a chain of n proxies
/// observed. Entries a client forges sit further left. When the header is
/// shorter than `hops`, the leftmost address that is present is as close as
/// this process can get; a missing or unparseable value uses `peer`.
fn resolve_client_ip(peer: IpAddr, hops: u8, forwarded: &[String]) -> IpAddr {
    if hops == 0 {
        return peer;
    }
    let n = usize::from(hops);
    let candidate = if forwarded.len() >= n {
        forwarded.get(forwarded.len() - n).map(String::as_str)
    } else {
        forwarded.first().map(String::as_str)
    };
    if forwarded.len() < n && candidate.is_some() {
        tracing::debug!(
            hops,
            entries = forwarded.len(),
            "X-Forwarded-For is shorter than trusted_proxy_hops; using the leftmost address"
        );
    }
    match candidate.and_then(|value| value.parse().ok()) {
        Some(ip) => ip,
        None => {
            tracing::debug!(
                peer = %peer,
                hops,
                "X-Forwarded-For is missing or unusable; using the socket peer"
            );
            peer
        }
    }
}

fn forwarded_for(parts: &Parts) -> Vec<String> {
    let mut out = Vec::new();
    for value in parts.headers.get_all("x-forwarded-for") {
        let Ok(text) = value.to_str() else {
            continue;
        };
        for part in text.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                out.push(part.to_owned());
            }
        }
    }
    out
}

/// Per-IP buckets are only for globally routable clients. The pod network
/// (10.42.0.0/16) and a proxy that forwards that same address would otherwise
/// put every user in one bucket. Username limits still apply.
fn ip_key(ip: IpAddr) -> Option<Key> {
    if skips_ip_limit(ip) {
        return None;
    }
    Some(match ip {
        IpAddr::V4(v4) => Key::V4(v4),
        IpAddr::V6(v6) => Key::V6Net(v6_prefix(v6)),
    })
}

fn v6_prefix(ip: Ipv6Addr) -> [u8; 8] {
    let octets = ip.octets();
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&octets[..8]);
    prefix
}

/// Loopback, RFC1918, link-local, CGNAT (`100.64.0.0/10`), and IPv6 ULA
/// (`fc00::/7`). `0.0.0.0` is not in this set: in-process tests share that
/// bucket and fail closed.
fn skips_ip_limit(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            a == 127
                || a == 10
                || (a == 172 && (b & 0xf0) == 0x10)
                || (a == 192 && b == 168)
                || (a == 169 && b == 254)
                || (a == 100 && (b & 0xc0) == 0x40)
        }
        IpAddr::V6(ip) => {
            if ip.is_loopback() {
                return true;
            }
            let [a, b, ..] = ip.octets();
            (a & 0xfe) == 0xfc || (a == 0xfe && (b & 0xc0) == 0x80)
        }
    }
}

fn charge_pair(
    ip_table: &Mutex<Table>,
    user_table: &Mutex<Table>,
    ip: Option<Key>,
    user: Key,
    now: Instant,
) -> Result<(), ApiError> {
    let mut ips = lock(ip_table);
    let mut users = lock(user_table);
    let ip_result = match &ip {
        Some(key) => ips.take(key.clone(), now),
        None => Ok(()),
    };
    let user_result = users.take(user.clone(), now);
    match (ip_result, user_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(left), Err(right)) => Err(too_many(left.max(right))),
        (Err(wait), Ok(())) => {
            users.refund(&user, now);
            Err(too_many(wait))
        }
        (Ok(()), Err(wait)) => {
            if let Some(key) = &ip {
                ips.refund(key, now);
            }
            Err(too_many(wait))
        }
    }
}

fn charge(rate: Rate, bucket: &mut Bucket, now: Instant) -> Result<(), Duration> {
    refill(rate, bucket, now);
    if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        Ok(())
    } else {
        let need = 1.0 - bucket.tokens;
        Err(rate.every.mul_f64(need))
    }
}

fn too_many(wait: Duration) -> ApiError {
    ApiError::TooManyRequests {
        retry_after: retry_after_secs(wait),
    }
}

fn retry_after_secs(wait: Duration) -> u64 {
    let millis = wait.as_millis().max(1);
    u64::try_from(millis.div_ceil(1000)).unwrap_or(u64::MAX)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

impl Table {
    fn new(rate: Rate, max_keys: usize) -> Self {
        Self {
            rate,
            max_keys,
            map: HashMap::new(),
        }
    }

    fn take(&mut self, key: Key, now: Instant) -> Result<(), Duration> {
        if self.map.contains_key(&key) {
            let rate = self.rate;
            let bucket = self.map.get_mut(&key).expect("key just checked");
            return charge(rate, bucket, now);
        }
        if self.map.len() >= self.max_keys {
            self.prune(now);
        }
        if self.map.len() >= self.max_keys {
            return Err(self.rate.every);
        }
        let mut bucket = Bucket {
            tokens: self.rate.burst as f64,
            updated: now,
        };
        let result = charge(self.rate, &mut bucket, now);
        self.map.insert(key, bucket);
        result
    }

    fn refund(&mut self, key: &Key, now: Instant) {
        let Some(bucket) = self.map.get_mut(key) else {
            return;
        };
        refill(self.rate, bucket, now);
        let burst = self.rate.burst as f64;
        bucket.tokens = (bucket.tokens + 1.0).min(burst);
    }

    /// Drops buckets that have refilled to full once the table is at the cap.
    /// A table that is still full rejects the new key.
    fn prune(&mut self, now: Instant) {
        if self.map.len() < self.max_keys {
            return;
        }
        let rate = self.rate;
        let burst = rate.burst as f64;
        self.map.retain(|_, bucket| {
            refill(rate, bucket, now);
            bucket.tokens < burst
        });
    }
}

fn refill(rate: Rate, bucket: &mut Bucket, now: Instant) {
    if now <= bucket.updated {
        return;
    }
    if rate.every.is_zero() {
        bucket.tokens = rate.burst as f64;
        bucket.updated = now;
        return;
    }
    let elapsed = now.saturating_duration_since(bucket.updated);
    let add = elapsed.as_secs_f64() / rate.every.as_secs_f64();
    bucket.tokens = (bucket.tokens + add).min(rate.burst as f64);
    bucket.updated = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn auth_cfg(ip_burst: u32, user_burst: u32) -> AuthConfig {
        let every = Duration::from_secs(60);
        AuthConfig {
            login_ip: Rate {
                burst: ip_burst,
                every,
            },
            login_user: Rate {
                burst: user_burst,
                every,
            },
            ..AuthConfig::default()
        }
    }

    #[test]
    fn refill_and_retry_after_ceiling() {
        let rate = Rate {
            burst: 2,
            every: Duration::from_millis(1500),
        };
        let mut table = Table::new(rate, 100);
        let t0 = Instant::now();
        let key = Key::User("matt".into());
        assert!(table.take(key.clone(), t0).is_ok());
        assert!(table.take(key.clone(), t0).is_ok());
        let wait = table.take(key.clone(), t0).unwrap_err();
        assert_eq!(retry_after_secs(wait), 2);

        let later = t0 + Duration::from_millis(1500);
        assert!(table.take(key.clone(), later).is_ok());
        let wait = table.take(key.clone(), later).unwrap_err();
        assert_eq!(retry_after_secs(wait), 2);

        let mid = later + Duration::from_millis(750);
        let wait = table.take(key, mid).unwrap_err();
        assert_eq!(retry_after_secs(wait), 1);
    }

    #[test]
    fn ipv6_64_shares_a_key() {
        let cfg = auth_cfg(1, 10);
        let throttle = Throttle::new(&cfg);
        assert!(throttle
            .login(ClientIp(ip("2001:db8:1:2::1")), "matt")
            .is_ok());
        assert!(throttle
            .login(ClientIp(ip("2001:db8:1:2::ffff")), "ada")
            .is_err());
        assert!(throttle
            .login(ClientIp(ip("2001:db8:1:3::1")), "ada")
            .is_ok());
    }

    #[test]
    fn prune_drops_full_buckets_and_fails_closed() {
        let rate = Rate {
            burst: 1,
            every: Duration::from_secs(60),
        };
        let mut table = Table::new(rate, 2);
        let t0 = Instant::now();
        let a = Key::V4(Ipv4Addr::new(1, 1, 1, 1));
        let b = Key::V4(Ipv4Addr::new(1, 1, 1, 2));
        let c = Key::V4(Ipv4Addr::new(1, 1, 1, 3));
        assert!(table.take(a, t0).is_ok());
        assert!(table.take(b, t0).is_ok());
        assert!(table.take(c.clone(), t0).is_err());
        let later = t0 + Duration::from_secs(60);
        assert!(table.take(c, later).is_ok());
        assert!(table
            .take(Key::V4(Ipv4Addr::new(1, 1, 1, 4)), later)
            .is_ok());
    }

    #[test]
    fn private_ranges_skip_the_ip_bucket_and_public_ones_do_not() {
        let cfg = auth_cfg(1, 100);
        let throttle = Throttle::new(&cfg);
        for text in [
            "127.0.0.1",
            "10.42.0.8",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "100.127.255.255",
            "::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
        ] {
            assert!(
                throttle.login(ClientIp(ip(text)), "one").is_ok(),
                "{text} first"
            );
            assert!(
                throttle.login(ClientIp(ip(text)), "two").is_ok(),
                "{text} should not spend an IP token"
            );
        }
        assert!(throttle.login(ClientIp(ip("1.1.1.1")), "pub").is_ok());
        assert!(throttle.login(ClientIp(ip("1.1.1.1")), "pub2").is_err());
        assert!(throttle.login(ClientIp(ip("8.8.8.8")), "pub2").is_ok());
        assert!(throttle.login(ClientIp(ip("0.0.0.0")), "shared").is_ok());
        assert!(throttle.login(ClientIp(ip("0.0.0.0")), "shared2").is_err());
    }

    #[test]
    fn username_limit_still_applies_to_a_private_ip() {
        let cfg = auth_cfg(1, 2);
        let throttle = Throttle::new(&cfg);
        let ip = ClientIp(ip("10.42.1.5"));
        assert!(throttle.login(ip, "matt").is_ok());
        assert!(throttle.login(ip, "matt").is_ok());
        assert!(throttle.login(ip, "matt").is_err());
        assert!(throttle.login(ip, "ada").is_ok());
    }

    #[test]
    fn forwarded_for_hops_and_a_short_header() {
        let peer = ip("10.42.0.9");
        assert_eq!(
            resolve_client_ip(peer, 0, &["1.1.1.1".into(), "8.8.8.8".into()]),
            peer
        );
        assert_eq!(
            resolve_client_ip(peer, 1, &["1.1.1.1".into(), "203.0.113.9".into()]),
            ip("203.0.113.9")
        );
        assert_eq!(
            resolve_client_ip(peer, 2, &["203.0.113.9".into(), "203.0.113.10".into()]),
            ip("203.0.113.9")
        );
        assert_eq!(
            resolve_client_ip(peer, 2, &["1.1.1.1".into()]),
            ip("1.1.1.1")
        );
        assert_eq!(resolve_client_ip(peer, 2, &["not-an-ip".into()]), peer);
        assert_eq!(resolve_client_ip(peer, 1, &["not-an-ip".into()]), peer);
        assert_eq!(resolve_client_ip(peer, 1, &[]), peer);
    }
}
