//! Open Library search. One in-flight request, 1 s apart. The User-Agent has
//! no contact email, so the gap stays at a second.

use crate::requests::http::{self, base_url, UpstreamError};
use crate::requests::languages;
use crate::requests::matching::{
    isbn13s, precolon_names_a_sibling, without_this_work, WorkContext,
};
use crate::AppState;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

const CACHE_FOR: Duration = Duration::from_secs(24 * 60 * 60);
const INFO_CAP: usize = 500;
const NEGATIVE_FOR: Duration = Duration::from_secs(60);
const MISSING_FOR: Duration = Duration::from_secs(60 * 60);
const FRESH_FOR: Duration = Duration::from_secs(6 * 60 * 60);
const STALE_FOR: Duration = Duration::from_secs(48 * 60 * 60);
/// How long a trending failure suppresses another upstream call.
const FAILURE_FOR: Duration = Duration::from_secs(60);
const DESCRIPTION_CAP: usize = 2000;
pub(crate) const MISSING_WORK: &str = "Open Library has no record of this work.";
pub(crate) const LIBRARY_DOWN: &str = "Open Library is not answering.";
pub(crate) const LIBRARY_AGAIN: &str = "Open Library is not answering. Try again.";

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

#[derive(Debug, Clone)]
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

/// Weekly trending. Fresh for 6h. Older than that, but under 48h, is served
/// immediately while one refresh runs. Empty or older than 48h is fetched
/// inline, and a failure there is the caller's 502. That failure is remembered
/// for a minute: a cold or expired cache returns it with no new fetch, and a
/// stale cache skips the refresh.
pub(crate) struct TrendingCache {
    slot: Mutex<Option<Trending>>,
    refreshing: AtomicBool,
    /// Last upstream failure. Lock `slot` before `failed` when both are held.
    failed: Mutex<Option<(Instant, UpstreamError)>>,
}

struct Trending {
    at: Instant,
    works: Arc<Vec<TrendingWork>>,
}

#[derive(Clone)]
pub(crate) struct TrendingWork {
    pub work: Work,
    pub languages: Vec<String>,
}

impl TrendingCache {
    pub(crate) fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            refreshing: AtomicBool::new(false),
            failed: Mutex::new(None),
        }
    }

    async fn recent_failure(&self) -> Option<UpstreamError> {
        let guard = self.failed.lock().await;
        let (at, err) = guard.as_ref()?;
        if Instant::now().saturating_duration_since(*at) < FAILURE_FOR {
            Some(err.clone())
        } else {
            None
        }
    }

    async fn serve(
        self: Arc<Self>,
        state: &AppState,
    ) -> Result<Arc<Vec<TrendingWork>>, UpstreamError> {
        {
            let guard = self.slot.lock().await;
            if let Some(hit) = guard.as_ref() {
                match age(hit.at, Instant::now()) {
                    Age::Fresh => return Ok(Arc::clone(&hit.works)),
                    Age::Stale => {
                        let works = Arc::clone(&hit.works);
                        drop(guard);
                        if self.recent_failure().await.is_none() {
                            Arc::clone(&self).spawn_refresh(state);
                        }
                        return Ok(works);
                    }
                    Age::Expired => {}
                }
            }
        }
        // Empty or older than 48h. Hold the mutex so the waiters share one fetch.
        let mut guard = self.slot.lock().await;
        if let Some(hit) = guard.as_ref() {
            match age(hit.at, Instant::now()) {
                Age::Fresh => return Ok(Arc::clone(&hit.works)),
                Age::Stale => {
                    let works = Arc::clone(&hit.works);
                    drop(guard);
                    if self.recent_failure().await.is_none() {
                        self.spawn_refresh(state);
                    }
                    return Ok(works);
                }
                Age::Expired => {}
            }
        }
        if let Some(err) = self.recent_failure().await {
            return Err(err);
        }
        match fetch_trending(state).await {
            Ok(works) => {
                let works = Arc::new(works);
                *guard = Some(Trending {
                    at: Instant::now(),
                    works: Arc::clone(&works),
                });
                *self.failed.lock().await = None;
                Ok(works)
            }
            Err(err) => {
                *self.failed.lock().await = Some((Instant::now(), err.clone()));
                Err(err)
            }
        }
    }

    fn spawn_refresh(self: Arc<Self>, state: &AppState) {
        if self.refreshing.swap(true, Ordering::AcqRel) {
            return;
        }
        let state = state.clone();
        tokio::spawn(async move {
            match fetch_trending(&state).await {
                Ok(works) => {
                    let mut guard = self.slot.lock().await;
                    *guard = Some(Trending {
                        at: Instant::now(),
                        works: Arc::new(works),
                    });
                    *self.failed.lock().await = None;
                }
                Err(err) => {
                    tracing::warn!(error = %err, "open library trending refresh failed");
                    *self.failed.lock().await = Some((Instant::now(), err));
                }
            }
            self.refreshing.store(false, Ordering::Release);
        });
    }
}

