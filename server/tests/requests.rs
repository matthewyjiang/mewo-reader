//! Book requests against in-process Prowlarr, SABnzbd, and Open Library.

mod common;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use http_body_util::BodyExt;
use mewo_server::config::{Config, MIN_EPUB_BYTES};
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
    token: String,
    user_id: String,
}

async fn world() -> World {
    let downloads = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let prowlarr = Arc::new(Mutex::new(Prowl::default()));
    let sab = Arc::new(Mutex::new(Sab::default()));
    let prowlarr_url = serve(prowlarr_router(prowlarr.clone())).await;
    let sab_url = serve(sab_router(sab.clone())).await;
    let library_url = serve(library_router()).await;
    prowlarr.lock().await.origin = prowlarr_url.clone();
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: data.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 64 * 1024 * 1024,
        downloads_dir: Some(downloads.path().to_path_buf()),
        openlibrary_url: library_url,
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

fn library_router() -> Router {
    Router::new().route(
        "/search.json",
        get(|Query(query): Query<HashMap<String, String>>| async move {
            let page: i64 = query
                .get("page")
                .and_then(|page| page.parse().ok())
                .unwrap_or(1);
            Json(json!({
                "numFound": 1,
                "docs": [{
                    "key": "/works/OL893415W",
                    "title": "Dune",
                    "author_name": ["Frank Herbert"],
                    "first_publish_year": 1965,
                    "cover_i": 11481354
                }],
                "page": page
            }))
        }),
    )
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
    assert_eq!(page["items"][0]["workKey"], "/works/OL893415W");
    assert_eq!(page["items"][0]["title"], "Dune");
    assert!(page["items"][0]["request"].is_null());

    let created = ask(&world, "/works/OL893415W").await;
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
    let created = ask(&world, "/works/OL893415W").await;
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
    assert_eq!(body["error"], "HTTP 500");
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=", Some(&token)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Enter a search.");
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
    let id = bring_in(&world, "/works/OL893415W").await;
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
            "workKey": "/works/OL893415W",
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
    })
    .await
    .unwrap();
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let (status, body) = empty_req(&app, "GET", "/v1/catalog/search?q=dune", Some(&token)).await;
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
    assert!(body["downloadsDir"].is_null());
    assert!(body.get("prowlarrApiKey").is_none());
}
