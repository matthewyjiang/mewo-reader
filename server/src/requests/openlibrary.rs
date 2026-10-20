//! Open Library search. One in-flight request, 1 s apart. The User-Agent has
//! no contact email, so the gap stays at a second.

use crate::requests::http::{base_url, UpstreamError};
use crate::requests::matching::{isbn13s, WorkContext};
use crate::AppState;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

const CACHE_FOR: Duration = Duration::from_secs(24 * 60 * 60);

const GAP: Duration = Duration::from_secs(1);
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

/// The work plus the author's top 100 works by edition count. Two paced calls.
/// Successes stay in [`WorkCache`] for 24 hours. A miss is not cached, so a
/// later tick can recover. No `author_key`, or an author named Unknown, leaves
/// `siblings` empty and the title gate is the only check.
pub(crate) async fn work_context(
    state: &AppState,
    work_key: &str,
) -> Result<WorkContext, UpstreamError> {
    if let Some(hit) = state.work_cache.get(work_key) {
        return Ok(hit);
    }
    let ctx = fetch_context(state, work_key).await?;
    state.work_cache.insert(work_key, ctx.clone());
    Ok(ctx)
}

pub(crate) struct WorkCache {
    entries: std::sync::Mutex<HashMap<String, (Instant, WorkContext)>>,
}

impl WorkCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, key: &str) -> Option<WorkContext> {
        let mut entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        let stale = entries
            .get(key)
            .is_some_and(|(at, _)| at.elapsed() >= CACHE_FOR);
        if stale {
            entries.remove(key);
            return None;
        }
        entries.get(key).map(|(_, ctx)| ctx.clone())
    }

    fn insert(&self, key: &str, ctx: WorkContext) {
        let mut entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        entries.insert(key.to_string(), (Instant::now(), ctx));
    }
}

