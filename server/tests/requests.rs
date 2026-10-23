//! Book requests against in-process Prowlarr, SABnzbd, and Open Library.

mod common;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use http_body_util::BodyExt;
use mewo_server::config::{AuthConfig, Config, MIN_EPUB_BYTES};
use mewo_server::requests::worker;
use mewo_server::{prepare, router, AppState};
use serde_json::{json, Value};
use sqlx::migrate::Migrate;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

const SIX_HOURS: i64 = 6 * 60 * 60 * 1000;
const DAY: i64 = 24 * 60 * 60 * 1000;
const POLL: i64 = 30 * 1000;

struct World {
    state: AppState,
    app: axum::Router,
    /// Kept so the database directory outlives the test.
    _data: TempDir,
    downloads: TempDir,
    prowlarr: Arc<Mutex<Prowl>>,
    sab: Arc<Mutex<Sab>>,
    catalog: Arc<Mutex<Catalog>>,
    token: String,
    user_id: String,
}

async fn world() -> World {
    let downloads = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let prowlarr = Arc::new(Mutex::new(Prowl::default()));
    let sab = Arc::new(Mutex::new(Sab::default()));
    let catalog = Arc::new(Mutex::new(Catalog::default()));
    let prowlarr_url = serve(prowlarr_router(prowlarr.clone())).await;
    let sab_url = serve(sab_router(sab.clone())).await;
    let library_url = serve(library_router(catalog.clone())).await;
    prowlarr.lock().await.origin = prowlarr_url.clone();
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: Some(downloads.path().to_path_buf()),
        openlibrary_url: library_url,
        auth: AuthConfig::default(),
    })
    .await
    .unwrap();
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'matt'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    grant(&state.pool, "matt").await;
    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({
            "requestsEnabled": true,
            "prowlarrUrl": prowlarr_url,
            "sabnzbdUrl": sab_url,
            "sabnzbdCategory": "books",
            "prowlarrApiKey": "prowlarr-key",
            "sabnzbdApiKey": "sab-key",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["secrets"]["prowlarrApiKey"].as_bool().unwrap());
    assert!(body.get("prowlarrApiKey").is_none(), "{body}");
    World {
        state,
        app,
        _data: data,
        downloads,
        prowlarr,
        sab,
        catalog,
        token,
        user_id,
    }
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    origin
}

#[derive(Default)]
struct Prowl {
    origin: String,
    key_ok: bool,
    queries: Vec<String>,
    indexer_ids: Vec<String>,
    releases: Vec<Value>,
    redirect: bool,
    huge: bool,
    app_name: String,
    status_code: u16,
}

impl Prowl {
    fn canned(origin: &str) -> Vec<Value> {
        vec![
            json!({ "guid": 1, "title": "not a release" }),
            release(origin, "g1", "Frank.Herbert.Dune.1965.RETAIL.EPUB", 10),
        ]
    }
}

fn release(origin: &str, guid: &str, title: &str, grabs: i64) -> Value {
    json!({
        "guid": guid,
        "indexerId": 1,
        "indexer": "NZBgeek",
        "title": title,
        "size": 80_000,
        "protocol": "usenet",
        "downloadUrl": format!("{origin}/nzb/{guid}"),
        "grabs": grabs,
        "categories": [{ "id": 7020 }]
    })
}

fn prowlarr_router(state: Arc<Mutex<Prowl>>) -> Router {
    Router::new()
        .route("/api/v1/system/status", get(prowlarr_status))
        .route("/api/v1/indexer", get(prowlarr_indexers))
        .route("/api/v1/search", get(prowlarr_search))
        .route("/followed", get(|| async { "followed" }))
        .with_state(state)
}

async fn prowlarr_key(state: &Arc<Mutex<Prowl>>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok());
    let guard = state.lock().await;
    if guard.status_code != 0 {
        return Err(StatusCode::from_u16(guard.status_code).unwrap());
    }
    if key != Some("prowlarr-key") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

async fn prowlarr_status(
    State(state): State<Arc<Mutex<Prowl>>>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    prowlarr_key(&state, &headers).await?;
    let guard = state.lock().await;
    let name = if guard.app_name.is_empty() {
        "Prowlarr"
    } else {
        guard.app_name.as_str()
    };
    Ok(Json(json!({ "appName": name, "version": "2.4.0" })))
}

async fn prowlarr_indexers(
    State(state): State<Arc<Mutex<Prowl>>>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    prowlarr_key(&state, &headers).await?;
    Ok(Json(json!([{
        "enable": true,
        "protocol": "usenet",
        "capabilities": { "categories": [{ "id": 7020 }] }
    }])))
}

async fn prowlarr_search(
    State(state): State<Arc<Mutex<Prowl>>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<axum::response::Response, StatusCode> {
    prowlarr_key(&state, &headers).await?;
    let mut guard = state.lock().await;
    guard
        .queries
        .push(query.get("query").cloned().unwrap_or_default());
    guard
        .indexer_ids
        .push(query.get("indexerIds").cloned().unwrap_or_default());
    if guard.redirect {
        return Ok(axum::response::Response::builder()
            .status(StatusCode::FOUND)
            .header("location", format!("{}/followed", guard.origin))
            .body(Body::empty())
            .unwrap());
    }
    if guard.huge {
        let body = vec![b'x'; 20 * 1024 * 1024];
        return Ok(axum::response::Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(body))
            .unwrap());
    }
    let releases = if guard.releases.is_empty() && guard.key_ok {
        Prowl::canned(&guard.origin)
    } else {
        guard.releases.clone()
    };
    Ok(Json(releases).into_response())
}

#[derive(Default)]
struct Sab {
    categories: Vec<String>,
    jobs: Vec<SabJob>,
    adds: Vec<SabAdd>,
    deletes: Vec<String>,
    /// The `archive` query value of each delete. Real SAB leaves completed
    /// files on disk either way.
    archives: Vec<String>,
    status_code: u16,
    next_id: u64,
}

struct SabJob {
    nzo: String,
    name: String,
    status: String,
    percent: u8,
    storage: String,
    fail_message: String,
    in_queue: bool,
}

struct SabAdd {
    nzo: String,
    nzbname: String,
    cat: String,
    name: String,
}

fn sab_router(state: Arc<Mutex<Sab>>) -> Router {
    Router::new().route("/api", get(sab_api)).with_state(state)
}

