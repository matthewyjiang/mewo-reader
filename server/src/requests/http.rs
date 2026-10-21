//! One outbound client. Paths are fixed, redirects are off, bodies are capped,
//! and errors never carry a URL. The SABnzbd key travels in `?apikey=`.

use reqwest::Url;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// The one outbound client. `redirect::Policy::none()`, connect 5s, total 20s,
/// User-Agent `mewo-reader/<version> (+https://github.com/matthewyjiang/mewo-reader)`.
#[derive(Clone)]
pub(crate) struct Upstream {
    client: reqwest::Client,
}

/// Display never includes a URL, query string, or body. reqwest errors go
/// through `without_url()` because the SAB key travels in `?apikey=`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UpstreamError {
    #[error("not configured")]
    NotConfigured,
    #[error("can't connect")]
    Connect,
    #[error("timed out")]
    Timeout,
    #[error("HTTP {0}")]
    Status(u16),
    /// Wrong JSON, the wrong product, or a body over the size cap.
    #[error("unexpected response")]
    Shape,
    /// SAB's own `{"error": "..."}` text, at most 200 characters.
    #[error("{0}")]
    Service(String),
}

impl Upstream {
    pub(crate) fn new() -> Result<Self, String> {
        let version = crate::version::version();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .user_agent(format!(
                "mewo-reader/{version} (+https://github.com/matthewyjiang/mewo-reader)"
            ))
            .build()
            .map_err(|_| "http client".to_string())?;
        Ok(Self { client })
    }

    /// GET `base` + a fixed path. `path` is `&'static str` so no caller can build
    /// a path from user data. The body is read up to `cap` bytes, then serde.
    /// The client timeout is 20s. [`Self::get_json_timeout`] overrides that for
    /// one call.
    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        base: &Url,
        path: &'static str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        cap: usize,
    ) -> Result<T, UpstreamError> {
        self.request_json(base, path, query, headers, cap, None)
            .await
    }

    /// [`Self::get_json`] with its own total timeout. A per-request timeout
    /// replaces the client's 20s, it does not add to it.
    pub(crate) async fn get_json_timeout<T: DeserializeOwned>(
        &self,
        base: &Url,
        path: &'static str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        cap: usize,
        timeout: Duration,
    ) -> Result<T, UpstreamError> {
        self.request_json(base, path, query, headers, cap, Some(timeout))
            .await
    }

    /// `GET /works/{id}.json` for a work key this process already accepted.
    /// A redirect yields that response's work key, from the `Location` header
    /// or a JSON `location`. One hop. The path is only `works/OL<digits>W.json`.
    pub(crate) async fn work_location(
        &self,
        base: &Url,
        work_key: &str,
    ) -> Result<Option<String>, UpstreamError> {
        // The matcher only needs the location. A full work document is larger.
        match self.load_work(base, work_key, 64 * 1024).await? {
            RawWork::Redirect(key) => Ok(key),
            RawWork::Body(value) => {
                let location = value
                    .get("location")
                    .and_then(|item| item.as_str())
                    .unwrap_or("");
                if location.is_empty() {
                    Ok(None)
                } else {
                    Ok(work_key_from_location(location))
                }
            }
        }
    }

    /// `GET /works/{id}.json` including the document. Catalog detail reads this.
    /// Same path rules as [`Self::work_location`], with room for subjects and
    /// a description. One hop: an HTTP redirect is [`RawWork::Redirect`].
    pub(crate) async fn work_document(
        &self,
        base: &Url,
        work_key: &str,
    ) -> Result<RawWork, UpstreamError> {
        self.load_work(base, work_key, 2 * 1024 * 1024).await
    }

    async fn load_work(
        &self,
        base: &Url,
        work_key: &str,
        cap: usize,
    ) -> Result<RawWork, UpstreamError> {
        let relative = work_json_path(work_key)?;
        let url = base.join(&relative).map_err(|_| UpstreamError::Shape)?;
        let response = self.client.get(url).send().await.map_err(map_err)?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            return Ok(RawWork::Redirect(work_key_from_location(location)));
        }
        if !status.is_success() {
            return Err(UpstreamError::Status(status.as_u16()));
        }
        let bytes = read_capped(response, cap).await?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| UpstreamError::Shape)?;
        Ok(RawWork::Body(value))
    }

    async fn request_json<T: DeserializeOwned>(
        &self,
        base: &Url,
        path: &'static str,
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        cap: usize,
        timeout: Option<Duration>,
    ) -> Result<T, UpstreamError> {
        let url = endpoint(base, path)?;
        let mut request = self.client.get(url);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        if !query.is_empty() {
            request = request.query(query);
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().await.map_err(map_err)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(UpstreamError::Status(status.as_u16()));
        }
        if !status.is_success() {
            return Err(UpstreamError::Status(status.as_u16()));
        }
        let bytes = read_capped(response, cap).await?;
        serde_json::from_slice(&bytes).map_err(|_| UpstreamError::Shape)
    }
}