pub(crate) async fn trending(state: &AppState) -> Result<Arc<Vec<TrendingWork>>, UpstreamError> {
    Arc::clone(&state.trending).serve(state).await
}

/// Filtered at serve time: the language setting can change while the list is cached.
pub(crate) fn browse_page(
    works: &[TrendingWork],
    page: i64,
    accepted: &[String],
) -> (Vec<Work>, Option<i64>) {
    let shown: Vec<&TrendingWork> = works.iter().filter(|work| kept(work, accepted)).collect();
    let page = page.max(1);
    let start = (page as usize - 1).saturating_mul(PAGE as usize);
    if start >= shown.len() {
        return (Vec::new(), None);
    }
    let end = (start + PAGE as usize).min(shown.len());
    let next = if end < shown.len() {
        Some(page + 1)
    } else {
        None
    };
    let page_works = shown[start..end]
        .iter()
        .map(|work| work.work.clone())
        .collect();
    (page_works, next)
}

fn kept(work: &TrendingWork, accepted: &[String]) -> bool {
    work.languages.is_empty()
        || work
            .languages
            .iter()
            .any(|code| languages::accepts(accepted, code))
}

async fn fetch_trending(state: &AppState) -> Result<Vec<TrendingWork>, UpstreamError> {
    state.catalog_limit.pace().await;
    let base = base_url(&state.openlibrary_url)?;
    let body: OlTrending = state
        .upstream
        .get_json(
            &base,
            "/trending/weekly.json",
            &[
                ("limit", "100"),
                (
                    "fields",
                    "key,title,author_name,first_publish_year,cover_i,language",
                ),
            ],
            &[],
            2 * 1024 * 1024,
        )
        .await?;
    Ok(body.works.into_iter().filter_map(trending_work).collect())
}

fn trending_work(doc: TrendDoc) -> Option<TrendingWork> {
    let work = work_from_parts(
        doc.key,
        doc.title,
        doc.author_name,
        doc.first_publish_year,
        doc.cover_i,
    )?;
    Some(TrendingWork {
        work,
        languages: doc.language,
    })
}

fn work_from_parts(
    key: String,
    title: String,
    author_name: Vec<String>,
    year: Option<i64>,
    cover_id: Option<i64>,
) -> Option<Work> {
    if !key.starts_with("/works/") || title.trim().is_empty() {
        return None;
    }
    Some(Work {
        work_key: key,
        title: title.trim().to_string(),
        author: author_name
            .into_iter()
            .find(|name| !name.trim().is_empty())
            .map(|name| name.trim().to_string())
            .unwrap_or_else(|| "Unknown".into()),
        year,
        cover_id: cover_id.filter(|id| *id >= 0),
    })
}

enum Age {
    Fresh,
    Stale,
    Expired,
}

fn age(at: Instant, now: Instant) -> Age {
    let elapsed = now.saturating_duration_since(at);
    if elapsed < FRESH_FOR {
        Age::Fresh
    } else if elapsed < STALE_FOR {
        Age::Stale
    } else {
        Age::Expired
    }
}

#[derive(Clone)]
enum InfoEntry {
    Ready(Box<super::WorkInfo>),
    Failed(String),
}

pub(crate) struct InfoCache {
    inner: TtlCache<InfoEntry>,
}

impl InfoCache {
    pub(crate) fn new() -> Self {
        Self {
            inner: TtlCache::new(INFO_CAP),
        }
    }
}

/// Work page for the catalog sheet. A miss is cached for a minute so the
/// sheet's poll cannot hammer Open Library. A 404 is cached for an hour.
/// The caller still returns 200 with `info: null`.
pub(crate) async fn work_info(state: &AppState, work_key: &str) -> Result<super::WorkInfo, String> {
    let cache = Arc::clone(&state.info_cache);
    let entry = cache
        .inner
        .get_or_load(work_key, || assemble_info(state, work_key))
        .await;
    match entry {
        InfoEntry::Ready(info) => Ok(*info),
        InfoEntry::Failed(message) => Err(message),
    }
}

/// The cached flag is from fetch time. Recompute so a language change applies
/// before the 24h entry expires. Display names still pass [`languages::accepts`].
pub(crate) fn mark_accepted(info: &mut super::WorkInfo, accepted: &[String]) {
    info.has_accepted_edition = info.languages.is_empty()
        || info
            .languages
            .iter()
            .any(|name| languages::accepts(accepted, name));
}

async fn assemble_info(state: &AppState, work_key: &str) -> (InfoEntry, Duration) {
    match load_info(state, work_key).await {
        Ok(info) => (InfoEntry::Ready(Box::new(info)), CACHE_FOR),
        Err(InfoFail::Missing) => (InfoEntry::Failed(MISSING_WORK.into()), MISSING_FOR),
        Err(InfoFail::Down) => (InfoEntry::Failed(LIBRARY_DOWN.into()), NEGATIVE_FOR),
    }
}

enum InfoFail {
    Missing,
    Down,
}