async fn sab_api(
    State(state): State<Arc<Mutex<Sab>>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    let mut guard = state.lock().await;
    if guard.status_code != 0 {
        return Err(StatusCode::from_u16(guard.status_code).unwrap());
    }
    if query.get("apikey").map(String::as_str) != Some("sab-key") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mode = query.get("mode").map(String::as_str).unwrap_or("");
    if query.get("name").map(String::as_str) == Some("delete") {
        let nzo = query.get("value").cloned().unwrap_or_default();
        let archive = query.get("archive").cloned().unwrap_or_default();
        guard.deletes.push(nzo.clone());
        guard.archives.push(archive.clone());
        // archive=0 removes the history row. Anything else archives it and
        // leaves it visible. Files on disk are never touched.
        if archive == "0" {
            guard.jobs.retain(|job| job.nzo != nzo);
        }
        return Ok(Json(json!({ "status": true })));
    }
    match mode {
        "version" => Ok(Json(json!({ "version": "4.5.3" }))),
        "get_cats" => {
            let categories = if guard.categories.is_empty() {
                vec!["*".into(), "books".into()]
            } else {
                guard.categories.clone()
            };
            Ok(Json(json!({ "categories": categories })))
        }
        "addurl" => {
            guard.next_id += 1;
            let nzo = format!("nzo-{}", guard.next_id);
            let name = query.get("nzbname").cloned().unwrap_or_default();
            guard.adds.push(SabAdd {
                nzo: nzo.clone(),
                nzbname: name.clone(),
                cat: query.get("cat").cloned().unwrap_or_default(),
                name: query.get("name").cloned().unwrap_or_default(),
            });
            guard.jobs.push(SabJob {
                nzo: nzo.clone(),
                name,
                status: "Downloading".into(),
                percent: 0,
                storage: String::new(),
                fail_message: String::new(),
                in_queue: true,
            });
            Ok(Json(json!({ "status": true, "nzo_ids": [nzo] })))
        }
        "queue" | "history" => {
            let search = query.get("search").map(String::as_str);
            let nzo = query.get("nzo_ids").map(String::as_str);
            let slots: Vec<Value> = guard
                .jobs
                .iter()
                .filter(|job| mode == "queue" && job.in_queue || mode == "history" && !job.in_queue)
                .filter(|job| search.is_none_or(|name| job.name == name))
                .filter(|job| nzo.is_none_or(|id| job.nzo == id))
                .map(|job| {
                    json!({
                        "nzo_id": job.nzo,
                        "filename": job.name,
                        "name": job.name,
                        "status": job.status,
                        "percentage": job.percent,
                        "storage": job.storage,
                        "fail_message": job.fail_message,
                    })
                })
                .collect();
            let key = if mode == "history" {
                "history"
            } else {
                "queue"
            };
            Ok(Json(json!({ key: { "slots": slots } })))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

#[derive(Default)]
struct Catalog {
    status: u16,
    hits: Vec<String>,
    author_key: String,
    siblings: Vec<String>,
    isbns: Vec<String>,
    /// Search for this work key returns no docs. `GET /works/{id}.json` redirects
    /// at the second key.
    merged: Option<(String, String)>,
    /// 200 `{"type":{"key":"/type/redirect"},"location":...}` instead of HTTP 302.
    json_redirect: bool,
    trending: Vec<Value>,
    trending_status: u16,
    /// Holds the trending response so two catalog callers overlap one fetch.
    trending_delay_ms: u64,
    work_docs: HashMap<String, Value>,
    detail_docs: HashMap<String, Value>,
}

fn library_router(state: Arc<Mutex<Catalog>>) -> Router {
    Router::new()
        .route("/search.json", get(library_search))
        .route("/trending/weekly.json", get(library_trending))
        .route("/works/{id}", get(library_work))
        .with_state(state)
}

async fn library_trending(
    State(state): State<Arc<Mutex<Catalog>>>,
) -> Result<Json<Value>, StatusCode> {
    let (status, delay_ms, body) = {
        let mut guard = state.lock().await;
        guard.hits.push("trending".into());
        let status = if guard.trending_status != 0 {
            guard.trending_status
        } else {
            guard.status
        };
        (
            status,
            guard.trending_delay_ms,
            json!({ "works": guard.trending }),
        )
    };
    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
    if status != 0 {
        return Err(StatusCode::from_u16(status).unwrap());
    }
    Ok(Json(body))
}

async fn library_search(
    State(state): State<Arc<Mutex<Catalog>>>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, StatusCode> {
    let mut guard = state.lock().await;
    if guard.status != 0 {
        return Err(StatusCode::from_u16(guard.status).unwrap());
    }
    let hit = if let Some(author) = query.get("author_key") {
        format!("author_key={author}")
    } else if let Some(q) = query.get("q") {
        format!("q={q}")
    } else {
        "other".into()
    };
    guard.hits.push(hit);
    if let Some((from, _)) = &guard.merged {
        if query.get("q").is_some_and(|q| q == &format!("key:{from}")) {
            return Ok(Json(json!({ "numFound": 0, "docs": [] })));
        }
    }
    if let Some(q) = query.get("q") {
        if let Some(key) = q.strip_prefix("key:") {
            if let Some(doc) = guard.detail_docs.get(key) {
                let doc = doc.clone();
                return Ok(Json(json!({ "numFound": 1, "docs": [doc] })));
            }
        }
    }
    if query.contains_key("author_key") {
        let docs: Vec<Value> = guard
            .siblings
            .iter()
            .map(|title| json!({ "title": title }))
            .collect();
        return Ok(Json(json!({ "numFound": docs.len(), "docs": docs })));
    }
    if query.get("q").is_some_and(|q| q.starts_with("key:")) && !guard.author_key.is_empty() {
        let key = query["q"].trim_start_matches("key:").to_string();
        let author_key = guard.author_key.clone();
        let isbns = guard.isbns.clone();
        return Ok(Json(json!({
            "numFound": 1,
            "docs": [{
                "key": key,
                "title": "Dune",
                "author_key": [author_key],
                "author_name": ["Frank Herbert"],
                "isbn": isbns
            }]
        })));
    }
    let page: i64 = query
        .get("page")
        .and_then(|page| page.parse().ok())
        .unwrap_or(1);
    Ok(Json(json!({
        "numFound": 1,
        "docs": [{
            "key": "/works/OL893414W",
            "title": "Dune",
            "author_name": ["Frank Herbert"],
            "first_publish_year": 1965,
            "cover_i": 11481354
        }],
        "page": page
    })))
}

async fn library_work(
    State(state): State<Arc<Mutex<Catalog>>>,
    id: axum::extract::Path<String>,
) -> axum::response::Response {
    let mut guard = state.lock().await;
    let key = format!("/works/{}", id.trim_end_matches(".json"));
    guard.hits.push(format!("work={key}"));
    if guard.status != 0 {
        return axum::response::Response::builder()
            .status(StatusCode::from_u16(guard.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            .body(Body::empty())
            .unwrap();
    }
    if let Some(doc) = guard.work_docs.get(&key) {
        let doc = doc.to_string();
        return axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(doc))
            .unwrap();
    }
    if let Some((from, to)) = &guard.merged {
        if key == *from {
            let to = to.clone();
            if guard.json_redirect {
                let body = json!({
                    "type": { "key": "/type/redirect" },
                    "location": to
                });
                return axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap();
            }
            return axum::response::Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, format!("{to}.json"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"location":"{to}"}}"#)))
                .unwrap();
        }
    }
    axum::response::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::empty())
        .unwrap()
}

async fn register(app: &axum::Router, username: &str, password: &str) -> String {
    let (status, body) = json_req(
        app,
        "POST",
        "/v1/auth/register",
        None,
        json!({ "username": username, "password": password }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_string()
}

async fn grant(pool: &SqlitePool, username: &str) {
    sqlx::query("UPDATE users SET is_admin = 1 WHERE username = ?")
        .bind(username)
        .execute(pool)
        .await
        .unwrap();
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, value)
}

async fn json_req(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    send(app, builder.body(Body::from(body.to_string())).unwrap()).await
}

async fn empty_req(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    send(app, builder.body(Body::empty()).unwrap()).await
}

#[derive(sqlx::FromRow)]
struct Stored {
    state: String,
    progress: i64,
    error: Option<String>,
    attempts: i64,
    book_id: Option<String>,
    download_id: Option<String>,
    next_check_at: i64,
    tried_guids: String,
    created_at: i64,
    updated_at: i64,
}

async fn stored(pool: &SqlitePool, id: &str) -> Stored {
    sqlx::query_as(
        "SELECT state, progress, error, attempts, book_id, download_id, next_check_at,
                tried_guids, created_at, updated_at
         FROM book_requests WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn ask(world: &World, work: &str) -> Value {
    let (status, body) = json_req(
        &world.app,
        "POST",
        "/v1/requests",
        Some(&world.token),
        json!({
            "workKey": work,
            "title": "Dune",
            "author": "Frank Herbert",
            "year": 1965,
            "coverId": 11481354
        }),
    )
    .await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "{status} {body}"
    );
    body
}

async fn bring_in(world: &World, work: &str) -> String {
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(world, work).await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(world, &nzo, &format!("/data/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "imported");
    assert_eq!(row.book_id.as_deref(), Some(id.as_str()));
    id
}

fn write_epub(dir: &Path, name: &str, bytes: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

async fn finish_job(world: &World, nzo: &str, storage: &str) {
    let mut sab = world.sab.lock().await;
    let job = sab.jobs.iter_mut().find(|job| job.nzo == nzo).unwrap();
    job.in_queue = false;
    job.status = "Completed".into();
    job.storage = storage.into();
}

#[tokio::test]
async fn request_happy_path() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["workKey"], "/works/OL893414W");
    assert_eq!(page["items"][0]["title"], "Dune");
    assert!(page["items"][0]["request"].is_null());

    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();

    let (nzo, nzbname, cat, url) = {
        let sab = world.sab.lock().await;
        assert_eq!(sab.adds.len(), 1);
        (
            sab.adds[0].nzo.clone(),
            sab.adds[0].nzbname.clone(),
            sab.adds[0].cat.clone(),
            sab.adds[0].name.clone(),
        )
    };
    assert_eq!(nzbname, format!("mewo-{id}"));
    assert_eq!(cat, "books");
    assert!(url.contains("/nzb/g1"), "{url}");
    assert_eq!(
        world.prowlarr.lock().await.indexer_ids,
        vec!["-1".to_string()]
    );
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.download_id.as_deref(), Some(nzo.as_str()));
    assert_eq!(row.attempts, 1);

    world.sab.lock().await.jobs[0].percent = 40;
    let before = stored(&world.state.pool, &id).await;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.progress, 40);
    assert_eq!(row.updated_at, before.updated_at);

    finish_job(&world, &nzo, &format!("/data/books/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "imported");
    assert_eq!(row.book_id.as_deref(), Some(id.as_str()));
    assert!(
        !world.downloads.path().join(format!("mewo-{id}")).exists(),
        "mewo removes the completed folder; SAB delete does not"
    );
    let sab = world.sab.lock().await;
    assert!(sab.deletes.contains(&nzo));
    assert!(!sab.archives.is_empty());
    assert!(sab.archives.iter().all(|value| value == "0"));

    let (status, library) = empty_req(&world.app, "GET", "/v1/library", Some(&world.token)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    assert_eq!(library["books"][0]["id"], id);
    assert_eq!(library["books"][0]["title"], "Dune");
    assert_eq!(library["books"][0]["shared"], false);
    let tmp = world.state.data_dir.join("tmp");
    if tmp.exists() {
        assert!(
            std::fs::read_dir(&tmp).unwrap().next().is_none(),
            "tmp left behind"
        );
    }
}

#[tokio::test]
async fn no_release_waits_then_fails() {
    let world = world().await;
    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.error.as_deref(), Some("No release yet."));
    assert_eq!(row.next_check_at, now + SIX_HOURS);
    assert_eq!(row.attempts, 0);

    worker::tick_at(&world.state, row.created_at + 14 * DAY + 1)
        .await
        .unwrap();
    let row = stored(&world.state.pool, id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some("No release found."));
}

#[tokio::test]
async fn failed_download_tries_next_release() {
    let world = world().await;
    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.key_ok = true;
        prowl.releases = vec![
            release(&prowl.origin, "first", "Dune.RETAIL.EPUB", 10),
            release(&prowl.origin, "second", "Dune.EPUB", 1),
            release(&prowl.origin, "third", "Dune.EPUB", 0),
        ];
    }
    let created = ask(&world, "/works/OL1W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let mut now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    assert_eq!(stored(&world.state.pool, &id).await.attempts, 1);

    fail_current(&world, "CRC error").await;
    now += POLL;
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.error.as_deref(), Some("Download failed: CRC error"));
    assert_eq!(row.attempts, 1);

    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.attempts, 2);
    assert!(row.tried_guids.contains("second"));

    fail_current(&world, "CRC error").await;
    now += POLL;
    worker::tick_at(&world.state, now).await.unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    assert_eq!(stored(&world.state.pool, &id).await.attempts, 3);

    fail_current(&world, "CRC error").await;
    now += POLL;
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some("Download failed: CRC error"));
}

async fn fail_current(world: &World, message: &str) {
    let mut sab = world.sab.lock().await;
    let job = sab.jobs.last_mut().unwrap();
    job.in_queue = false;
    job.status = "Failed".into();
    job.fail_message = message.into();
}

#[tokio::test]
async fn no_epub_or_bad_epub_retries() {
    let world = world().await;
    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.releases = vec![
            release(&prowl.origin, "g1", "Dune.EPUB", 3),
            release(&prowl.origin, "g2", "Dune.EPUB", 2),
            release(&prowl.origin, "g3", "Dune.EPUB", 1),
        ];
    }
    let created = ask(&world, "/works/OL2W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let mut now = created["createdAt"].as_i64().unwrap();

    worker::tick_at(&world.state, now).await.unwrap();
    complete_with(&world, &id, "note.pdf", b"not an epub").await;
    now += POLL;
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.error.as_deref(), Some("No EPUB in the download."));
    assert!(!world.downloads.path().join(format!("mewo-{id}")).exists());

    worker::tick_at(&world.state, now).await.unwrap();
    complete_with(&world, &id, "book.epub", b"this is text").await;
    now += POLL;
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert!(
        row.error
            .as_deref()
            .unwrap()
            .starts_with("Could not read that EPUB:"),
        "{:?}",
        row.error
    );
    assert!(!world.downloads.path().join(format!("mewo-{id}")).exists());

    let (status, body) = json_req(
        &world.app,
        "PATCH",
        "/v1/admin/settings",
        Some(&world.token),
        json!({ "maxEpubBytes": MIN_EPUB_BYTES }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    worker::tick_at(&world.state, now).await.unwrap();
    let big = vec![0u8; (MIN_EPUB_BYTES as usize) + 8];
    complete_with(&world, &id, "book.epub", &big).await;
    now += POLL;
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "failed");
    assert!(
        row.error.as_deref().unwrap().starts_with("EPUB too large:"),
        "{:?}",
        row.error
    );
}

async fn complete_with(world: &World, id: &str, file: &str, bytes: &[u8]) {
    let nzo = world.sab.lock().await.jobs.last().unwrap().nzo.clone();
    let folder = format!("mewo-{id}");
    finish_job(world, &nzo, &format!("/data/{folder}")).await;
    write_epub(&world.downloads.path().join(&folder), file, bytes);
}

#[tokio::test]
async fn folder_confinement() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let outside = TempDir::new().unwrap();
    write_epub(outside.path(), "canary.epub", &common::epub());

    for (work, storage, link) in [
        ("/works/OL3W", "/x/../../etc", false),
        ("/works/OL4W", "/data/other-job", false),
        ("/works/OL5W", "", true),
    ] {
        let created = ask(&world, work).await;
        let id = created["id"].as_str().unwrap().to_string();
        let now = created["createdAt"].as_i64().unwrap();
        worker::tick_at(&world.state, now).await.unwrap();
        let nzo = world.sab.lock().await.jobs.last().unwrap().nzo.clone();
        let storage = if link {
            let folder = world.downloads.path().join(format!("mewo-{id}"));
            std::os::unix::fs::symlink(outside.path(), &folder).unwrap();
            format!("/data/mewo-{id}")
        } else {
            storage.to_string()
        };
        finish_job(&world, &nzo, &storage).await;
        worker::tick_at(&world.state, now + POLL).await.unwrap();
        let row = stored(&world.state.pool, &id).await;
        assert_eq!(row.state, "failed", "{work}");
        assert_eq!(row.error.as_deref(), Some("Unexpected download folder."));
        assert!(row.book_id.is_none());
    }
    let books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(books, 0);
}

#[tokio::test]
async fn completed_reads_a_folder_or_a_single_file() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let file = "Frank Herbert - Dune (epub).epub";
    for (work, single) in [("/works/OL30W", false), ("/works/OL31W", true)] {
        let created = ask(&world, work).await;
        let id = created["id"].as_str().unwrap().to_string();
        let now = created["createdAt"].as_i64().unwrap();
        worker::tick_at(&world.state, now).await.unwrap();
        let nzo = world.sab.lock().await.adds.last().unwrap().nzo.clone();
        let folder_name = format!("mewo-{id}");
        write_epub(
            &world.downloads.path().join(&folder_name),
            file,
            &common::epub(),
        );
        let storage = if single {
            format!("/data/downloads/usenet/complete/books/{folder_name}/{file}")
        } else {
            format!("/data/downloads/usenet/complete/books/{folder_name}")
        };
        finish_job(&world, &nzo, &storage).await;
        worker::tick_at(&world.state, now + POLL).await.unwrap();
        let row = stored(&world.state.pool, &id).await;
        assert_eq!(row.state, "imported", "{work} {:?}", row.error);
        assert_eq!(row.book_id.as_deref(), Some(id.as_str()));
    }
}

#[tokio::test]
async fn completed_rejects_a_foreign_storage_path() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let outside = TempDir::new().unwrap();
    let file = "Frank Herbert - Dune (epub).epub";

    let created = ask(&world, "/works/OL32W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds.last().unwrap().nzo.clone();
    let outside_dir = outside.path().join(format!("mewo-{id}"));
    write_epub(&outside_dir, file, &common::epub());
    finish_job(&world, &nzo, outside_dir.join(file).to_str().unwrap()).await;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some("Unexpected download folder."));
    assert!(row.book_id.is_none());

    let created = ask(&world, "/works/OL33W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds.last().unwrap().nzo.clone();
    write_epub(
        &world.downloads.path().join("mewo-other"),
        file,
        &common::epub(),
    );
    finish_job(
        &world,
        &nzo,
        &format!("/data/downloads/usenet/complete/books/mewo-other/{file}"),
    )
    .await;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some("Unexpected download folder."));
    assert!(row.book_id.is_none());

    let books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(books, 0);
}

#[tokio::test]
async fn crash_after_addurl_is_idempotent() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL6W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    world.sab.lock().await.jobs.push(SabJob {
        nzo: "already".into(),
        name: format!("mewo-{id}"),
        status: "Downloading".into(),
        percent: 0,
        storage: String::new(),
        fail_message: String::new(),
        in_queue: true,
    });
    worker::tick_at(&world.state, now).await.unwrap();
    assert!(world.sab.lock().await.adds.is_empty());
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.download_id.as_deref(), Some("already"));
    assert_eq!(row.attempts, 1);
}

#[tokio::test]
async fn crash_after_import_is_idempotent() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL7W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at, post_count)
         VALUES (?, ?, 'Dune', 'Frank Herbert', 'herbert', 1, 1)",
    )
    .bind(&id)
    .bind(&world.user_id)
    .execute(&world.state.pool)
    .await
    .unwrap();
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "imported");
    assert_eq!(row.book_id.as_deref(), Some(id.as_str()));
    let books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(books, 1);
}

#[tokio::test]
async fn a_stale_book_dir_is_replaced_on_import() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL12W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    let dest = world
        .state
        .data_dir
        .join("books")
        .join(&world.user_id)
        .join(&id);
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("stale.txt"), b"old").unwrap();
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "imported");
    assert!(!dest.join("stale.txt").exists());
    assert!(dest.join("book.epub").is_file());
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_only_download_folder_stays_imported() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL13W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{id}")).await;
    let folder = world.downloads.path().join(format!("mewo-{id}"));
    write_epub(&folder, "Dune.epub", &common::epub());
    let mut perms = std::fs::metadata(&folder).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(&folder, perms).unwrap();
    let _restore = ModeGuard(&folder);

    let buf = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .with_writer(move || StdLog(writer.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    drop(_guard);

    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "imported");
    assert!(folder.join("Dune.epub").is_file());
    let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains(&format!("mewo-{id}")) && logs.contains("could not remove download folder"),
        "{logs}"
    );
}