async fn fetch_context(state: &AppState, work_key: &str) -> Result<WorkContext, UpstreamError> {
    state.catalog_limit.pace().await;
    let base = base_url(&state.openlibrary_url)?;
    let q = format!("key:{work_key}");
    let body: Search<WorkDoc> = state
        .upstream
        .get_json(
            &base,
            "/search.json",
            &[
                ("q", q.as_str()),
                (
                    "fields",
                    "key,title,subtitle,alternative_title,author_key,author_name,isbn",
                ),
            ],
            &[],
            2 * 1024 * 1024,
        )
        .await?;
    let Some(doc) = body
        .docs
        .into_iter()
        .find(|doc| !doc.title.trim().is_empty())
    else {
        return Err(UpstreamError::Shape);
    };
    let author_key = sibling_key(&doc);
    let siblings = if let Some(author_key) = author_key {
        state.catalog_limit.pace().await;
        let body: Search<SibDoc> = state
            .upstream
            .get_json(
                &base,
                "/search.json",
                &[
                    ("author_key", author_key.as_str()),
                    ("fields", "key,title,subtitle"),
                    ("sort", "editions"),
                    ("limit", "100"),
                ],
                &[],
                2 * 1024 * 1024,
            )
            .await?;
        body.docs
            .into_iter()
            .filter_map(|doc| {
                let title = doc.title.trim();
                if title.is_empty() {
                    None
                } else {
                    Some(title.to_string())
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(assemble(doc, siblings))
}

fn sibling_key(doc: &WorkDoc) -> Option<String> {
    let key = doc
        .author_key
        .iter()
        .map(|key| key.trim())
        .find(|key| !key.is_empty())?;
    let names: Vec<&str> = doc
        .author_name
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty()
        || names
            .iter()
            .all(|name| name.eq_ignore_ascii_case("unknown"))
    {
        return None;
    }
    Some(key.to_string())
}

fn assemble(doc: WorkDoc, siblings: Vec<String>) -> WorkContext {
    let title = doc.title.trim();
    let mut titles = Vec::new();
    let mut push = |value: &str| {
        let value = value.trim();
        if value.is_empty() || titles.iter().any(|have: &String| have == value) {
            return;
        }
        titles.push(value.to_string());
    };
    push(title);
    if let Some(subtitle) = doc
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|subtitle| !subtitle.is_empty())
    {
        push(&format!("{title}: {subtitle}"));
    }
    for alt in &doc.alternative_title {
        push(alt);
    }
    if let Some((head, _)) = title.split_once(':') {
        push(head);
    }
    let mut isbns = HashSet::new();
    for raw in &doc.isbn {
        isbns.extend(isbn13s(raw));
    }
    WorkContext {
        titles,
        authors: doc
            .author_name
            .into_iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect(),
        isbns,
        siblings,
    }
}

#[derive(Debug, Deserialize)]
struct OlSearch {
    #[serde(default)]
    docs: Vec<OlDoc>,
    #[serde(rename = "numFound", default)]
    num_found: i64,
}

#[derive(Debug, Deserialize)]
struct Search<T> {
    docs: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct WorkDoc {
    #[serde(default)]
    title: String,
    subtitle: Option<String>,
    #[serde(default, deserialize_with = "string_list")]
    alternative_title: Vec<String>,
    #[serde(default)]
    author_key: Vec<String>,
    #[serde(default)]
    author_name: Vec<String>,
    #[serde(default)]
    isbn: Vec<String>,
}

fn string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::String(text) => Ok(vec![text]),
        serde_json::Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()),
        _ => Err(serde::de::Error::custom("alternative_title")),
    }
}

#[derive(Debug, Deserialize)]
struct SibDoc {
    #[serde(default)]
    title: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_include_subtitle_alternatives_and_the_precolon_part() {
        let raw = r#"{
            "docs": [{
                "key": "/works/OL1W",
                "title": "Dune: The Butlerian Jihad",
                "subtitle": "Legends",
                "alternative_title": ["The Butlerian Jihad", "Dune"],
                "author_key": ["OL79034A"],
                "author_name": ["Frank Herbert", "Brian Herbert"],
                "isbn": ["0441172717", "9780441172719", "not-an-isbn"]
            }]
        }"#;
        let body: Search<WorkDoc> = serde_json::from_str(raw).unwrap();
        let ctx = assemble(
            body.docs.into_iter().next().unwrap(),
            vec!["Dune Messiah".into()],
        );
        assert_eq!(
            ctx.titles,
            vec![
                "Dune: The Butlerian Jihad".to_string(),
                "Dune: The Butlerian Jihad: Legends".to_string(),
                "The Butlerian Jihad".to_string(),
                "Dune".to_string(),
            ]
        );
        assert_eq!(
            ctx.authors,
            vec!["Frank Herbert".to_string(), "Brian Herbert".to_string()]
        );
        assert_eq!(ctx.isbns, HashSet::from(["9780441172719".to_string()]));
        assert_eq!(ctx.siblings, vec!["Dune Messiah".to_string()]);
    }

    #[test]
    fn unknown_author_or_missing_key_skips_siblings() {
        let mut doc = WorkDoc {
            title: "Dune".into(),
            subtitle: None,
            alternative_title: vec![],
            author_key: vec!["OL79034A".into()],
            author_name: vec!["Unknown".into()],
            isbn: vec![],
        };
        assert!(sibling_key(&doc).is_none());
        doc.author_name.clear();
        assert!(sibling_key(&doc).is_none());
        doc.author_name = vec!["Frank Herbert".into()];
        doc.author_key.clear();
        assert!(sibling_key(&doc).is_none());
        doc.author_key = vec![" OL79034A ".into()];
        assert_eq!(sibling_key(&doc).as_deref(), Some("OL79034A"));
    }

    #[test]
    fn a_single_alternative_title_string_is_accepted() {
        let raw = r#"{"title":"Dune","alternative_title":"Dune Messiah"}"#;
        let doc: WorkDoc = serde_json::from_str(raw).unwrap();
        assert_eq!(doc.alternative_title, vec!["Dune Messiah".to_string()]);
    }
}