async fn load_info(state: &AppState, work_key: &str) -> Result<super::WorkInfo, InfoFail> {
    let (canonical, body) = fetch_work_body(state, work_key).await?;
    let doc = fetch_detail(state, &canonical).await?;
    Ok(build_info(&canonical, &body, doc))
}

async fn fetch_work_body(
    state: &AppState,
    work_key: &str,
) -> Result<(String, serde_json::Value), InfoFail> {
    state.catalog_limit.pace().await;
    let base = base_url(&state.openlibrary_url).map_err(|_| InfoFail::Down)?;
    let first = state
        .upstream
        .work_document(&base, work_key)
        .await
        .map_err(work_fail)?;
    let (canonical, body) = match first {
        http::RawWork::Body(value) => match json_redirect(&value) {
            Some(next) if next != work_key => (next, None),
            _ => (work_key.to_string(), Some(value)),
        },
        http::RawWork::Redirect(Some(next)) if next != work_key => (next, None),
        http::RawWork::Redirect(_) => return Err(InfoFail::Down),
    };
    let Some(body) = body else {
        state.catalog_limit.pace().await;
        let second = state
            .upstream
            .work_document(&base, &canonical)
            .await
            .map_err(work_fail)?;
        let http::RawWork::Body(value) = second else {
            return Err(InfoFail::Down);
        };
        // One hop. A second redirect is not a document we can show.
        if json_redirect(&value).is_some() {
            return Err(InfoFail::Down);
        }
        return Ok((canonical, value));
    };
    if json_redirect(&body).is_some() {
        return Err(InfoFail::Down);
    }
    Ok((canonical, body))
}

fn work_fail(err: UpstreamError) -> InfoFail {
    match err {
        UpstreamError::Status(404) => InfoFail::Missing,
        err => {
            tracing::warn!(error = %err, "open library work document failed");
            InfoFail::Down
        }
    }
}

/// `/type/redirect` only. Any other document, including one with a stray
/// `location`, is the work itself.
fn json_redirect(value: &serde_json::Value) -> Option<String> {
    let type_key = value
        .pointer("/type/key")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if type_key != "/type/redirect" {
        return None;
    }
    let location = value
        .get("location")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    http::work_key_from_location(location)
}

async fn fetch_detail(state: &AppState, canonical: &str) -> Result<DetailDoc, InfoFail> {
    state.catalog_limit.pace().await;
    let base = base_url(&state.openlibrary_url).map_err(|_| InfoFail::Down)?;
    let q = format!("key:{canonical}");
    let body: Search<DetailDoc> = state
        .upstream
        .get_json(
            &base,
            "/search.json",
            &[
                ("q", q.as_str()),
                (
                    "fields",
                    "key,title,subtitle,author_name,first_publish_year,cover_i,edition_count,number_of_pages_median,language,ratings_average,ratings_count",
                ),
            ],
            &[],
            2 * 1024 * 1024,
        )
        .await
        .map_err(|err| {
            tracing::warn!(error = %err, "open library work search failed");
            InfoFail::Down
        })?;
    Ok(body.docs.into_iter().next().unwrap_or_default())
}

fn build_info(canonical: &str, body: &serde_json::Value, doc: DetailDoc) -> super::WorkInfo {
    let work_title = text_field(body.get("title"));
    let title = if work_title.is_empty() {
        doc.title.trim().to_string()
    } else {
        work_title
    };
    let subtitle = doc
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|subtitle| !subtitle.is_empty())
        .map(str::to_string);
    let authors: Vec<String> = doc
        .author_name
        .into_iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    let cover_id = cover_list(body.get("covers")).or(doc.cover_i.filter(|id| *id >= 0));
    let (subjects, awards) = subjects_and_awards(&string_values(body.get("subjects")));
    super::WorkInfo {
        canonical_key: canonical.to_string(),
        title,
        subtitle,
        authors,
        year: doc.first_publish_year,
        cover_id,
        description: body.get("description").and_then(description_text),
        subjects,
        awards,
        series: string_values(body.get("series")),
        edition_count: doc.edition_count,
        pages_median: doc.number_of_pages_median,
        languages: display_languages(&doc.language),
        has_accepted_edition: false,
        rating: rating(doc.ratings_average, doc.ratings_count),
    }
}