struct ModeGuard<'a>(&'a Path);

impl Drop for ModeGuard<'_> {
    fn drop(&mut self) {
        if let Ok(meta) = std::fs::metadata(self.0) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(self.0, perms);
        }
    }
}

#[tokio::test]
async fn catalog_upstream_failure_is_502() {
    let data = TempDir::new().unwrap();
    let origin = serve(Router::new().route(
        "/search.json",
        get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
    ))
    .await;
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: origin,
        auth: AuthConfig::default(),
    })
    .await
    .unwrap();
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    grant(&state.pool, "matt").await;
    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "requestsEnabled": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&token)).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "Couldn't search the catalog right now.");
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=", Some(&token)).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "Open Library is not answering. Try again.");
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search", Some(&token)).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "Open Library is not answering. Try again.");
}

#[tokio::test]
async fn catalog_search_rejects_a_query_shorter_than_three_characters() {
    let catalog = Arc::new(Mutex::new(Catalog::default()));
    let (_state, app, _data, token) = catalog_app(Arc::clone(&catalog)).await;
    for q in ["d", "du", " d ", "é", "书"] {
        let path = format!("/v1/catalog/search?q={}", percent_encode(q));
        let (status, body) = empty_req(&app, "GET", &path, Some(&token)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q:?} {body}");
        assert_eq!(
            body["error"],
            "Type at least 3 characters to search the catalog."
        );
    }
    assert!(
        catalog.lock().await.hits.is_empty(),
        "short queries must not call Open Library: {:?}",
        catalog.lock().await.hits
    );

    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=%20%20", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["items"].is_array(), "{body}");
    assert_eq!(catalog.lock().await.hits, vec!["trending".to_string()]);

    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=dun", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["items"][0]["title"], "Dune");
    assert_eq!(
        catalog.lock().await.hits,
        vec!["trending".to_string(), "q=dun".to_string()]
    );
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[tokio::test]
async fn cancel_races_worker() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL8W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    let (gate, _hold) = worker::install(&world.state.data_dir);
    gate.arm();
    let state = world.state.clone();
    let tick = tokio::spawn(async move {
        worker::tick_at(&state, now + POLL).await.unwrap();
    });
    tokio::time::timeout(Duration::from_secs(5), gate.entered())
        .await
        .expect("worker reached the import gate");
    let (status, body) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{id}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    gate.go();
    tick.await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "canceled");
    let books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(books, 0);
    assert!(world.sab.lock().await.deletes.contains(&nzo));
}

#[tokio::test]
async fn one_active_per_work_and_cap() {
    let world = world().await;
    let first = ask(&world, "/works/OL9W").await;
    let again = ask(&world, "/works/OL9W").await;
    assert_eq!(first["id"], again["id"]);
    for index in 2..=5 {
        let (status, _) = json_req(
            &world.app,
            "POST",
            "/v1/requests",
            Some(&world.token),
            json!({
                "workKey": format!("/works/OL{index}W"),
                "title": "Dune",
                "author": "Frank Herbert"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, body) = json_req(
        &world.app,
        "POST",
        "/v1/requests",
        Some(&world.token),
        json!({
            "workKey": "/works/OL99W",
            "title": "Dune",
            "author": "Frank Herbert"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "You have 5 requests in progress.");
}

#[tokio::test]
async fn retry_counts_toward_the_active_cap() {
    let world = world().await;
    let mut canceled = String::new();
    for index in 1..=5 {
        let created = ask(&world, &format!("/works/OL{index}W")).await;
        if index == 1 {
            canceled = created["id"].as_str().unwrap().to_string();
        }
    }
    let (status, body) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{canceled}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = json_req(
        &world.app,
        "POST",
        "/v1/requests",
        Some(&world.token),
        json!({
            "workKey": "/works/OL6W",
            "title": "Dune",
            "author": "Frank Herbert"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{canceled}/retry"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "You have 5 requests in progress.");
    assert_eq!(stored(&world.state.pool, &canceled).await.state, "canceled");

    let admin = register(&world.app, "ada", "password1").await;
    grant(&world.state.pool, "ada").await;
    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{canceled}/retry"),
        Some(&admin),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "You have 5 requests in progress.");
    assert_eq!(stored(&world.state.pool, &canceled).await.state, "canceled");
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND state IN ('requested', 'downloading')",
    )
    .bind(&world.user_id)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(active, 5);
}

#[tokio::test]
async fn deleting_an_imported_book_lets_you_request_it_again() {
    let world = world().await;
    let id = bring_in(&world, "/works/OL893414W").await;
    let (status, body) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/books/{id}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "canceled");
    assert!(row.book_id.is_none());
    let books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(books, 0);

    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["request"]["state"], "canceled");
    assert!(page["items"][0]["request"]["bookId"].is_null());

    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{id}/retry"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "requested");
    let (status, body) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{id}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = json_req(
        &world.app,
        "POST",
        "/v1/requests",
        Some(&world.token),
        json!({
            "workKey": "/works/OL893414W",
            "title": "Dune",
            "author": "Frank Herbert"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_ne!(body["id"], id);
    assert_eq!(stored(&world.state.pool, &id).await.state, "canceled");
}

#[tokio::test]
async fn permissions() {
    let data = TempDir::new().unwrap();
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: AuthConfig::default(),
    })
    .await
    .unwrap();
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Requests are off.");
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Requests are off.");
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/works/OL1W", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Requests are off.");
    let (status, body) = empty_req(&app, "GET", "/v1/requests", Some(&token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let world = world().await;
    let other = register(&world.app, "ada", "password1").await;
    let created = ask(&world, "/works/OL10W").await;
    let id = created["id"].as_str().unwrap();
    let (status, body) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{id}"),
        Some(&other),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{id}/retry"),
        Some(&other),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = empty_req(&world.app, "GET", "/v1/requests?all=1", Some(&other)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Admins only.");
}

#[tokio::test(flavor = "current_thread")]
async fn secrets_are_write_only() {
    let world = world().await;
    let sentinel = "prowlarr-key-sentinel-9f3c";
    let (status, body) = json_req(
        &world.app,
        "PATCH",
        "/v1/admin/settings",
        Some(&world.token),
        json!({ "prowlarrApiKey": sentinel }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let raw = body.to_string();
    assert!(!raw.contains(sentinel), "{raw}");
    assert_eq!(body["secrets"]["prowlarrApiKey"], true);

    let (status, body) =
        empty_req(&world.app, "GET", "/v1/admin/settings", Some(&world.token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.to_string().contains(sentinel));
    assert_eq!(body["secrets"]["prowlarrApiKey"], true);
    assert!(body["downloadsDir"].is_string());

    let (status, body) = json_req(
        &world.app,
        "PATCH",
        "/v1/admin/settings",
        Some(&world.token),
        json!({ "notASetting": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Unknown setting \"notASetting\".");
    assert!(!body.to_string().contains(sentinel));

    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let writer = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || LogBuf(writer.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    world.prowlarr.lock().await.status_code = 500;
    let started = Instant::now();
    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/admin/requests/test",
        Some(&world.token),
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "500 should not wait out the timeout"
    );
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][0]["detail"], "HTTP 500");
    assert!(!body.to_string().contains(sentinel));

    let (status, _) = json_req(
        &world.app,
        "PATCH",
        "/v1/admin/settings",
        Some(&world.token),
        json!({ "prowlarrUrl": "http://127.0.0.1:1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let started = Instant::now();
    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/admin/requests/test",
        Some(&world.token),
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "refused connect took {:?}",
        started.elapsed()
    );
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][0]["detail"], "can't connect");
    assert!(!body.to_string().contains(sentinel));

    let logs = String::from_utf8(buf.lock().await.clone()).unwrap();
    assert!(!logs.contains(sentinel), "{logs}");

    let (status, body) = json_req(
        &world.app,
        "PATCH",
        "/v1/admin/settings",
        Some(&world.token),
        json!({ "prowlarrApiKey": Value::Null }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["secrets"]["prowlarrApiKey"], false);
    assert!(!body.to_string().contains(sentinel));
}

struct StdLog(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for StdLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.blocking_lock().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn upstream_hardening() {
    let world = world().await;
    let created = ask(&world, "/works/OL11W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();

    world.prowlarr.lock().await.redirect = true;
    worker::tick_at(&world.state, now).await.unwrap();
    assert_eq!(
        stored(&world.state.pool, &id).await.error.as_deref(),
        Some("Prowlarr: HTTP 302")
    );
    assert_eq!(stored(&world.state.pool, &id).await.attempts, 0);

    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.redirect = false;
        prowl.huge = true;
    }
    worker::tick_at(&world.state, now + 5 * 60 * 1000)
        .await
        .unwrap();
    assert_eq!(
        stored(&world.state.pool, &id).await.error.as_deref(),
        Some("Prowlarr: unexpected response")
    );

    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.huge = false;
        prowl.app_name = "NotProwlarr".into();
    }
    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/admin/requests/test",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][0]["ok"], false);
    assert_eq!(body["checks"][0]["detail"], "unexpected response");

    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.app_name.clear();
        prowl.releases = vec![json!({
            "guid": "foreign",
            "indexerId": 1,
            "indexer": "NZBgeek",
            "title": "Dune.EPUB",
            "size": 80_000,
            "protocol": "usenet",
            "downloadUrl": "http://evil.example/nzb",
            "grabs": 1,
            "categories": [{ "id": 7020 }]
        })];
    }
    worker::tick_at(&world.state, now + 10 * 60 * 1000)
        .await
        .unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.error.as_deref(), Some("No release yet."));
    assert!(world.sab.lock().await.adds.is_empty());
}

#[tokio::test]
async fn test_connection_reports() {
    let world = world().await;
    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/admin/requests/test",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][0]["name"], "prowlarr");
    assert_eq!(body["checks"][0]["ok"], true);
    assert_eq!(
        body["checks"][0]["detail"],
        "2.4.0 · 1 usenet indexers with ebooks"
    );
    assert_eq!(body["checks"][1]["detail"], "4.5.3 · category books");
    assert_eq!(body["checks"][2]["detail"], "readable");

    let (status, _) = json_req(
        &world.app,
        "PATCH",
        "/v1/admin/settings",
        Some(&world.token),
        json!({ "prowlarrApiKey": "nope" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/admin/requests/test",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][0]["detail"], "HTTP 401");

    world.sab.lock().await.categories = vec!["*".into(), "tv".into()];
    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/admin/requests/test",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][1]["detail"], "No category \"books\"");

    let data = TempDir::new().unwrap();
    let bare = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: AuthConfig::default(),
    })
    .await
    .unwrap();
    let app = router(bare.clone());
    let token = register(&app, "matt", "password1").await;
    sqlx::query("UPDATE users SET is_admin = 1 WHERE username = 'matt'")
        .execute(&bare.pool)
        .await
        .unwrap();
    let (status, body) = empty_req(&app, "POST", "/v1/admin/requests/test", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["checks"][2]["detail"], "Not set");
}

#[tokio::test]
async fn migration_007_adds_empty_secrets() {
    let tmp = TempDir::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
    let pool = SqlitePool::connect(&url).await.unwrap();
    let migrator = sqlx::migrate!("./migrations");
    let mut conn = pool.acquire().await.unwrap();
    Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
    for migration in migrator.iter() {
        if migration.version < 7 {
            Migrate::apply(&mut *conn, migration).await.unwrap();
        }
    }
    drop(conn);
    sqlx::query(
        "INSERT INTO settings (id, doc, updated_at, updated_by)
         VALUES (1, '{\"allowSignup\":true,\"maxEpubBytes\":67108864}', 1, NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let secrets: String = sqlx::query_scalar("SELECT secrets FROM settings WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(secrets, "{}");
    drop(pool);

    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: AuthConfig::default(),
    })
    .await
    .unwrap();
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    sqlx::query("UPDATE users SET is_admin = 1 WHERE username = 'matt'")
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, body) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["secrets"]["prowlarrApiKey"], false);
    assert_eq!(body["secrets"]["sabnzbdApiKey"], false);
    assert_eq!(body["requestsEnabled"], false);
    assert_eq!(body["prowlarrUrl"], "");
    assert_eq!(body["sabnzbdUrl"], "");
    assert_eq!(body["sabnzbdCategory"], "books");
    assert_eq!(body["languages"], json!(["en"]));
    assert!(body["downloadsDir"].is_null());
    assert!(body.get("prowlarrApiKey").is_none());
}

#[tokio::test]
async fn chapterhouse_is_not_grabbed() {
    let world = world().await;
    {
        let mut catalog = world.catalog.lock().await;
        catalog.author_key = "OL79034A".into();
        catalog.siblings = vec!["Dune".into(), "Chapterhouse Dune".into()];
    }
    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.key_ok = true;
        prowl.releases = vec![release(
            &prowl.origin,
            "ch",
            "Frank Herbert - [Dune 06] - Chapterhouse Dune (retail) (epub)",
            10,
        )];
    }
    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.attempts, 0);
    assert_eq!(
        row.error.as_deref(),
        Some("No matching release (closest: Chapterhouse Dune, a different book)")
    );
    assert!(world.sab.lock().await.adds.is_empty());
}

#[tokio::test]
async fn a_wrong_opf_retries_the_next_guid() {
    let world = world().await;
    {
        let mut catalog = world.catalog.lock().await;
        catalog.author_key = "OL79034A".into();
        catalog.siblings = vec!["Dune".into(), "Chapterhouse Dune".into()];
    }
    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.releases = vec![
            release(
                &prowl.origin,
                "first",
                "Frank Herbert - Dune RETAIL EPUB",
                5,
            ),
            release(&prowl.origin, "second", "Frank Herbert - Dune EPUB", 1),
            release(&prowl.origin, "third", "Frank Herbert - Dune epub", 0),
        ];
    }
    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    assert_eq!(stored(&world.state.pool, &id).await.attempts, 1);
    let epub = common::with_metadata(
        "<dc:title>Chapterhouse Dune</dc:title><dc:creator>Frank Herbert</dc:creator>",
    );
    complete_with(&world, &id, "Dune.epub", &epub).await;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(
        row.error.as_deref(),
        Some("Downloaded book is Chapterhouse Dune")
    );
    assert!(row.tried_guids.contains("first"));
    assert!(!row.tried_guids.contains("second"));
    nothing_imported(&world, &id).await;

    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.attempts, 2);
    assert!(row.tried_guids.contains("second"));
    let urls: Vec<String> = world
        .sab
        .lock()
        .await
        .adds
        .iter()
        .map(|add| add.name.clone())
        .collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[1].contains("/nzb/second"), "{urls:?}");

    complete_with(&world, &id, "Dune.epub", &epub).await;
    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.attempts, 2);
    nothing_imported(&world, &id).await;

    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.attempts, 3);
    assert!(row.tried_guids.contains("third"));

    complete_with(&world, &id, "Dune.epub", &epub).await;
    worker::tick_at(&world.state, now + POLL * 3).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(
        row.error.as_deref(),
        Some("Downloaded book is Chapterhouse Dune")
    );
    nothing_imported(&world, &id).await;
}

async fn nothing_imported(world: &World, id: &str) {
    let books: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(books, 0);
    assert!(stored(&world.state.pool, id).await.book_id.is_none());
    assert!(!world.downloads.path().join(format!("mewo-{id}")).exists());
    let tmp = world.state.data_dir.join("tmp");
    if tmp.exists() {
        assert!(
            std::fs::read_dir(&tmp).unwrap().next().is_none(),
            "tmp left behind"
        );
    }
}

#[tokio::test]
async fn open_library_down_holds_then_recovers() {
    let world = world().await;
    world.catalog.lock().await.status = 500;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.error.as_deref(), Some("Open Library: HTTP 500"));
    assert_eq!(row.next_check_at, now + 5 * 60 * 1000);
    assert!(world.sab.lock().await.adds.is_empty());

    world.catalog.lock().await.status = 0;
    worker::tick_at(&world.state, now + 5 * 60 * 1000)
        .await
        .unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.attempts, 1);
    assert_eq!(world.sab.lock().await.adds.len(), 1);
}