/// `""` → [`UpstreamError::NotConfigured`]. Only http/https, a host, no
/// userinfo, no query or fragment. A trailing `/` is trimmed. Private addresses
/// are allowed on purpose (that's where Prowlarr lives). A non-empty path is
/// rejected so the base can't retarget the fixed paths.
pub(crate) fn base_url(raw: &str) -> Result<Url, UpstreamError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(UpstreamError::NotConfigured);
    }
    let mut url = Url::parse(raw).map_err(|_| UpstreamError::Shape)?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(UpstreamError::Shape);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UpstreamError::Shape);
    }
    if url.host_str().is_none() {
        return Err(UpstreamError::Shape);
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(UpstreamError::Shape);
    }
    let path = url.path();
    if path != "/" && !path.is_empty() {
        return Err(UpstreamError::Shape);
    }
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn same_origin(base: &Url, raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.scheme() == base.scheme()
        && url.host() == base.host()
        && url.port_or_known_default() == base.port_or_known_default()
}

pub(crate) fn service_text(raw: &str) -> String {
    let line = raw.lines().next().unwrap_or("").trim();
    let lower = line.to_ascii_lowercase();
    if line.contains("://") || lower.contains("apikey") || lower.contains("api_key") {
        return "error".into();
    }
    let text: String = line.chars().take(200).collect();
    if text.is_empty() {
        "error".into()
    } else {
        text
    }
}

fn endpoint(base: &Url, path: &'static str) -> Result<Url, UpstreamError> {
    let relative = path.trim_start_matches('/');
    base.join(relative).map_err(|_| UpstreamError::Shape)
}

fn work_json_path(work_key: &str) -> Result<String, UpstreamError> {
    let id = work_key
        .strip_prefix("/works/")
        .ok_or(UpstreamError::Shape)?;
    if !work_id(id) {
        return Err(UpstreamError::Shape);
    }
    Ok(format!("works/{id}.json"))
}

fn work_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("OL") else {
        return false;
    };
    let Some(digits) = rest.strip_suffix('W') else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

/// An HTTP `Location` or a JSON `location` that names `/works/OL…W`.
pub(crate) fn work_key_from_location(location: &str) -> Option<String> {
    let path = location.split(['?', '#']).next().unwrap_or(location).trim();
    let start = path.find("/works/")?;
    let rest = &path[start + "/works/".len()..];
    let id = rest
        .trim_end_matches(".json")
        .split('/')
        .next()
        .unwrap_or("");
    if !work_id(id) {
        return None;
    }
    Some(format!("/works/{id}"))
}

async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, UpstreamError> {
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > cap {
                    return Err(UpstreamError::Shape);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(err) => return Err(map_err(err)),
        }
    }
}

/// One `/works/{id}.json`. `Redirect` is an HTTP redirect, already parsed.
pub(crate) enum RawWork {
    Redirect(Option<String>),
    Body(serde_json::Value),
}

fn map_err(err: reqwest::Error) -> UpstreamError {
    let err = err.without_url();
    if err.is_timeout() {
        return UpstreamError::Timeout;
    }
    if err.is_redirect() {
        return UpstreamError::Status(err.status().map(|status| status.as_u16()).unwrap_or(302));
    }
    if let Some(status) = err.status() {
        return UpstreamError::Status(status.as_u16());
    }
    // Connect, DNS, and anything else. The Display of the reqwest error can
    // still carry a URL on some builds, so it is never formatted.
    UpstreamError::Connect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rejects_the_listed_shapes_and_trims_a_slash() {
        assert!(matches!(
            base_url("").unwrap_err(),
            UpstreamError::NotConfigured
        ));
        assert!(matches!(
            base_url("ftp://prowlarr.example").unwrap_err(),
            UpstreamError::Shape
        ));
        assert!(matches!(
            base_url("http://user:pass@prowlarr.example").unwrap_err(),
            UpstreamError::Shape
        ));
        assert!(matches!(
            base_url("http://prowlarr.example?apikey=secret").unwrap_err(),
            UpstreamError::Shape
        ));
        assert!(matches!(
            base_url("http://prowlarr.example/#frag").unwrap_err(),
            UpstreamError::Shape
        ));
        assert!(matches!(
            base_url("http://prowlarr.example/extra").unwrap_err(),
            UpstreamError::Shape
        ));
        let url = base_url("http://prowlarr.example/").unwrap();
        assert_eq!(url.as_str(), "http://prowlarr.example/");
        assert!(url.query().is_none());
    }

    #[test]
    fn a_work_redirect_location_is_a_work_key() {
        assert_eq!(
            work_key_from_location("/works/OL893414W.json").as_deref(),
            Some("/works/OL893414W")
        );
        assert_eq!(
            work_key_from_location("https://openlibrary.org/works/OL893414W").as_deref(),
            Some("/works/OL893414W")
        );
        assert!(work_key_from_location("/authors/OL79034A").is_none());
        assert!(work_json_path("/works/../OL1W").is_err());
        assert!(work_json_path("/works/OL893414W").is_ok());
    }

    #[test]
    fn service_text_drops_urls_and_caps_length() {
        assert_eq!(service_text("API Key Incorrect"), "API Key Incorrect");
        assert_eq!(service_text("see http://x?apikey=abc"), "error");
        assert_eq!(service_text(&"a".repeat(400)).chars().count(), 200);
    }
}