fn text_field(value: Option<&serde_json::Value>) -> String {
    value
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn cover_list(value: Option<&serde_json::Value>) -> Option<i64> {
    value.and_then(|item| item.as_array()).and_then(|items| {
        items
            .iter()
            .find_map(|item| item.as_i64().filter(|id| *id >= 0))
    })
}

fn description_text(value: &serde_json::Value) -> Option<String> {
    let raw = match value {
        serde_json::Value::String(text) => text.as_str(),
        serde_json::Value::Object(map) => map.get("value").and_then(|item| item.as_str())?,
        _ => return None,
    };
    let plain = plain_description(raw);
    if plain.is_empty() {
        None
    } else {
        Some(plain)
    }
}

fn plain_description(raw: &str) -> String {
    strip_markdown_links(raw)
        .trim()
        .chars()
        .take(DESCRIPTION_CAP)
        .collect()
}

fn strip_markdown_links(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let start = if chars[i] == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            i + 1
        } else {
            i
        };
        if chars[start] == '[' {
            if let Some((label, next)) = markdown_link(&chars, start) {
                out.push_str(&label);
                i = next;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn markdown_link(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut i = start + 1;
    let mut label = String::new();
    while i < chars.len() && chars[i] != ']' {
        label.push(chars[i]);
        i += 1;
    }
    if i >= chars.len() || i + 1 >= chars.len() || chars[i] != ']' || chars[i + 1] != '(' {
        return None;
    }
    i += 2;
    let url_start = i;
    while i < chars.len() && chars[i] != ')' && chars[i] != ' ' && chars[i] != '\n' {
        i += 1;
    }
    if i >= chars.len() || chars[i] != ')' || i == url_start {
        return None;
    }
    Some((label, i + 1))
}

fn subjects_and_awards(raw: &[String]) -> (Vec<String>, Vec<String>) {
    let mut subjects = Vec::new();
    let mut awards = Vec::new();
    let mut seen_subjects = HashSet::new();
    let mut seen_awards = HashSet::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("nyt:") {
            continue;
        }
        if lower.starts_with("award:") {
            if let Some(award) = award_name(trimmed) {
                if seen_awards.insert(award.to_ascii_lowercase()) {
                    awards.push(award);
                }
            }
            continue;
        }
        if lower.starts_with("fiction,") {
            continue;
        }
        if subjects.len() < 8 && seen_subjects.insert(lower) {
            subjects.push(trimmed.to_string());
        }
    }
    (subjects, awards)
}

fn award_name(raw: &str) -> Option<String> {
    let rest = raw.split_once(':')?.1.trim();
    if rest.is_empty() {
        return None;
    }
    let (name, year) = match rest.split_once('=') {
        Some((name, year)) => (
            name.trim(),
            Some(year.trim()).filter(|year| !year.is_empty()),
        ),
        None => (rest, None),
    };
    let pretty = name
        .split('_')
        .filter(|word| !word.is_empty())
        .map(title_word)
        .collect::<Vec<_>>()
        .join(" ");
    if pretty.is_empty() {
        return None;
    }
    Some(match year {
        Some(year) => format!("{pretty} {year}"),
        None => pretty,
    })
}

fn title_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().collect();
    out.extend(chars.flat_map(|ch| ch.to_lowercase()));
    out
}

fn string_values(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text.to_string()]
            }
        }
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn display_languages(codes: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for code in codes {
        let name = language_name(code);
        if name.is_empty() {
            continue;
        }
        if seen.insert(name.to_ascii_lowercase()) && out.len() < 40 {
            out.push(name);
        }
    }
    out
}

fn language_name(code: &str) -> String {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    let primary = lower.split(['-', '_']).next().unwrap_or(lower.as_str());
    for language in languages::LANGUAGES {
        if language.code == primary
            || language.aliases.contains(&primary)
            || language.word == lower
            || language.name.eq_ignore_ascii_case(trimmed)
        {
            return language.name.to_string();
        }
    }
    trimmed.to_string()
}

fn rating(average: Option<f64>, count: Option<i64>) -> Option<super::Rating> {
    let count = count.filter(|count| *count >= 5)?;
    let average = average.filter(|average| average.is_finite())?;
    Some(super::Rating {
        average: (average * 10.0).round() / 10.0,
        count,
    })
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
    let (canonical, ctx) = fetch_context(state, work_key).await?;
    state.work_cache.insert(work_key, &canonical, ctx.clone());
    Ok(ctx)
}

#[derive(Clone)]
struct CachedWork {
    canonical: String,
    ctx: WorkContext,
}

pub(crate) struct WorkCache {
    inner: TtlCache<CachedWork>,
}

impl WorkCache {
    pub(crate) fn new() -> Self {
        Self {
            inner: TtlCache::new(usize::MAX),
        }
    }

    fn get(&self, key: &str) -> Option<WorkContext> {
        self.inner.get(key).map(|hit| hit.ctx)
    }

    /// Canonical work key when this process already fetched `asked` and followed
    /// a redirect. A miss does not call Open Library.
    pub(crate) fn cached_canonical(&self, asked: &str) -> Option<String> {
        self.inner.get(asked).map(|hit| hit.canonical)
    }

    fn insert(&self, key: &str, canonical: &str, ctx: WorkContext) {
        self.inner.insert(
            key,
            CachedWork {
                canonical: canonical.to_string(),
                ctx,
            },
            CACHE_FOR,
        );
    }

    #[cfg(test)]
    fn remember(&self, key: &str, canonical: &str, ctx: WorkContext, at: Instant, now: Instant) {
        self.inner.insert_at(
            key,
            CachedWork {
                canonical: canonical.to_string(),
                ctx,
            },
            at,
            CACHE_FOR,
            now,
        );
    }
}