#[tokio::test]
async fn open_library_down_at_completed_keeps_the_job() {
    let world = world().await;
    let created = ask(&world, "/works/OL40W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    world.sab.lock().await.jobs.push(SabJob {
        nzo: "kept".into(),
        name: format!("mewo-{id}"),
        status: "Downloading".into(),
        percent: 0,
        storage: String::new(),
        fail_message: String::new(),
        in_queue: true,
    });
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.download_id.as_deref(), Some("kept"));
    assert!(world.catalog.lock().await.hits.is_empty());

    finish_job(&world, "kept", &format!("/data/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    world.catalog.lock().await.status = 500;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.error.as_deref(), Some("Open Library: HTTP 500"));
    assert!(!world
        .sab
        .lock()
        .await
        .deletes
        .iter()
        .any(|nzo| nzo == "kept"));
    assert!(world.downloads.path().join(format!("mewo-{id}")).exists());
}

#[tokio::test]
async fn a_merged_work_follows_the_live_key() {
    let world = world().await;
    {
        let mut catalog = world.catalog.lock().await;
        catalog.merged = Some(("/works/OL893415W".into(), "/works/OL893414W".into()));
        catalog.author_key = "OL79034A".into();
        catalog.siblings = vec!["Chapterhouse Dune".into()];
    }
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893415W").await;
    let id = created["id"].as_str().unwrap();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let row = stored(&world.state.pool, id).await;
    assert_eq!(row.state, "downloading", "{:?}", row.error);
    assert_eq!(row.attempts, 1);
    let hits = world.catalog.lock().await.hits.clone();
    assert_eq!(
        hits,
        vec![
            "q=key:/works/OL893415W".to_string(),
            "work=/works/OL893415W".to_string(),
            "q=key:/works/OL893414W".to_string(),
            "author_key=OL79034A".to_string(),
        ]
    );
}

#[tokio::test]
async fn open_library_gives_up_after_fourteen_days() {
    let world = world().await;
    world.catalog.lock().await.status = 500;
    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now + 14 * DAY + 1)
        .await
        .unwrap();
    let row = stored(&world.state.pool, id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some("Open Library: HTTP 500"));
    assert!(world.sab.lock().await.deletes.is_empty());

    let created = ask(&world, "/works/OL41W").await;
    let id = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    world.sab.lock().await.jobs.push(SabJob {
        nzo: "expired".into(),
        name: format!("mewo-{id}"),
        status: "Downloading".into(),
        percent: 0,
        storage: String::new(),
        fail_message: String::new(),
        in_queue: true,
    });
    worker::tick_at(&world.state, now).await.unwrap();
    assert_eq!(
        stored(&world.state.pool, &id).await.download_id.as_deref(),
        Some("expired")
    );
    finish_job(&world, "expired", &format!("/data/mewo-{id}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{id}")),
        "Dune.epub",
        &common::epub(),
    );
    worker::tick_at(&world.state, now + 14 * DAY + 1)
        .await
        .unwrap();
    let row = stored(&world.state.pool, &id).await;
    assert_eq!(row.state, "failed");
    assert_eq!(row.error.as_deref(), Some("Open Library: HTTP 500"));
    assert!(row.download_id.is_none());
    assert!(world
        .sab
        .lock()
        .await
        .deletes
        .iter()
        .any(|nzo| nzo == "expired"));
    assert!(!world.downloads.path().join(format!("mewo-{id}")).exists());
}

#[tokio::test]
async fn one_open_library_pair_per_request() {
    let world = world().await;
    {
        let mut catalog = world.catalog.lock().await;
        catalog.author_key = "OL79034A".into();
        catalog.siblings = vec!["Dune".into(), "Dune Messiah".into()];
    }
    let created = ask(&world, "/works/OL893414W").await;
    let id = created["id"].as_str().unwrap();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    worker::tick_at(&world.state, now + SIX_HOURS)
        .await
        .unwrap();
    let hits = world.catalog.lock().await.hits.clone();
    assert_eq!(
        hits,
        vec![
            "q=key:/works/OL893414W".to_string(),
            "author_key=OL79034A".to_string(),
        ]
    );
    let row = stored(&world.state.pool, id).await;
    assert_eq!(row.state, "requested");
    assert_eq!(row.error.as_deref(), Some("No release yet."));
    assert!(world.sab.lock().await.adds.is_empty());
}

struct Account {
    token: String,
    id: String,
}

async fn account(world: &World, username: &str) -> Account {
    let token = register(&world.app, username, "password1").await;
    let id: String = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    Account { token, id }
}

async fn post_work(world: &World, token: &str, work: &str) -> (StatusCode, Value) {
    json_req(
        &world.app,
        "POST",
        "/v1/requests",
        Some(token),
        json!({
            "workKey": work,
            "title": "Dune",
            "author": "Frank Herbert",
            "year": 1965,
            "coverId": 11481354
        }),
    )
    .await
}

async fn follows_of(pool: &SqlitePool, id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT follows FROM book_requests WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn epub_file(world: &World, owner: &str, book: &str) -> std::path::PathBuf {
    world
        .state
        .data_dir
        .join("books")
        .join(owner)
        .join(book)
        .join("book.epub")
}

async fn book_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn two_releases(origin: &str) -> Vec<Value> {
    vec![
        release(origin, "g1", "Frank.Herbert.Dune.1965.RETAIL.EPUB", 10),
        release(origin, "g2", "Frank.Herbert.Dune.1965.EPUB", 1),
    ]
}

#[tokio::test]
async fn reuse_of_a_private_copy_skips_the_download() {
    let world = world().await;
    let book_id = bring_in(&world, "/works/OL893414W").await;
    let queries = world.prowlarr.lock().await.queries.len();
    let adds = world.sab.lock().await.adds.len();
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;

    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["state"], "imported");
    assert_eq!(body["progress"], 100);
    assert_eq!(body["bookId"], book_id);
    assert_eq!(body["requester"], "ada");
    assert!(body.get("follows").is_none(), "{body}");
    let ada_request = body["id"].as_str().unwrap();
    assert_ne!(ada_request, book_id);

    assert_eq!(book_count(&world.state.pool).await, 1);
    assert!(epub_file(&world, &world.user_id, &book_id).is_file());
    assert!(!epub_file(&world, &ada.id, &book_id).exists());
    assert_eq!(world.prowlarr.lock().await.queries.len(), queries);
    assert_eq!(world.sab.lock().await.adds.len(), adds);

    let (status, again) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["id"], ada_request);

    let (status, library) = empty_req(&world.app, "GET", "/v1/library", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    let held = library["books"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["id"] == book_id)
        .unwrap();
    assert_eq!(held["held"], true);
    assert_eq!(held["ownerUsername"], "");
    assert_eq!(held["mine"], false);
    assert_eq!(held["shared"], false);

    let (status, _) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}/epub"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, missing) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, _) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}/epub"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn failed_request(pool: &SqlitePool, id: &str, user_id: &str, work: &str, at: i64) {
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, state, error, tried_guids,
            next_check_at, created_at, updated_at
         ) VALUES (?, ?, ?, 'Dune', 'Frank Herbert', 'failed', 'Download failed: CRC error', '[]', 0, ?, ?)",
    )
    .bind(id)
    .bind(user_id)
    .bind(work)
    .bind(at)
    .bind(at)
    .execute(pool)
    .await
    .unwrap();
}

async fn imported_for(pool: &SqlitePool, user_id: &str, work: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND work_key = ? AND state = 'imported'",
    )
    .bind(user_id)
    .bind(work)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Deleting the books row nulls `book_id` and leaves the request imported.
/// A missing books row is the same dead hold.
async fn delete_leaving_import(pool: &SqlitePool, book_id: &str, request_id: &str) {
    sqlx::query("DELETE FROM books WHERE id = ?")
        .bind(book_id)
        .execute(pool)
        .await
        .unwrap();
    let row = stored(pool, request_id).await;
    assert_eq!(row.state, "imported");
    assert!(row.book_id.is_none());
}

#[tokio::test]
async fn request_again_reuses_a_live_copy_when_the_import_is_stale() {
    let world = world().await;
    let work = "/works/OL893414W";
    let book_id = bring_in(&world, work).await;
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;
    shelf_book(&world.state.pool, "ada-gone", &ada.id, 0).await;
    shelf_request(&world.state.pool, "ada-stale", &ada.id, work, "ada-gone", 1).await;
    delete_leaving_import(&world.state.pool, "ada-gone", "ada-stale").await;
    say(&world, &world.token, &book_id, "owner note").await;

    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["request"]["id"], "ada-stale");
    assert_eq!(page["items"][0]["request"]["state"], "imported");
    assert!(page["items"][0]["request"]["bookId"].is_null());
    assert!(page["items"][0]["onServer"].is_null(), "{page}");

    let queries = world.prowlarr.lock().await.queries.len();
    let adds = world.sab.lock().await.adds.len();
    let books = book_count(&world.state.pool).await;

    let (status, body) = post_work(&world, &ada.token, work).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "ada-stale");
    assert_eq!(body["state"], "imported");
    assert_eq!(body["bookId"], book_id);
    assert_eq!(body["progress"], 100);
    assert_eq!(body["requester"], "ada");
    assert!(body.get("follows").is_none(), "{body}");

    assert_eq!(book_count(&world.state.pool).await, books);
    assert_eq!(world.prowlarr.lock().await.queries.len(), queries);
    assert_eq!(world.sab.lock().await.adds.len(), adds);
    assert!(epub_file(&world, &world.user_id, &book_id).is_file());
    assert!(!epub_file(&world, &ada.id, &book_id).exists());
    assert_eq!(imported_for(&world.state.pool, &ada.id, work).await, 1);
    assert_eq!(
        stored(&world.state.pool, "ada-stale")
            .await
            .book_id
            .as_deref(),
        Some(book_id.as_str())
    );
    assert_eq!(
        stored(&world.state.pool, &book_id).await.book_id.as_deref(),
        Some(book_id.as_str())
    );

    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["request"]["id"], "ada-stale");
    assert_eq!(page["items"][0]["request"]["bookId"], book_id);
    assert_eq!(page["items"][0]["onServer"]["bookId"], book_id);
    assert_eq!(page["items"][0]["onServer"]["held"], true);
    assert_eq!(page["items"][0]["onServer"]["mine"], false);
    assert_eq!(page["items"][0]["onServer"]["shared"], false);
    assert_eq!(page["items"][0]["onServer"]["ownerUsername"], "");

    let (status, _) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}/epub"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, missing) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");

    let ada_texts = thread_texts(&world, &ada.token, &book_id).await;
    assert!(ada_texts.is_empty(), "{ada_texts:?}");
    say(&world, &ada.token, &book_id, "holder private").await;
    let matt_texts = thread_texts(&world, &world.token, &book_id).await;
    assert_eq!(matt_texts, vec!["owner note".to_string()]);
    let ada_texts = thread_texts(&world, &ada.token, &book_id).await;
    assert_eq!(ada_texts, vec!["holder private".to_string()]);

    let (status, mine) = empty_req(&world.app, "GET", "/v1/requests", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["items"].as_array().unwrap().len(), 1);
    assert_eq!(mine["items"][0]["id"], "ada-stale");
    assert_eq!(mine["items"][0]["requester"], "ada");
    assert!(mine["items"][0].get("follows").is_none(), "{mine}");
    let (status, all) = empty_req(&world.app, "GET", "/v1/requests?all=1", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{all}");
}

