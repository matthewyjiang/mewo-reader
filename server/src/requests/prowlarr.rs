//! Prowlarr is search-only. Nothing here calls the grab endpoint.

use crate::requests::http::{same_origin, Upstream, UpstreamError};
use crate::requests::pick::{Category, Release};
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

pub(crate) const EBOOK_CATEGORY: u32 = 7020;
const SEARCH_CAP: usize = 8 * 1024 * 1024;
const SMALL_CAP: usize = 1024 * 1024;
const SEARCH_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) struct Prowlarr<'a> {
    pub up: &'a Upstream,
    pub base: Url,
    pub key: String,
}

impl Prowlarr<'_> {
    /// `GET /api/v1/search?type=search&query=..&categories=7020&indexerIds=-1&limit=100`
    /// (cap 8 MiB, 90s). `indexerIds=-1` is every usenet indexer. One malformed
    /// element is skipped. Keeps `protocol == usenet` only, and only rows whose
    /// `downloadUrl` has the configured base's origin, so an indexer can't make
    /// SAB fetch an arbitrary URL.
    pub async fn search(&self, query: &str) -> Result<Vec<Release>, UpstreamError> {
        let body: Value = self
            .up
            .get_json_timeout(
                &self.base,
                "/api/v1/search",
                &[
                    ("query", query),
                    ("type", "search"),
                    ("categories", "7020"),
                    ("indexerIds", "-1"),
                    ("limit", "100"),
                ],
                &[("X-Api-Key", self.key.as_str())],
                SEARCH_CAP,
                SEARCH_TIMEOUT,
            )
            .await?;
        let items = body.as_array().ok_or(UpstreamError::Shape)?;
        let releases = items
            .iter()
            .filter_map(|item| serde_json::from_value::<Release>(item.clone()).ok());
        Ok(releases
            .filter(|release| {
                release.protocol.eq_ignore_ascii_case("usenet")
                    && release
                        .download_url
                        .as_deref()
                        .is_some_and(|url| same_origin(&self.base, url))
            })
            .collect())
    }

    /// `GET /api/v1/system/status` (`appName` must be `Prowlarr`) plus
    /// `/api/v1/indexer`: count enabled usenet indexers whose caps include 7020.
    pub async fn status(&self) -> Result<String, UpstreamError> {
        let status: SystemStatus = self
            .up
            .get_json(
                &self.base,
                "/api/v1/system/status",
                &[],
                &[("X-Api-Key", self.key.as_str())],
                SMALL_CAP,
            )
            .await?;
        if status.app_name != "Prowlarr" || status.version.is_empty() {
            return Err(UpstreamError::Shape);
        }
        let indexers: Vec<Indexer> = self
            .up
            .get_json(
                &self.base,
                "/api/v1/indexer",
                &[],
                &[("X-Api-Key", self.key.as_str())],
                SMALL_CAP,
            )
            .await?;
        let count = indexers
            .iter()
            .filter(|indexer| indexer.ebook_usenet())
            .count();
        Ok(format!(
            "{} · {count} usenet indexers with ebooks",
            status.version
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemStatus {
    app_name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct Indexer {
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    enable: bool,
    #[serde(default)]
    capabilities: Caps,
}

#[derive(Debug, Default, Deserialize)]
struct Caps {
    #[serde(default)]
    categories: Vec<Category>,
}

impl Indexer {
    fn ebook_usenet(&self) -> bool {
        self.enable
            && self.protocol.eq_ignore_ascii_case("usenet")
            && self
                .capabilities
                .categories
                .iter()
                .any(|category| category.id == i64::from(EBOOK_CATEGORY))
    }
}