struct Slot<V> {
    seq: u64,
    until: Instant,
    value: V,
}

struct Store<V> {
    next_seq: u64,
    entries: HashMap<String, Slot<V>>,
}

struct Flight<V> {
    id: u64,
    tx: tokio::sync::watch::Sender<Option<V>>,
}

/// Clears the in-flight entry when the leader is dropped mid-load. Waiters then
/// see a closed watch channel and load the key themselves.
struct FlightGuard<'a, V> {
    flights: &'a std::sync::Mutex<HashMap<String, Flight<V>>>,
    key: &'a str,
    id: u64,
}

impl<V> Drop for FlightGuard<'_, V> {
    fn drop(&mut self) {
        let mut flights = self.flights.lock().unwrap_or_else(|err| err.into_inner());
        let ours = flights
            .get(self.key)
            .is_some_and(|flight| flight.id == self.id);
        if ours {
            flights.remove(self.key);
        }
    }
}

/// In-memory TTL map. Insert drops expired rows, then the oldest ones past `cap`.
/// [`TtlCache::get_or_load`] runs one load per key; the other callers wait for it.
/// Dropping the leader removes that flight, so a cancelled caller does not leave
/// the key stuck.
pub(crate) struct TtlCache<V> {
    cap: usize,
    store: std::sync::Mutex<Store<V>>,
    flights: std::sync::Mutex<HashMap<String, Flight<V>>>,
    next_flight: std::sync::atomic::AtomicU64,
}

impl<V> TtlCache<V>
where
    V: Clone + Send + Sync + 'static,
{
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            cap,
            store: std::sync::Mutex::new(Store {
                next_seq: 0,
                entries: HashMap::new(),
            }),
            flights: std::sync::Mutex::new(HashMap::new()),
            next_flight: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<V> {
        let mut store = self.store.lock().unwrap_or_else(|err| err.into_inner());
        let now = Instant::now();
        let stale = store.entries.get(key).is_some_and(|slot| slot.until <= now);
        if stale {
            store.entries.remove(key);
            return None;
        }
        store.entries.get(key).map(|slot| slot.value.clone())
    }

    pub(crate) fn insert(&self, key: &str, value: V, ttl: Duration) {
        let now = Instant::now();
        self.insert_at(key, value, now, ttl, now);
    }

    fn insert_at(&self, key: &str, value: V, at: Instant, ttl: Duration, now: Instant) {
        let mut store = self.store.lock().unwrap_or_else(|err| err.into_inner());
        store.entries.retain(|_, slot| slot.until > now);
        store.next_seq = store.next_seq.saturating_add(1);
        let seq = store.next_seq;
        store.entries.insert(
            key.to_string(),
            Slot {
                seq,
                until: at.checked_add(ttl).unwrap_or(at),
                value,
            },
        );
        while store.entries.len() > self.cap {
            let oldest = store
                .entries
                .iter()
                .min_by_key(|(_, slot)| slot.seq)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                break;
            };
            store.entries.remove(&oldest);
        }
    }

    /// One `load` per key. Waiters get that value. `load` returns the ttl to store.
    /// The load must not call [`Self::get_or_load`] for the same key.
    pub(crate) async fn get_or_load<F, Fut>(&self, key: &str, load: F) -> V
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = (V, Duration)>,
    {
        let mut load = Some(load);
        loop {
            if let Some(hit) = self.get(key) {
                return hit;
            }
            let mut lead = None;
            let mut rx = {
                let mut flights = self.flights.lock().unwrap_or_else(|err| err.into_inner());
                if let Some(hit) = self.get(key) {
                    return hit;
                }
                if let Some(flight) = flights.get(key) {
                    Some(flight.tx.subscribe())
                } else {
                    let (tx, rx) = tokio::sync::watch::channel(None);
                    let id = self.next_flight.fetch_add(1, Ordering::Relaxed);
                    flights.insert(key.to_string(), Flight { id, tx });
                    // Waiters subscribe on the sender. This receiver would only
                    // observe the value the leader is about to write.
                    drop(rx);
                    lead = Some(id);
                    None
                }
            };
            if let Some(rx) = rx.as_mut() {
                loop {
                    if let Some(value) = rx.borrow().clone() {
                        return value;
                    }
                    // The leader was dropped, or it finished and closed the
                    // channel after publishing. Either way, try the cache again.
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
                continue;
            }
            let guard = FlightGuard {
                flights: &self.flights,
                key,
                id: lead.expect("the leader records its flight id"),
            };
            let (value, ttl) = (load.take().expect("one load"))().await;
            self.insert(key, value.clone(), ttl);
            {
                let mut flights = self.flights.lock().unwrap_or_else(|err| err.into_inner());
                let ours = flights.get(key).is_some_and(|flight| flight.id == guard.id);
                if ours {
                    if let Some(flight) = flights.remove(key) {
                        let _ = flight.tx.send(Some(value.clone()));
                    }
                }
            }
            drop(guard);
            return value;
        }
    }
}