#[tokio::test]
async fn retry_reuses_a_live_copy_when_the_import_is_stale() {
    let world = world().await;
    let work = "/works/OL893414W";
    let book_id = bring_in(&world, work).await;
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;

    // The failure is newer, so the catalog retries that row. The import's
    // book is already gone.
    shelf_book(&world.state.pool, "ada-gone", &ada.id, 0).await;
    shelf_request(&world.state.pool, "ada-stale", &ada.id, work, "ada-gone", 1).await;
    delete_leaving_import(&world.state.pool, "ada-gone", "ada-stale").await;
    failed_request(&world.state.pool, "ada-failed", &ada.id, work, 5).await;

    // The dead import is newer than the failure. Retry still has to leave one
    // imported row on the live copy, which is the row the catalog shows.
    let other = "/works/OLOTHER";
    shelf_book(&world.state.pool, "live-2", &world.user_id, 0).await;
    shelf_request(
        &world.state.pool,
        "matt-2",
        &world.user_id,
        other,
        "live-2",
        1,
    )
    .await;
    shelf_book(&world.state.pool, "ada-gone-2", &ada.id, 0).await;
    shelf_request(
        &world.state.pool,
        "ada-stale-2",
        &ada.id,
        other,
        "ada-gone-2",
        20,
    )
    .await;
    delete_leaving_import(&world.state.pool, "ada-gone-2", "ada-stale-2").await;
    failed_request(&world.state.pool, "ada-failed-2", &ada.id, other, 5).await;

    let queries = world.prowlarr.lock().await.queries.len();
    let adds = world.sab.lock().await.adds.len();
    let books = book_count(&world.state.pool).await;

    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/requests/ada-failed/retry",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "ada-failed");
    assert_eq!(body["state"], "imported");
    assert_eq!(body["bookId"], book_id);
    assert_eq!(body["requester"], "ada");
    assert!(body.get("follows").is_none(), "{body}");
    assert_eq!(
        stored(&world.state.pool, "ada-failed")
            .await
            .book_id
            .as_deref(),
        Some(book_id.as_str())
    );
    assert_eq!(
        stored(&world.state.pool, "ada-stale").await.state,
        "canceled"
    );
    assert!(stored(&world.state.pool, "ada-stale")
        .await
        .book_id
        .is_none());
    assert_eq!(imported_for(&world.state.pool, &ada.id, work).await, 1);

    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["request"]["id"], "ada-failed");
    assert_eq!(page["items"][0]["request"]["bookId"], book_id);

    let (status, body) = empty_req(
        &world.app,
        "POST",
        "/v1/requests/ada-failed-2/retry",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "ada-stale-2");
    assert_eq!(body["state"], "imported");
    assert_eq!(body["bookId"], "live-2");
    assert_eq!(body["requester"], "ada");
    assert!(body.get("follows").is_none(), "{body}");
    assert_eq!(
        stored(&world.state.pool, "ada-failed-2").await.state,
        "failed"
    );
    assert!(stored(&world.state.pool, "ada-failed-2")
        .await
        .book_id
        .is_none());
    assert_eq!(
        stored(&world.state.pool, "ada-stale-2")
            .await
            .book_id
            .as_deref(),
        Some("live-2")
    );
    assert_eq!(imported_for(&world.state.pool, &ada.id, other).await, 1);

    assert_eq!(book_count(&world.state.pool).await, books);
    assert_eq!(world.prowlarr.lock().await.queries.len(), queries);
    assert_eq!(world.sab.lock().await.adds.len(), adds);
    assert_eq!(
        stored(&world.state.pool, &book_id).await.book_id.as_deref(),
        Some(book_id.as_str())
    );

    let (status, missing) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, mine) = empty_req(&world.app, "GET", "/v1/requests", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    let ids: Vec<&str> = mine["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&book_id.as_str()));
    assert!(!ids.contains(&"matt-2"));
    assert!(ids.iter().all(|id| !id.starts_with("matt")));
    let (status, all) = empty_req(&world.app, "GET", "/v1/requests?all=1", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{all}");
}

#[tokio::test]
async fn a_follower_completes_with_the_leader() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893414W").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    world.sab.lock().await.jobs[0].percent = 40;
    worker::tick_at(&world.state, now + POLL).await.unwrap();

    let ada = account(&world, "ada").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["state"], "downloading");
    assert_eq!(body["progress"], 40);
    assert_eq!(body["requester"], "ada");
    assert!(body.get("follows").is_none(), "{body}");
    assert!(!body.to_string().contains("matt"), "{body}");
    let ada_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(
        follows_of(&world.state.pool, &ada_id).await.as_deref(),
        Some(leader.as_str())
    );
    assert_eq!(world.sab.lock().await.adds.len(), 1);

    let (status, mine) = empty_req(&world.app, "GET", "/v1/requests", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert!(mine["items"][0].get("follows").is_none(), "{mine}");
    let (status, all) =
        empty_req(&world.app, "GET", "/v1/requests?all=1", Some(&world.token)).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let admin = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == ada_id)
        .unwrap();
    assert_eq!(admin["follows"], "matt");
    assert_eq!(admin["state"], "downloading");

    sqlx::query("UPDATE book_requests SET download_id = 'follower-job' WHERE id = ?")
        .bind(&ada_id)
        .execute(&world.state.pool)
        .await
        .unwrap();
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    assert!(stored(&world.state.pool, &ada_id)
        .await
        .download_id
        .is_none());
    assert!(world
        .sab
        .lock()
        .await
        .deletes
        .iter()
        .any(|nzo| nzo == "follower-job"));
    assert_eq!(
        stored(&world.state.pool, &leader).await.state,
        "downloading"
    );

    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{leader}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{leader}")),
        "Dune.epub",
        &common::epub(),
    );
    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();

    let lead = stored(&world.state.pool, &leader).await;
    let follow = stored(&world.state.pool, &ada_id).await;
    assert_eq!(lead.state, "imported");
    assert_eq!(follow.state, "imported");
    assert_eq!(lead.book_id.as_deref(), Some(leader.as_str()));
    assert_eq!(follow.book_id, lead.book_id);
    assert_eq!(follow.progress, 100);
    assert!(follows_of(&world.state.pool, &ada_id).await.is_none());
    assert_eq!(book_count(&world.state.pool).await, 1);
    assert_eq!(world.sab.lock().await.adds.len(), 1);
}

#[tokio::test]
async fn follower_takes_over_when_the_leader_cancels() {
    let world = world().await;
    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.key_ok = true;
        let origin = prowl.origin.clone();
        prowl.releases = two_releases(&origin);
    }
    let created = ask(&world, "/works/OL893414W").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let tried = stored(&world.state.pool, &leader).await.tried_guids;
    assert!(tried.contains("g1"), "{tried}");
    let nzo = world.sab.lock().await.adds[0].nzo.clone();

    let ada = account(&world, "ada").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let ada_id = body["id"].as_str().unwrap().to_string();

    let (status, _) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{leader}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(stored(&world.state.pool, &leader).await.state, "canceled");
    assert!(world.sab.lock().await.deletes.iter().any(|id| id == &nzo));

    let promoted = stored(&world.state.pool, &ada_id).await;
    assert_eq!(promoted.state, "requested");
    assert!(follows_of(&world.state.pool, &ada_id).await.is_none());
    assert_eq!(promoted.attempts, 0);
    assert_eq!(promoted.tried_guids, tried);
    assert_eq!(promoted.next_check_at, promoted.updated_at);
    assert_eq!(promoted.progress, 0);

    worker::tick_at(&world.state, promoted.next_check_at)
        .await
        .unwrap();
    let promoted = stored(&world.state.pool, &ada_id).await;
    assert_eq!(promoted.state, "downloading");
    assert_eq!(promoted.attempts, 1);
    assert!(
        promoted.tried_guids.contains("g2"),
        "{}",
        promoted.tried_guids
    );
    let urls: Vec<String> = world
        .sab
        .lock()
        .await
        .adds
        .iter()
        .map(|add| add.name.clone())
        .collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[1].contains("/nzb/g2"), "{urls:?}");
}

#[tokio::test]
async fn follower_takes_over_when_the_leader_fails() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893414W").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let tried = stored(&world.state.pool, &leader).await.tried_guids;
    sqlx::query("UPDATE book_requests SET attempts = 3 WHERE id = ?")
        .bind(&leader)
        .execute(&world.state.pool)
        .await
        .unwrap();

    let ada = account(&world, "ada").await;
    let bea = account(&world, "bea").await;
    let carol = account(&world, "carol").await;
    let mut ids = Vec::new();
    for account in [&ada, &bea, &carol] {
        let (status, body) = post_work(&world, &account.token, "/works/OL893414W").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        ids.push(body["id"].as_str().unwrap().to_string());
    }

    fail_current(&world, "CRC error").await;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    let failed = stored(&world.state.pool, &leader).await;
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.error.as_deref(), Some("Download failed: CRC error"));

    let promoted = stored(&world.state.pool, &ids[0]).await;
    assert_eq!(promoted.state, "requested");
    assert_eq!(promoted.attempts, 0);
    assert_eq!(promoted.tried_guids, tried);
    assert!(follows_of(&world.state.pool, &ids[0]).await.is_none());
    assert_eq!(
        follows_of(&world.state.pool, &ids[1]).await.as_deref(),
        Some(ids[0].as_str())
    );
    assert_eq!(
        follows_of(&world.state.pool, &ids[2]).await.as_deref(),
        Some(ids[0].as_str())
    );
    let leaders: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE work_key = '/works/OL893414W' AND follows IS NULL
           AND state IN ('requested', 'downloading')",
    )
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(leaders, 1);
}

#[tokio::test]
async fn one_leader_under_concurrent_requests() {
    let world = world().await;
    let ada = account(&world, "ada").await;
    let bea = account(&world, "bea").await;
    let work = "/works/OLCONC";
    let (left, right) = tokio::join!(
        post_work(&world, &ada.token, work),
        post_work(&world, &bea.token, work),
    );
    assert_eq!(left.0, StatusCode::CREATED, "{}", left.1);
    assert_eq!(right.0, StatusCode::CREATED, "{}", right.1);
    let leaders: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE work_key = ? AND follows IS NULL AND state = 'requested'",
    )
    .bind(work)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    let followers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE work_key = ? AND follows IS NOT NULL AND state = 'requested'",
    )
    .bind(work)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(leaders, 1);
    assert_eq!(followers, 1);

    let err = sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, state, next_check_at, created_at, updated_at)
         VALUES ('second-leader', ?, ?, 'Dune', 'Frank Herbert', 'requested', 1, 1, 1)",
    )
    .bind(&world.user_id)
    .bind(work)
    .execute(&world.state.pool)
    .await
    .unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("work_key") && !message.contains("user_id"),
        "{message}"
    );
}

#[tokio::test]
async fn followers_and_reuses_do_not_count_toward_the_cap() {
    let world = world().await;
    let ada = account(&world, "ada").await;
    for n in 0..5 {
        let (status, body) = post_work(&world, &ada.token, &format!("/works/OLCAP{n}W")).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, body) = post_work(&world, &ada.token, "/works/OLCAP6W").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "You have 5 requests in progress.");

    let (status, body) = post_work(&world, &world.token, "/works/OLCAPLEAD").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = post_work(&world, &ada.token, "/works/OLCAPLEAD").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["state"], "requested");
    assert!(follows_of(&world.state.pool, body["id"].as_str().unwrap())
        .await
        .is_some());
    let (status, body) = post_work(&world, &ada.token, "/works/OLCAP7W").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "You have 5 requests in progress.");
    let leaders: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND follows IS NULL AND state IN ('requested', 'downloading')",
    )
    .bind(&ada.id)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(leaders, 5);
}

