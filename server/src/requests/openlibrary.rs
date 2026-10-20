//! Open Library search. One in-flight request, 350 ms apart.

use crate::requests::http::{base_url, UpstreamError};
use crate::AppState;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

const GAP: Duration = Duration::from_millis(350);
const PAGE: i64 = 20;

pub(crate) struct Limiter {
    sem: Semaphore,
    next: Mutex<Instant>,
}

impl Limiter {
    pub(crate) fn new() -> Self {
        Self {
            sem: Semaphore::new(1),
            next: Mutex::new(Instant::now()),
        }
    }

    async fn pace(&self) {
        let _permit = self
            .sem
            .acquire()
            .await
            .expect("catalog limiter stays open");
        let wait = {
            let mut next = self.next.lock().await;
            let now = Instant::now();
            let wait = next.saturating_duration_since(now);
            *next = now.max(*next) + GAP;
            wait
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[derive(Debug)]
pub(crate) struct Work {
    pub work_key: String,
    pub title: String,
    pub author: String,
    pub year: Option<i64>,
    pub cover_id: Option<i64>,
}

pub(crate) async fn search(
    state: &AppState,
    q: &str,
    page: i64,
) -> Result<(Vec<Work>, Option<i64>), UpstreamError> {
    state.catalog_limit.pace().await;
    let base = base_url(&state.openlibrary_url)?;
    let page_s = page.to_string();
    let body: OlSearch = state
        .upstream
        .get_json(
            &base,
            "/search.json",
            &[
                ("q", q),
                ("page", page_s.as_str()),
                ("limit", "20"),
                ("fields", "key,title,author_name,first_publish_year,cover_i"),
            ],
            &[],
            2 * 1024 * 1024,
        )
        .await?;
    let works = body
        .docs
        .into_iter()
        .filter_map(|doc| {
            if !doc.key.starts_with("/works/") || doc.title.trim().is_empty() {
                return None;
            }
            Some(Work {
                work_key: doc.key,
                title: doc.title.trim().to_string(),
                author: doc
                    .author_name
                    .into_iter()
                    .find(|name| !name.trim().is_empty())
                    .map(|name| name.trim().to_string())
                    .unwrap_or_else(|| "Unknown".into()),
                year: doc.first_publish_year,
                cover_id: doc.cover_i.filter(|id| *id >= 0),
            })
        })
        .collect();
    let next = if page.saturating_mul(PAGE) < body.num_found {
        Some(page + 1)
    } else {
        None
    };
    Ok((works, next))
}

#[derive(Debug, Deserialize)]
struct OlSearch {
    #[serde(default)]
    docs: Vec<OlDoc>,
    #[serde(rename = "numFound", default)]
    num_found: i64,
}

#[derive(Debug, Deserialize)]
struct OlDoc {
    key: String,
    title: String,
    #[serde(default)]
    author_name: Vec<String>,
    first_publish_year: Option<i64>,
    cover_i: Option<i64>,
}