async fn fetch_context(
    state: &AppState,
    work_key: &str,
) -> Result<(String, WorkContext), UpstreamError> {
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
    let mut canonical = work_key.to_string();
    let doc = match body
        .docs
        .into_iter()
        .find(|doc| !doc.title.trim().is_empty())
    {
        Some(doc) => doc,
        None => {
            let Some(next) = merged_work(state, &base, work_key).await? else {
                return Err(UpstreamError::Shape);
            };
            canonical = next;
            state.catalog_limit.pace().await;
            let q = format!("key:{canonical}");
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
            body.docs
                .into_iter()
                .find(|doc| !doc.title.trim().is_empty())
                .ok_or(UpstreamError::Shape)?
        }
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
    Ok((canonical, assemble(doc, siblings)))
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

/// One `/works/{id}.json`. A redirect's `location` is the canonical work key.
/// Anything else is a miss; the caller does not walk a chain.
async fn merged_work(
    state: &AppState,
    base: &reqwest::Url,
    work_key: &str,
) -> Result<Option<String>, UpstreamError> {
    state.catalog_limit.pace().await;
    let Some(next) = state.upstream.work_location(base, work_key).await? else {
        return Ok(None);
    };
    if next == work_key {
        return Ok(None);
    }
    Ok(Some(next))
}

fn assemble(doc: WorkDoc, siblings: Vec<String>) -> WorkContext {
    let title = doc.title.trim();
    let mut primary = Vec::new();
    let push = |list: &mut Vec<String>, value: &str| {
        let value = value.trim();
        if value.is_empty() || list.iter().any(|have: &String| have == value) {
            return;
        }
        list.push(value.to_string());
    };
    push(&mut primary, title);
    if let Some(subtitle) = doc
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|subtitle| !subtitle.is_empty())
    {
        push(&mut primary, &format!("{title}: {subtitle}"));
    }
    for alt in &doc.alternative_title {
        push(&mut primary, alt);
    }
    let mut titles = primary.clone();
    if let Some((head, tail)) = title.split_once(':') {
        if !tail.trim().is_empty() && !precolon_names_a_sibling(head, &siblings) {
            push(&mut titles, head);
        }
    }
    let siblings = without_this_work(siblings, &primary);
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

#[derive(Debug, Deserialize)]
struct OlTrending {
    #[serde(default)]
    works: Vec<TrendDoc>,
}

#[derive(Debug, Deserialize)]
struct TrendDoc {
    #[serde(default)]
    key: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author_name: Vec<String>,
    #[serde(default, deserialize_with = "loose_i64")]
    first_publish_year: Option<i64>,
    #[serde(default, deserialize_with = "loose_i64")]
    cover_i: Option<i64>,
    #[serde(default, deserialize_with = "string_list")]
    language: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DetailDoc {
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    author_name: Vec<String>,
    #[serde(default, deserialize_with = "loose_i64")]
    first_publish_year: Option<i64>,
    #[serde(default, deserialize_with = "loose_i64")]
    cover_i: Option<i64>,
    #[serde(default, deserialize_with = "loose_i64")]
    edition_count: Option<i64>,
    #[serde(default, deserialize_with = "loose_i64")]
    number_of_pages_median: Option<i64>,
    #[serde(default, deserialize_with = "string_list")]
    language: Vec<String>,
    #[serde(default)]
    ratings_average: Option<f64>,
    #[serde(default, deserialize_with = "loose_i64")]
    ratings_count: Option<i64>,
}

/// Open Library mixes ints and one-decimal medians in the same fields.
fn loose_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(number) => {
            if let Some(n) = number.as_i64() {
                Ok(Some(n))
            } else if let Some(n) = number.as_f64() {
                if n.is_finite() {
                    Ok(Some(n.round() as i64))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
        _ => Err(serde::de::Error::custom("number")),
    }
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

    #[test]
    fn a_precolon_fragment_that_names_a_sibling_is_not_a_title() {
        let doc = WorkDoc {
            title: "Mistborn: Secret History".into(),
            subtitle: None,
            alternative_title: vec![],
            author_key: vec!["OL1A".into()],
            author_name: vec!["Brandon Sanderson".into()],
            isbn: vec![],
        };
        let ctx = assemble(doc, vec!["Mistborn".into(), "The Final Empire".into()]);
        assert_eq!(ctx.titles, vec!["Mistborn: Secret History".to_string()]);
        assert_eq!(
            ctx.siblings,
            vec!["Mistborn".to_string(), "The Final Empire".to_string()]
        );
    }

    #[test]
    fn the_cache_drops_entries_older_than_a_day() {
        let cache = WorkCache::new();
        let ctx = WorkContext {
            titles: vec!["Dune".into()],
            authors: vec![],
            isbns: HashSet::new(),
            siblings: vec![],
        };
        let then = Instant::now();
        cache.remember("old", "/works/OL1W", ctx.clone(), then, then);
        let later = then + CACHE_FOR + Duration::from_secs(1);
        cache.remember("new", "/works/OL2W", ctx, later, later);
        assert!(cache.get("old").is_none());
        assert!(cache.get("new").is_some());
        assert_eq!(
            cache.cached_canonical("new").as_deref(),
            Some("/works/OL2W")
        );
        assert!(cache.cached_canonical("missing").is_none());
    }

    #[test]
    fn descriptions_drop_links_and_stop_at_2000_chars() {
        let linked = description_text(&serde_json::json!(
            "See [Arrakis](https://example.com/a) and ![map](https://example.com/m)."
        ))
        .unwrap();
        assert_eq!(linked, "See Arrakis and map.");
        let wrapped = description_text(&serde_json::json!({
            "value": "  A [desert](https://example.com) planet.  "
        }))
        .unwrap();
        assert_eq!(wrapped, "A desert planet.");
        assert!(description_text(&serde_json::json!({ "value": 1 })).is_none());
        let long = "a".repeat(DESCRIPTION_CAP + 25);
        assert_eq!(plain_description(&long).chars().count(), DESCRIPTION_CAP);
    }

    #[test]
    fn subjects_drop_nyt_awards_and_fiction_prefixes() {
        let raw = vec![
            "Fiction, general".into(),
            "Dune".into(),
            "award:hugo_award=1966".into(),
            "nyt:hardcover".into(),
            "dune".into(),
            "Arrakis".into(),
            "award:nebula_award".into(),
            "award:".into(),
            "One".into(),
            "Two".into(),
            "Three".into(),
            "Four".into(),
            "Five".into(),
            "Six".into(),
            "Seven".into(),
        ];
        let (subjects, awards) = subjects_and_awards(&raw);
        assert_eq!(
            subjects,
            vec![
                "Dune".to_string(),
                "Arrakis".to_string(),
                "One".to_string(),
                "Two".to_string(),
                "Three".to_string(),
                "Four".to_string(),
                "Five".to_string(),
                "Six".to_string(),
            ]
        );
        assert_eq!(
            awards,
            vec!["Hugo Award 1966".to_string(), "Nebula Award".to_string()]
        );
    }

    #[test]
    fn rating_needs_five_votes_and_one_decimal() {
        assert!(rating(Some(4.25), Some(4)).is_none());
        assert!(rating(Some(f64::NAN), Some(10)).is_none());
        let shown = rating(Some(4.25), Some(448)).unwrap();
        assert_eq!(shown.average, 4.3);
        assert_eq!(shown.count, 448);
    }

    #[test]
    fn browse_page_filters_languages_and_pages_by_20() {
        let mut works = Vec::new();
        works.push(TrendingWork {
            work: sample_work("/works/OL1W", "German only"),
            languages: vec!["ger".into()],
        });
        works.push(TrendingWork {
            work: sample_work("/works/OL2W", "No language"),
            languages: vec![],
        });
        for n in 0..21 {
            works.push(TrendingWork {
                work: sample_work(&format!("/works/OL{}W", 100 + n), &format!("En {n}")),
                languages: vec!["eng".into()],
            });
        }
        let en = vec!["en".into()];
        let (page, next) = browse_page(&works, 1, &en);
        assert_eq!(page.len(), 20);
        assert_eq!(next, Some(2));
        assert!(page.iter().all(|work| work.title != "German only"));
        let (rest, next) = browse_page(&works, 2, &en);
        assert_eq!(rest.len(), 2);
        assert!(next.is_none());
        let both = vec!["en".into(), "de".into()];
        let (page, _) = browse_page(&works, 1, &both);
        assert_eq!(page[0].title, "German only");
        let (rest, next) = browse_page(&works, 2, &both);
        assert_eq!(rest.len(), 3);
        assert!(next.is_none());
    }

    #[test]
    fn accepted_edition_follows_the_current_setting() {
        let mut info = build_info(
            "/works/OL1W",
            &serde_json::json!({ "title": "Dune" }),
            DetailDoc {
                language: vec!["ger".into()],
                ..DetailDoc::default()
            },
        );
        mark_accepted(&mut info, &["en".into()]);
        assert!(!info.has_accepted_edition);
        assert_eq!(info.languages, vec!["German".to_string()]);
        mark_accepted(&mut info, &["de".into()]);
        assert!(info.has_accepted_edition);
        info.languages.clear();
        mark_accepted(&mut info, &["en".into()]);
        assert!(info.has_accepted_edition);
    }

    fn sample_work(key: &str, title: &str) -> Work {
        Work {
            work_key: key.into(),
            title: title.into(),
            author: "Author".into(),
            year: None,
            cover_id: None,
        }
    }

    #[tokio::test]
    async fn ttl_cache_expires_evicts_and_single_flights() {
        let cache = TtlCache::new(2);
        let now = Instant::now();
        cache.insert_at("a", 1, now, Duration::from_secs(10), now);
        cache.insert_at("b", 2, now, Duration::from_secs(10), now);
        cache.insert_at("c", 3, now, Duration::from_secs(10), now);
        assert!(cache.get("a").is_none());
        assert_eq!(cache.get("b"), Some(2));
        assert_eq!(cache.get("c"), Some(3));

        let past = now - Duration::from_secs(30);
        cache.insert_at("old", 9, past, Duration::from_secs(1), now);
        assert!(cache.get("old").is_none());
        assert_eq!(cache.get("c"), Some(3));

        let cache = Arc::new(TtlCache::new(10));
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (tx, rx) = tokio::sync::watch::channel(false);
        let spawn = |cache: Arc<TtlCache<i32>>,
                     fetches: Arc<std::sync::atomic::AtomicUsize>,
                     rx: tokio::sync::watch::Receiver<bool>| {
            tokio::spawn(async move {
                cache
                    .get_or_load("k", || {
                        let fetches = Arc::clone(&fetches);
                        let mut rx = rx.clone();
                        async move {
                            fetches.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            while !*rx.borrow() {
                                if rx.changed().await.is_err() {
                                    break;
                                }
                            }
                            (7, Duration::from_secs(60))
                        }
                    })
                    .await
            })
        };
        let first = spawn(Arc::clone(&cache), Arc::clone(&fetches), rx.clone());
        let second = spawn(cache, Arc::clone(&fetches), rx);
        let start = Instant::now();
        while fetches.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            if start.elapsed() > Duration::from_secs(2) {
                panic!("load did not start");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
        tx.send(true).unwrap();
        assert_eq!(first.await.unwrap(), 7);
        assert_eq!(second.await.unwrap(), 7);
        assert_eq!(fetches.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_cancelled_load_does_not_stick_the_flight() {
        let cache = Arc::new(TtlCache::new(4));
        let slow = Arc::clone(&cache);
        let first = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_millis(50),
                slow.get_or_load("k", || async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    (1, Duration::from_secs(60))
                }),
            )
            .await
        });
        assert!(first.await.unwrap().is_err());
        let second = tokio::time::timeout(Duration::from_millis(200), async {
            cache
                .get_or_load("k", || async { (2, Duration::from_secs(60)) })
                .await
        })
        .await;
        assert_eq!(second.expect("second load hung on the abandoned flight"), 2);
    }

    #[tokio::test]
    async fn stale_trending_serves_the_old_list_when_refresh_fails() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::clone(&hits);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/trending/weekly.json",
                axum::routing::get(move || {
                    let seen = Arc::clone(&seen);
                    async move {
                        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    }
                }),
            );
            axum::serve(listener, app).await.unwrap();
        });
        let data = tempfile::TempDir::new().unwrap();
        let state = crate::prepare(crate::config::Config {
            listen: "127.0.0.1:0".into(),
            data_dir: data.path().to_path_buf(),
            web_dir: None,
            allow_signup: true,
            max_epub_bytes: 64 * 1024 * 1024,
            downloads_dir: None,
            openlibrary_url: origin,
            auth: crate::config::AuthConfig::default(),
        })
        .await
        .unwrap();
        let old = Arc::new(vec![TrendingWork {
            work: sample_work("/works/OL1W", "Old"),
            languages: vec![],
        }]);
        {
            let mut slot = state.trending.slot.lock().await;
            *slot = Some(Trending {
                at: Instant::now(),
                works: Arc::clone(&old),
            });
        }
        let fresh = trending(&state).await.unwrap();
        assert_eq!(fresh[0].work.title, "Old");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);

        {
            let mut slot = state.trending.slot.lock().await;
            slot.as_mut().unwrap().at = Instant::now() - FRESH_FOR - Duration::from_secs(1);
        }
        let served = trending(&state).await.unwrap();
        assert_eq!(served[0].work.title, "Old");
        let start = Instant::now();
        while hits.load(std::sync::atomic::Ordering::SeqCst) < 1 {
            if start.elapsed() > Duration::from_secs(5) {
                panic!("stale refresh did not run");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let slot = state.trending.slot.lock().await;
            assert_eq!(slot.as_ref().unwrap().works[0].work.title, "Old");
        }

        let again = trending(&state).await.unwrap();
        assert_eq!(again[0].work.title, "Old");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        {
            let mut slot = state.trending.slot.lock().await;
            slot.as_mut().unwrap().at = Instant::now() - STALE_FOR - Duration::from_secs(1);
        }
        assert!(trending(&state).await.is_err());
        {
            let slot = state.trending.slot.lock().await;
            assert_eq!(slot.as_ref().unwrap().works[0].work.title, "Old");
        }
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        {
            let mut failed = state.trending.failed.lock().await;
            let (at, _) = failed.as_mut().expect("the refresh failure was recorded");
            *at = Instant::now() - FAILURE_FOR - Duration::from_secs(1);
        }
        assert!(trending(&state).await.is_err());
        let slot = state.trending.slot.lock().await;
        assert_eq!(slot.as_ref().unwrap().works[0].work.title, "Old");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