async fn give_up(world: &World, token: &str, work: &str) -> String {
    let (status, body) = post_work(world, token, work).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_string();
    let now = body["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    worker::tick_at(&world.state, now + 14 * DAY + 1)
        .await
        .unwrap();
    assert_eq!(stored(&world.state.pool, &id).await.state, "failed");
    id
}

#[tokio::test]
async fn retry_reuses_follows_or_leads() {
    let world = world().await;
    let ada = account(&world, "ada").await;
    let reuse = give_up(&world, &ada.token, "/works/OLREUSE").await;
    let follow = give_up(&world, &ada.token, "/works/OLFOLLOW").await;
    let lead = give_up(&world, &ada.token, "/works/OLLEAD").await;
    world.prowlarr.lock().await.key_ok = true;

    let book_id = bring_in(&world, "/works/OLREUSE").await;
    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{reuse}/retry"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "imported");
    assert_eq!(body["bookId"], book_id);
    assert_eq!(body["progress"], 100);
    assert_eq!(book_count(&world.state.pool).await, 1);

    let created = ask(&world, "/works/OLFOLLOW").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    let adds = world.sab.lock().await.adds.len();
    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{follow}/retry"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "downloading");
    assert_eq!(body["requester"], "ada");
    assert!(body.get("follows").is_none(), "{body}");
    assert_eq!(
        follows_of(&world.state.pool, &follow).await.as_deref(),
        Some(leader.as_str())
    );
    assert_eq!(world.sab.lock().await.adds.len(), adds);

    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{lead}/retry"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "requested");
    assert!(follows_of(&world.state.pool, &lead).await.is_none());
    let due = stored(&world.state.pool, &lead).await.next_check_at;
    worker::tick_at(&world.state, due).await.unwrap();
    let row = stored(&world.state.pool, &lead).await;
    assert_eq!(row.state, "downloading");
    assert_eq!(row.attempts, 1);
}

async fn say(world: &World, token: &str, book: &str, text: &str) {
    let (status, body) = json_req(
        &world.app,
        "POST",
        &format!("/v1/books/{book}/comments/0"),
        Some(token),
        json!({ "text": text }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn thread_texts(world: &World, token: &str, book: &str) -> Vec<String> {
    let (status, thread) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}/comments/0"),
        Some(token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    thread["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["text"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn private_holders_do_not_see_each_others_activity() {
    let world = world().await;
    let book = bring_in(&world, "/works/OL893414W").await;
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    say(&world, &world.token, &book, "owner note").await;
    say(&world, &ada.token, &book, "holder note").await;
    for token in [&world.token, &ada.token] {
        let (status, body) = empty_req(
            &world.app,
            "PUT",
            &format!("/v1/books/{book}/posts/0/repost"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, body) = empty_req(
            &world.app,
            "POST",
            &format!("/v1/books/{book}/likes/0"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["ids"], json!(["0"]));
    }
    let likes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM likes WHERE book_id = ? AND post_id = '0'")
            .bind(&book)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(likes, 2);

    let (status, _) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    for (token, name, text) in [
        (world.token.as_str(), "matt", "owner note"),
        (ada.token.as_str(), "ada", "holder note"),
    ] {
        let (status, index) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/books/{book}/comments"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{index}");
        assert_eq!(index["counts"]["0"], 1, "{name} {index}");
        assert_eq!(index["reposts"]["0"], 1, "{name} {index}");

        let (status, thread) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/books/{book}/comments/0"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{thread}");
        assert_eq!(thread["comments"].as_array().unwrap().len(), 1);
        assert_eq!(thread["comments"][0]["text"], text);
        assert_eq!(thread["comments"][0]["username"], name);

        let (status, state) = empty_req(
            &world.app,
            "PUT",
            &format!("/v1/books/{book}/posts/0/repost"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{state}");
        assert_eq!(state["reposts"], 1);
        assert_eq!(state["reposted"], true);

        let (status, feed) = empty_req(&world.app, "GET", "/v1/feed", Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{feed}");
        let items = feed["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "{name} {feed}");
        assert!(items.iter().all(|item| item["actor"]["username"] == name));

        let (status, posts) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/users/{name}/posts"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{posts}");
        assert_eq!(posts["items"].as_array().unwrap().len(), 1);
        let (status, reposts) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/users/{name}/reposts"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reposts}");
        assert_eq!(reposts["items"].as_array().unwrap().len(), 1);
        let (status, replies) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/profiles/{name}/replies"),
            Some(token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{replies}");
        assert_eq!(replies["replies"].as_array().unwrap().len(), 1);
        let (status, profile) =
            empty_req(&world.app, "GET", &format!("/v1/users/{name}"), Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{profile}");
        assert_eq!(profile["postCount"], 1);
        assert_eq!(profile["repostCount"], 1);
    }

    for (viewer, subject) in [(&ada.token, "matt"), (&world.token, "ada")] {
        let (status, posts) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/users/{subject}/posts"),
            Some(viewer),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{posts}");
        assert_eq!(posts["items"].as_array().unwrap().len(), 0, "{posts}");
        let (status, reposts) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/users/{subject}/reposts"),
            Some(viewer),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{reposts}");
        assert_eq!(reposts["items"].as_array().unwrap().len(), 0, "{reposts}");
        let (status, replies) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/profiles/{subject}/replies"),
            Some(viewer),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{replies}");
        assert_eq!(replies["replies"].as_array().unwrap().len(), 0, "{replies}");
        let (status, profile) = empty_req(
            &world.app,
            "GET",
            &format!("/v1/users/{subject}"),
            Some(viewer),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{profile}");
        assert_eq!(profile["postCount"], 0, "{profile}");
        assert_eq!(profile["repostCount"], 0, "{profile}");
        let (status, feed) = empty_req(&world.app, "GET", "/v1/feed", Some(viewer)).await;
        assert_eq!(status, StatusCode::OK, "{feed}");
        assert!(feed["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["actor"]["username"] != subject));
    }
}

#[tokio::test]
async fn sharing_hides_replies_that_were_written_while_private() {
    let world = world().await;
    let book = bring_in(&world, "/works/OL893414W").await;
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;
    let (status, _) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED);
    say(&world, &world.token, &book, "owner note").await;
    say(&world, &ada.token, &book, "holder private").await;

    let (status, _) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&world.token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let carol_texts = thread_texts(&world, &carol.token, &book).await;
    assert_eq!(carol_texts, vec!["owner note".to_string()]);
    let ada_texts = thread_texts(&world, &ada.token, &book).await;
    assert!(
        ada_texts.contains(&"owner note".to_string()),
        "{ada_texts:?}"
    );
    assert!(
        ada_texts.contains(&"holder private".to_string()),
        "{ada_texts:?}"
    );
    let matt_texts = thread_texts(&world, &world.token, &book).await;
    assert_eq!(matt_texts, vec!["owner note".to_string()]);

    say(&world, &ada.token, &book, "holder public").await;
    let carol_texts = thread_texts(&world, &carol.token, &book).await;
    assert_eq!(
        carol_texts,
        vec!["owner note".to_string(), "holder public".to_string()]
    );
    assert!(!carol_texts.iter().any(|text| text == "holder private"));

    let (status, _) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&world.token),
        json!({ "shared": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let matt_texts = thread_texts(&world, &world.token, &book).await;
    assert_eq!(matt_texts, vec!["owner note".to_string()]);
    let ada_texts = thread_texts(&world, &ada.token, &book).await;
    assert!(ada_texts.contains(&"holder private".to_string()));
    assert!(ada_texts.contains(&"holder public".to_string()));
    assert!(!ada_texts.iter().any(|text| text == "owner note"));

    let (status, _) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&world.token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let carol_texts = thread_texts(&world, &carol.token, &book).await;
    assert!(carol_texts.iter().any(|text| text == "holder public"));
    assert!(carol_texts.iter().any(|text| text == "owner note"));
    assert!(!carol_texts.iter().any(|text| text == "holder private"));
}

#[tokio::test]
async fn the_private_flag_is_set_by_a_trigger() {
    let world = world().await;
    let ada = account(&world, "ada").await;
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at, is_sample, shared)
         VALUES ('private-book', ?, 'Dune', 'Frank', 'dune', 1, 0, 0),
                ('sample-book', ?, 'Sample', 'Mewo', 'mewo', 1, 1, 0),
                ('shared-book', ?, 'Shared', 'Frank', 'shared', 1, 0, 1)",
    )
    .bind(&world.user_id)
    .bind(&world.user_id)
    .bind(&world.user_id)
    .execute(&world.state.pool)
    .await
    .unwrap();

    for (id, user, book) in [
        ("c-holder", ada.id.as_str(), "private-book"),
        ("c-owner", world.user_id.as_str(), "private-book"),
        ("c-sample", ada.id.as_str(), "sample-book"),
        ("c-shared", ada.id.as_str(), "shared-book"),
    ] {
        sqlx::query(
            "INSERT INTO comments (id, user_id, book_id, post_id, text, created_at)
             VALUES (?, ?, ?, '0', 'hi', 1)",
        )
        .bind(id)
        .bind(user)
        .bind(book)
        .execute(&world.state.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO reposts (user_id, book_id, post_id, created_at) VALUES (?, ?, '0', 1)",
        )
        .bind(user)
        .bind(book)
        .execute(&world.state.pool)
        .await
        .unwrap();
    }
    let comment_private = async |id| -> i64 {
        sqlx::query_scalar("SELECT private FROM comments WHERE id = ?")
            .bind(id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap()
    };
    assert_eq!(comment_private("c-holder").await, 1);
    assert_eq!(comment_private("c-owner").await, 0);
    assert_eq!(comment_private("c-sample").await, 0);
    assert_eq!(comment_private("c-shared").await, 0);
    let repost_flag = async |book, user: &str| -> i64 {
        sqlx::query_scalar("SELECT private FROM reposts WHERE user_id = ? AND book_id = ?")
            .bind(user)
            .bind(book)
            .fetch_one(&world.state.pool)
            .await
            .unwrap()
    };
    assert_eq!(repost_flag("private-book", &ada.id).await, 1);
    assert_eq!(repost_flag("private-book", &world.user_id).await, 0);
    assert_eq!(repost_flag("sample-book", &ada.id).await, 0);
}

#[tokio::test]
async fn owner_delete_passes_the_book_to_the_earliest_holder() {
    let world = world().await;
    let book = bring_in(&world, "/works/OL893414W").await;
    let ada = account(&world, "ada").await;
    let bea = account(&world, "bea").await;
    for account in [&ada, &bea] {
        let (status, body) = post_work(&world, &account.token, "/works/OL893414W").await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    say(&world, &world.token, &book, "owner note").await;
    say(&world, &ada.token, &book, "ada note").await;
    say(&world, &bea.token, &book, "bea note").await;
    let (status, _) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/books/{book}/likes/0"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&world.token),
        json!({ "shared": true, "progressIndex": 4 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = epub_file(&world, &world.user_id, &book);
    let before = std::fs::read(&path).unwrap();

    let (status, _) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/books/{book}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let row: (String, String, i64) =
        sqlx::query_as("SELECT user_id, files_user_id, shared FROM books WHERE id = ?")
            .bind(&book)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(row.0, ada.id);
    assert_eq!(row.1, world.user_id);
    assert_eq!(row.2, 0);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!epub_file(&world, &ada.id, &book).exists());

    let matt_comments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE book_id = ? AND user_id = ?")
            .bind(&book)
            .bind(&world.user_id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(matt_comments, 0);
    let matt_likes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM likes WHERE book_id = ? AND user_id = ?")
            .bind(&book)
            .bind(&world.user_id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(matt_likes, 0);
    let matt_progress: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM book_progress WHERE book_id = ? AND user_id = ?")
            .bind(&book)
            .bind(&world.user_id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(matt_progress, 0);
    let matt_request: String = sqlx::query_scalar(
        "SELECT state FROM book_requests WHERE user_id = ? AND work_key = '/works/OL893414W'",
    )
    .bind(&world.user_id)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(matt_request, "canceled");

    let (status, _) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}"),
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, owned) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{owned}");
    assert_eq!(owned["mine"], true);
    assert_eq!(owned["held"], false);
    assert_eq!(owned["shared"], false);
    let (status, held) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}"),
        Some(&bea.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{held}");
    assert_eq!(held["held"], true);
    assert_eq!(held["ownerUsername"], "");

    let (status, _) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&ada.token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, thread) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}/comments/0"),
        Some(&bea.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let texts: Vec<&str> = thread["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["bea note"]);
    let ada_private: i64 =
        sqlx::query_scalar("SELECT private FROM comments WHERE book_id = ? AND user_id = ?")
            .bind(&book)
            .bind(&ada.id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(ada_private, 1);
}

#[tokio::test]
async fn holder_remove_drops_only_their_access() {
    let world = world().await;
    let book = bring_in(&world, "/works/OL893414W").await;
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let request_id = body["id"].as_str().unwrap().to_string();
    say(&world, &ada.token, &book, "holder private").await;
    let (status, _) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&ada.token),
        json!({ "progressIndex": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/books/{book}/likes/0"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, shared) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book}"),
        Some(&world.token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shared}");
    say(&world, &ada.token, &book, "holder public").await;
    let (status, denied) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/books/{book}"),
        Some(&carol.token),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{denied}");

    let (status, _) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/books/{book}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let request = stored(&world.state.pool, &request_id).await;
    assert_eq!(request.state, "canceled");
    assert!(request.book_id.is_none());
    let private_left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comments WHERE book_id = ? AND user_id = ? AND text = 'holder private'",
    )
    .bind(&book)
    .bind(&ada.id)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(private_left, 0);
    let public_left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comments WHERE book_id = ? AND text = 'holder public'",
    )
    .bind(&book)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(public_left, 1);
    let progress: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM book_progress WHERE book_id = ? AND user_id = ?")
            .bind(&book)
            .bind(&ada.id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(progress, 0);
    let likes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM likes WHERE book_id = ? AND user_id = ?")
            .bind(&book)
            .bind(&ada.id)
            .fetch_one(&world.state.pool)
            .await
            .unwrap();
    assert_eq!(likes, 0);
    let owner: String = sqlx::query_scalar("SELECT user_id FROM books WHERE id = ?")
        .bind(&book)
        .fetch_one(&world.state.pool)
        .await
        .unwrap();
    assert_eq!(owner, world.user_id);
    assert!(epub_file(&world, &world.user_id, &book).is_file());

    let (status, still) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert_eq!(still["held"], false);
    assert_eq!(still["shared"], true);
    assert_eq!(still["ownerUsername"], "matt");
}

#[tokio::test]
async fn migration_008_demotes_duplicate_active_requests() {
    let tmp = TempDir::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
    let pool = SqlitePool::connect(&url).await.unwrap();
    let migrator = sqlx::migrate!("./migrations");
    let mut conn = pool.acquire().await.unwrap();
    Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
    for migration in migrator.iter() {
        if migration.version < 8 {
            Migrate::apply(&mut *conn, migration).await.unwrap();
        }
    }
    drop(conn);
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('u1', 'ann', 'ann', 'x', 1), ('u2', 'bob', 'bob', 'x', 2)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, state, download_id, next_check_at, created_at, updated_at)
         VALUES
         ('m1-old', 'u1', '/works/OLM1', 'Dune', 'Frank', 'downloading', 'sab-1', 1, 1700000000001, 1),
         ('m1-new', 'u2', '/works/OLM1', 'Dune', 'Frank', 'requested', NULL, 1, 1700000000002, 1),
         ('m2-b', 'u1', '/works/OLM2', 'Dune', 'Frank', 'downloading', 'sab-2', 1, 1700000000005, 1),
         ('m2-a', 'u2', '/works/OLM2', 'Dune', 'Frank', 'requested', NULL, 1, 1700000000005, 1),
         ('m3-old', 'u1', '/works/OLM3', 'Dune', 'Frank', 'requested', NULL, 1, 1700000000010, 1),
         ('m3-new', 'u2', '/works/OLM3', 'Dune', 'Frank', 'downloading', 'sab-3', 1, 1700000000020, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let row = |id: &'static str, pool: SqlitePool| async move {
        sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            "SELECT state, follows, download_id FROM book_requests WHERE id = ?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let oldest = row("m1-old", pool.clone()).await;
    assert_eq!(oldest.0, "downloading");
    assert!(oldest.1.is_none());
    let newer = row("m1-new", pool.clone()).await;
    assert_eq!(newer.0, "requested");
    assert_eq!(newer.1.as_deref(), Some("m1-old"));
    let same_time = row("m2-a", pool.clone()).await;
    assert_eq!(same_time.0, "requested");
    assert_eq!(same_time.1.as_deref(), Some("m2-b"));
    let downloading = row("m2-b", pool.clone()).await;
    assert_eq!(downloading.0, "downloading");
    assert!(downloading.1.is_none());
    assert_eq!(downloading.2.as_deref(), Some("sab-2"));
    let older_requested = row("m3-old", pool.clone()).await;
    assert_eq!(older_requested.0, "requested");
    assert_eq!(older_requested.1.as_deref(), Some("m3-new"));
    let newer_download = row("m3-new", pool.clone()).await;
    assert_eq!(newer_download.0, "downloading");
    assert!(newer_download.1.is_none());
    assert_eq!(newer_download.2.as_deref(), Some("sab-3"));

    let err = sqlx::query(
        "INSERT INTO book_requests
         (id, user_id, work_key, title, author, state, next_check_at, created_at, updated_at)
         VALUES ('m1-third', 'u1', '/works/OLM1', 'Dune', 'Frank', 'requested', 1, 1700000000003, 1)",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("work_key"), "{err}");
    let files: Option<String> =
        sqlx::query_scalar("SELECT files_user_id FROM books WHERE id = 'missing'")
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert!(files.is_none());
}

#[tokio::test]
async fn canceled_follower_keeps_its_own_state() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893414W").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();
    world.sab.lock().await.jobs[0].percent = 40;
    worker::tick_at(&world.state, now + POLL).await.unwrap();
    assert_eq!(stored(&world.state.pool, &leader).await.progress, 40);

    let ada = account(&world, "ada").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let ada_id = body["id"].as_str().unwrap().to_string();

    let (status, _) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{ada_id}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(follows_of(&world.state.pool, &ada_id).await.is_none());

    let (status, mine) = empty_req(&world.app, "GET", "/v1/requests", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["items"][0]["id"], ada_id);
    assert_eq!(mine["items"][0]["state"], "canceled");
    assert_eq!(mine["items"][0]["progress"], 0);
    assert!(mine["items"][0]["bookId"].is_null());
    assert!(mine["items"][0].get("follows").is_none(), "{mine}");

    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let item = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["workKey"] == "/works/OL893414W")
        .unwrap();
    assert_eq!(item["request"]["id"], ada_id);
    assert_eq!(item["request"]["state"], "canceled");
    assert_eq!(item["request"]["progress"], 0);
    assert!(item["request"]["bookId"].is_null());

    let (status, all) =
        empty_req(&world.app, "GET", "/v1/requests?all=1", Some(&world.token)).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let admin = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == ada_id)
        .unwrap();
    assert_eq!(admin["state"], "canceled");
    assert_eq!(admin["progress"], 0);
    assert!(admin["bookId"].is_null());
    assert!(admin.get("follows").is_none(), "{admin}");

    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{leader}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{leader}")),
        "Dune.epub",
        &common::epub(),
    );
    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();
    assert_eq!(stored(&world.state.pool, &leader).await.state, "imported");
    let ada_row = stored(&world.state.pool, &ada_id).await;
    assert_eq!(ada_row.state, "canceled");
    assert!(ada_row.book_id.is_none());

    let (status, mine) = empty_req(&world.app, "GET", "/v1/requests", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["items"][0]["state"], "canceled");
    assert!(mine["items"][0]["bookId"].is_null(), "{mine}");
    let (status, page) = empty_req(
        &world.app,
        "GET",
        "/v1/catalog/search?q=dune&page=1",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let item = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["workKey"] == "/works/OL893414W")
        .unwrap();
    assert_eq!(item["request"]["state"], "canceled");
    assert!(item["request"]["bookId"].is_null(), "{item}");
}

#[tokio::test]
async fn holder_search_does_not_match_a_private_owners_name() {
    let world = world().await;
    let book_id = bring_in(&world, "/works/OL893414W").await;
    let ada = account(&world, "ada").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["bookId"], book_id);

    let (status, by_owner) = empty_req(
        &world.app,
        "GET",
        "/v1/search?q=matt&type=books",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_owner}");
    assert!(
        by_owner["items"].as_array().unwrap().is_empty(),
        "{by_owner}"
    );

    let (status, by_title) = empty_req(
        &world.app,
        "GET",
        "/v1/search?q=Dune&type=books",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{by_title}");
    assert_eq!(by_title["items"][0]["id"], book_id);

    let (status, owner_hits) = empty_req(
        &world.app,
        "GET",
        "/v1/search?q=matt&type=books",
        Some(&world.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{owner_hits}");
    assert_eq!(owner_hits["items"][0]["id"], book_id);

    let (status, shared) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book_id}"),
        Some(&world.token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shared}");
    let (status, named) = empty_req(
        &world.app,
        "GET",
        "/v1/search?q=matt&type=books",
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{named}");
    assert_eq!(named["items"][0]["id"], book_id);
}

#[tokio::test]
async fn sweep_promotes_followers_of_an_inactive_leader() {
    let world = world().await;
    {
        let mut prowl = world.prowlarr.lock().await;
        prowl.key_ok = true;
        let origin = prowl.origin.clone();
        prowl.releases = two_releases(&origin);
    }
    let created = ask(&world, "/works/OL893414W").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();

    let ada = account(&world, "ada").await;
    let bea = account(&world, "bea").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let ada_id = body["id"].as_str().unwrap().to_string();
    let (status, body) = post_work(&world, &bea.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let bea_id = body["id"].as_str().unwrap().to_string();

    // The residue of a cancel that read `leading` before a promotion committed:
    // the new leader is canceled and the followers still point at it.
    sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', download_id = NULL, follows = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(&leader)
    .execute(&world.state.pool)
    .await
    .unwrap();

    worker::tick_at(&world.state, now + POLL).await.unwrap();
    assert_eq!(stored(&world.state.pool, &leader).await.state, "canceled");
    assert!(follows_of(&world.state.pool, &ada_id).await.is_none());
    let promoted = stored(&world.state.pool, &ada_id).await;
    assert_eq!(promoted.state, "downloading");
    assert_eq!(promoted.attempts, 1);
    assert_eq!(
        follows_of(&world.state.pool, &bea_id).await.as_deref(),
        Some(ada_id.as_str())
    );
    assert_eq!(stored(&world.state.pool, &bea_id).await.state, "requested");
}

#[tokio::test]
async fn sweep_follows_an_existing_leader_and_the_tick_continues() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OLSTUCK").await;
    let canceled = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();

    let ada = account(&world, "ada").await;
    let bea = account(&world, "bea").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OLSTUCK").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let ada_id = body["id"].as_str().unwrap().to_string();
    sqlx::query(
        "UPDATE book_requests
         SET state = 'canceled', download_id = NULL, follows = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(&canceled)
    .execute(&world.state.pool)
    .await
    .unwrap();

    let (status, body) = post_work(&world, &bea.token, "/works/OLSTUCK").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let leader = body["id"].as_str().unwrap().to_string();
    assert!(follows_of(&world.state.pool, &leader).await.is_none());
    let (status, body) = post_work(&world, &world.token, "/works/OLOTHER").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let other = body["id"].as_str().unwrap().to_string();
    let due = stored(&world.state.pool, &other).await.next_check_at;

    worker::tick_at(&world.state, due).await.unwrap();
    assert_eq!(
        follows_of(&world.state.pool, &ada_id).await.as_deref(),
        Some(leader.as_str())
    );
    assert_eq!(stored(&world.state.pool, &ada_id).await.state, "requested");
    assert!(follows_of(&world.state.pool, &leader).await.is_none());
    assert_eq!(stored(&world.state.pool, &other).await.state, "downloading");
    worker::tick_at(&world.state, due + POLL).await.unwrap();
}

#[tokio::test]
async fn a_holder_of_a_shared_book_still_holds_it() {
    let world = world().await;
    let book_id = bring_in(&world, "/works/OL893414W").await;
    let ada = account(&world, "ada").await;
    let carol = account(&world, "carol").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["state"], "imported");

    let (status, shared) = json_req(
        &world.app,
        "PATCH",
        &format!("/v1/books/{book_id}"),
        Some(&world.token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shared}");

    let (status, library) = empty_req(&world.app, "GET", "/v1/library", Some(&ada.token)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    let held = library["books"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["id"] == book_id)
        .unwrap();
    assert_eq!(held["held"], true);
    assert_eq!(held["mine"], false);
    assert_eq!(held["shared"], true);
    assert_eq!(held["ownerUsername"], "matt");

    let (status, book) = empty_req(
        &world.app,
        "GET",
        &format!("/v1/books/{book_id}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{book}");
    assert_eq!(book["held"], true);
    assert_eq!(book["ownerUsername"], "matt");

    let (status, library) = empty_req(&world.app, "GET", "/v1/library", Some(&carol.token)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    let seen = library["books"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["id"] == book_id)
        .unwrap();
    assert_eq!(seen["held"], false);
    assert_eq!(seen["shared"], true);
    assert_eq!(seen["ownerUsername"], "matt");
}

#[tokio::test]
async fn retry_of_an_old_row_conflicts_with_a_live_request() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893414W").await;
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();

    let ada = account(&world, "ada").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let old = body["id"].as_str().unwrap().to_string();
    let (status, _) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{old}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let live = body["id"].as_str().unwrap().to_string();

    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{old}/retry"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "You already have a request for this book.");
    assert_eq!(stored(&world.state.pool, &old).await.state, "canceled");
    assert_eq!(stored(&world.state.pool, &live).await.state, "requested");
    assert!(follows_of(&world.state.pool, &live).await.is_some());
}

#[tokio::test]
async fn retry_reuse_does_not_import_a_second_row() {
    let world = world().await;
    world.prowlarr.lock().await.key_ok = true;
    let created = ask(&world, "/works/OL893414W").await;
    let leader = created["id"].as_str().unwrap().to_string();
    let now = created["createdAt"].as_i64().unwrap();
    worker::tick_at(&world.state, now).await.unwrap();

    let ada = account(&world, "ada").await;
    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let old = body["id"].as_str().unwrap().to_string();
    let (status, _) = empty_req(
        &world.app,
        "DELETE",
        &format!("/v1/requests/{old}"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let nzo = world.sab.lock().await.adds[0].nzo.clone();
    finish_job(&world, &nzo, &format!("/data/mewo-{leader}")).await;
    write_epub(
        &world.downloads.path().join(format!("mewo-{leader}")),
        "Dune.epub",
        &common::epub(),
    );
    worker::tick_at(&world.state, now + POLL * 2).await.unwrap();
    assert_eq!(stored(&world.state.pool, &leader).await.state, "imported");

    let (status, body) = post_work(&world, &ada.token, "/works/OL893414W").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["state"], "imported");
    let imported = body["id"].as_str().unwrap().to_string();
    assert_ne!(imported, old);

    let (status, body) = empty_req(
        &world.app,
        "POST",
        &format!("/v1/requests/{old}/retry"),
        Some(&ada.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], imported);
    assert_eq!(body["state"], "imported");
    assert_eq!(body["bookId"], leader);
    assert_eq!(stored(&world.state.pool, &old).await.state, "canceled");
    assert!(stored(&world.state.pool, &old).await.book_id.is_none());
    let imported_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM book_requests
         WHERE user_id = ? AND work_key = '/works/OL893414W' AND state = 'imported'",
    )
    .bind(&ada.id)
    .fetch_one(&world.state.pool)
    .await
    .unwrap();
    assert_eq!(imported_rows, 1);
}

async fn catalog_app(catalog: Arc<Mutex<Catalog>>) -> (AppState, axum::Router, TempDir, String) {
    let data = TempDir::new().unwrap();
    let origin = serve(library_router(catalog)).await;
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: origin,
        auth: AuthConfig::default(),
    })
    .await
    .unwrap();
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    grant(&state.pool, "matt").await;
    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "requestsEnabled": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (state, app, data, token)
}

fn trend(key: &str, title: &str, language: Value) -> Value {
    json!({
        "key": key,
        "title": title,
        "author_name": ["Ada"],
        "first_publish_year": 1990,
        "cover_i": 3,
        "language": language
    })
}

fn titles_of(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["title"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn catalog_browse_pages_languages_and_shares_one_fetch() {
    let mut trending = vec![
        trend("/works/OL1W", "German only", json!(["ger"])),
        trend("/works/OL2W", "No language", json!([])),
    ];
    for n in 0..21 {
        trending.push(trend(
            &format!("/works/OL{}W", 100 + n),
            &format!("En {n}"),
            json!(["eng"]),
        ));
    }
    let catalog = Arc::new(Mutex::new(Catalog {
        trending,
        trending_delay_ms: 400,
        ..Catalog::default()
    }));
    let (state, app, _data, token) = catalog_app(Arc::clone(&catalog)).await;
    let app2 = app.clone();
    let token2 = token.clone();
    let (first, second) = tokio::join!(
        empty_req(&app, "GET", "/v1/catalog/search?page=1", Some(&token)),
        empty_req(&app2, "GET", "/v1/catalog/search", Some(&token2)),
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    assert_eq!(titles_of(&first.1), titles_of(&second.1));
    assert_eq!(first.1["items"].as_array().unwrap().len(), 20);
    assert_eq!(first.1["nextPage"], 2);
    assert_eq!(first.1["items"][0]["title"], "No language");
    assert_eq!(first.1["items"][0]["onServer"], Value::Null);
    assert!(!titles_of(&first.1)
        .iter()
        .any(|title| title == "German only"));
    assert_eq!(catalog.lock().await.hits, vec!["trending".to_string()]);

    let (status, rest) = empty_req(&app, "GET", "/v1/catalog/search?page=2", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{rest}");
    assert_eq!(rest["items"].as_array().unwrap().len(), 2);
    assert!(rest["nextPage"].is_null(), "{rest}");
    assert_eq!(catalog.lock().await.hits.len(), 1);

    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "languages": ["en", "de"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, page) = empty_req(&app, "GET", "/v1/catalog/search?page=1", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["title"], "German only");
    let (status, rest) = empty_req(&app, "GET", "/v1/catalog/search?page=2", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{rest}");
    assert_eq!(rest["items"].as_array().unwrap().len(), 3);
    assert!(rest["nextPage"].is_null(), "{rest}");
    assert_eq!(catalog.lock().await.hits.len(), 1);
    let _ = state;
}

#[tokio::test]
async fn catalog_browse_does_not_refetch_a_fresh_outage() {
    let catalog = Arc::new(Mutex::new(Catalog {
        trending_status: 500,
        ..Catalog::default()
    }));
    let (_state, app, _data, token) = catalog_app(Arc::clone(&catalog)).await;
    for _ in 0..3 {
        let (status, body) = empty_req(&app, "GET", "/v1/catalog/search", Some(&token)).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
        assert_eq!(body["error"], "Open Library is not answering. Try again.");
    }
    assert_eq!(catalog.lock().await.hits, vec!["trending".to_string()]);
}

async fn shelf_book(pool: &SqlitePool, id: &str, user_id: &str, shared: i64) {
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at, shared)
         VALUES (?, ?, 'Dune', 'Frank Herbert', 'frank', 1, ?)",
    )
    .bind(id)
    .bind(user_id)
    .bind(shared)
    .execute(pool)
    .await
    .unwrap();
}

async fn shelf_request(
    pool: &SqlitePool,
    id: &str,
    user_id: &str,
    work_key: &str,
    book_id: &str,
    at: i64,
) {
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, state, progress, tried_guids,
            book_id, next_check_at, created_at, updated_at
         ) VALUES (?, ?, ?, 'Dune', 'Frank Herbert', 'imported', 100, '[]', ?, 0, ?, ?)",
    )
    .bind(id)
    .bind(user_id)
    .bind(work_key)
    .bind(book_id)
    .bind(at)
    .bind(at)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn catalog_on_server_follows_what_the_caller_can_open() {
    let catalog = Arc::new(Mutex::new(Catalog::default()));
    let (state, app, _data, _admin) = catalog_app(catalog).await;
    let alice_token = register(&app, "alice", "password1").await;
    let alice: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    register(&app, "bob", "password1").await;
    let bob: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let work = "/works/OL893414W";

    shelf_book(&state.pool, "bob-book", &bob, 0).await;
    shelf_request(&state.pool, "bob-req", &bob, work, "bob-book", 10).await;
    let (status, page) =
        empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&alice_token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page["items"][0]["onServer"].is_null(), "{page}");

    sqlx::query("UPDATE books SET shared = 1 WHERE id = 'bob-book'")
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, page) =
        empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&alice_token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["onServer"]["bookId"], "bob-book");
    assert_eq!(page["items"][0]["onServer"]["ownerUsername"], "bob");
    assert_eq!(page["items"][0]["onServer"]["mine"], false);
    assert_eq!(page["items"][0]["onServer"]["shared"], true);
    assert_eq!(page["items"][0]["onServer"]["held"], false);

    shelf_book(&state.pool, "alice-book", &alice, 1).await;
    shelf_request(&state.pool, "alice-req", &alice, work, "alice-book", 5).await;
    let (status, page) =
        empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&alice_token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["onServer"]["bookId"], "alice-book");
    assert_eq!(page["items"][0]["onServer"]["mine"], true);
    assert_eq!(page["items"][0]["onServer"]["ownerUsername"], "alice");

    sqlx::query("UPDATE book_requests SET state = 'canceled' WHERE id = 'alice-req'")
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, page) =
        empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&alice_token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["onServer"]["bookId"], "bob-book");

    sqlx::query("DELETE FROM books WHERE id = 'bob-book'")
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, page) =
        empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&alice_token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page["items"][0]["onServer"].is_null(), "{page}");

    shelf_book(&state.pool, "bob-private", &bob, 0).await;
    shelf_request(&state.pool, "bob-again", &bob, work, "bob-private", 20).await;
    shelf_request(&state.pool, "alice-hold", &alice, work, "bob-private", 21).await;
    let (status, page) =
        empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&alice_token)).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"][0]["onServer"]["bookId"], "bob-private");
    assert_eq!(page["items"][0]["onServer"]["ownerUsername"], "");
    assert_eq!(page["items"][0]["onServer"]["mine"], false);
    assert_eq!(page["items"][0]["onServer"]["shared"], false);
    assert_eq!(page["items"][0]["onServer"]["held"], true);
    let _ = bob;
}

#[tokio::test]
async fn work_detail_follows_redirects_leaders_and_negative_cache() {
    let target = "/works/OL893414W";
    let asked = "/works/OL893415W";
    let catalog = Arc::new(Mutex::new(Catalog {
        json_redirect: true,
        merged: Some((asked.into(), target.into())),
        work_docs: HashMap::from([(
            target.into(),
            json!({
                "title": "Dune",
                "description": { "value": "A [desert](https://example.com) planet." },
                "subjects": ["Fiction, general", "Dune", "award:hugo_award=1966", "nyt:hardcover", "dune", "Arrakis"],
                "covers": [11481354],
                "series": ["Dune Saga"]
            }),
        )]),
        detail_docs: HashMap::from([(
            target.into(),
            json!({
                "key": target,
                "title": "Ignored",
                "subtitle": "The first book",
                "author_name": ["Frank Herbert"],
                "first_publish_year": 1965,
                "cover_i": 1,
                "edition_count": 155,
                "number_of_pages_median": 608.4,
                "language": ["eng"],
                "ratings_average": 4.25,
                "ratings_count": 448
            }),
        )]),
        ..Catalog::default()
    }));
    let (state, app, _data, admin) = catalog_app(Arc::clone(&catalog)).await;
    let ada_token = register(&app, "ada", "password1").await;
    let bob_token = register(&app, "bob", "password1").await;
    let _ = ada_token;
    let ada: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'ada'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let bob: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, state, progress, tried_guids, attempts,
            release_title, release_size, next_check_at, created_at, updated_at
         ) VALUES (
            'lead', ?, ?, 'Dune', 'Frank Herbert', 'downloading', 40, '[]', 2,
            'Dune Retail', 99, 50, 10, 10
         )",
    )
    .bind(&ada)
    .bind(asked)
    .execute(&state.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, state, progress, tried_guids, attempts,
            follows, next_check_at, created_at, updated_at
         ) VALUES (
            'follow', ?, ?, 'Dune', 'Frank Herbert', 'requested', 0, '[]', 0,
            'lead', 1, 11, 11
         )",
    )
    .bind(&bob)
    .bind(asked)
    .execute(&state.pool)
    .await
    .unwrap();

    let (status, body) = empty_req(&app, "GET", "/v1/catalog/works/nope", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Work id must look like OL123W.");
    let (status, _) = empty_req(&app, "GET", "/v1/catalog/works/OLW", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) =
        empty_req(&app, "GET", "/v1/catalog/works/OL893415W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["workKey"], asked);
    assert!(body["infoError"].is_null(), "{body}");
    let info = &body["info"];
    assert_eq!(info["canonicalKey"], target);
    assert_eq!(info["title"], "Dune");
    assert_eq!(info["subtitle"], "The first book");
    assert_eq!(info["description"], "A desert planet.");
    assert_eq!(info["subjects"], json!(["Dune", "Arrakis"]));
    assert_eq!(info["awards"], json!(["Hugo Award 1966"]));
    assert_eq!(info["series"], json!(["Dune Saga"]));
    assert_eq!(info["coverId"], 11481354);
    assert_eq!(info["editionCount"], 155);
    assert_eq!(info["pagesMedian"], 608);
    assert_eq!(info["languages"], json!(["English"]));
    assert_eq!(info["hasAcceptedEdition"], true);
    assert_eq!(info["rating"]["average"], 4.3);
    assert_eq!(info["rating"]["count"], 448);
    let request = &body["status"]["request"];
    assert_eq!(request["phase"], "downloading");
    assert_eq!(request["state"], "downloading");
    assert_eq!(request["progress"], 40);
    assert_eq!(request["attempts"], 2);
    assert_eq!(request["maxAttempts"], 3);
    assert!(request["nextCheckAt"].is_null(), "{request}");
    assert_eq!(request["releaseTitle"], "Dune Retail");
    assert_eq!(request["releaseSize"], 99);
    assert_eq!(request["requester"], "bob");
    assert_eq!(request["workKey"], asked);
    assert_eq!(body["status"]["active"], 0);
    assert_eq!(body["status"]["cap"], 5);
    assert_eq!(body["status"]["otherRequests"], json!([]));

    let (status, body) = empty_req(&app, "GET", "/v1/catalog/works/OL893415W", Some(&admin)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["status"]["request"].is_null(), "{body}");
    let others = body["status"]["otherRequests"].as_array().unwrap();
    assert_eq!(others.len(), 2, "{others:?}");
    assert_eq!(others[0]["requester"], "bob");
    assert_eq!(others[0]["follows"], "ada");
    assert_eq!(others[1]["requester"], "ada");
    assert!(others[1].get("follows").is_none(), "{others:?}");
    let hits = catalog.lock().await.hits.len();

    sqlx::query("UPDATE book_requests SET progress = 100 WHERE id = 'lead'")
        .execute(&state.pool)
        .await
        .unwrap();
    let (status, body) =
        empty_req(&app, "GET", "/v1/catalog/works/OL893415W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"]["request"]["phase"], "importing");
    assert_eq!(body["status"]["request"]["progress"], 100);
    assert_eq!(catalog.lock().await.hits.len(), hits);

    sqlx::query(
        "UPDATE book_requests
         SET state = 'requested', progress = 0, error = 'No release yet.', next_check_at = 999
         WHERE id = 'lead'",
    )
    .execute(&state.pool)
    .await
    .unwrap();
    let (status, body) =
        empty_req(&app, "GET", "/v1/catalog/works/OL893415W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"]["request"]["phase"], "waiting");
    assert_eq!(body["status"]["request"]["nextCheckAt"], 999);
    assert_eq!(body["status"]["request"]["attempts"], 2);

    sqlx::query(
        "UPDATE book_requests
         SET state = 'downloading', progress = 100, error = 'SABnzbd: paused'
         WHERE id = 'lead'",
    )
    .execute(&state.pool)
    .await
    .unwrap();
    let (status, body) =
        empty_req(&app, "GET", "/v1/catalog/works/OL893415W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"]["request"]["phase"], "held");

    let (status, body) =
        empty_req(&app, "GET", "/v1/catalog/works/OL893414W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["info"]["canonicalKey"], target);
    // The row lives on the merged-away key. This call asked for the canonical
    // id, so it has no alias to search. The redirect response above finds it.
    assert!(body["status"]["request"].is_null(), "{body}");

    catalog.lock().await.status = 500;
    let before = catalog.lock().await.hits.len();
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/works/OL9W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["info"].is_null(), "{body}");
    assert_eq!(body["infoError"], "Open Library is not answering.");
    assert_eq!(body["status"]["cap"], 5);
    assert!(body["status"]["request"].is_null(), "{body}");
    let (status, again) = empty_req(&app, "GET", "/v1/catalog/works/OL9W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["infoError"], "Open Library is not answering.");
    assert_eq!(catalog.lock().await.hits.len(), before + 1);

    catalog.lock().await.status = 404;
    let before = catalog.lock().await.hits.len();
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/works/OL8W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["info"].is_null(), "{body}");
    assert_eq!(
        body["infoError"],
        "Open Library has no record of this work."
    );
    let (status, _) = empty_req(&app, "GET", "/v1/catalog/works/OL8W", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog.lock().await.hits.len(), before + 1);
}
