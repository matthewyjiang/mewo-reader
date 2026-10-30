use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use mewo_server::build;
use mewo_server::cli::{self, AdminCommand};
use mewo_server::config::{
    request_body_limit, AuthConfig, Config, LimitsConfig, Rate, MAX_COVER_BYTES, MIN_EPUB_BYTES,
};
use mewo_server::disk::DiskGuard;
use mewo_server::{prepare, router};
use serde_json::{json, Value};
use sqlx::migrate::Migrate;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tower::ServiceExt;

mod common;

async fn app_with(allow_signup: bool, max_epub_bytes: u64) -> (axum::Router, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup,
        max_epub_bytes,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits: test_limits(),
    };
    let router = build(config).await.unwrap();
    (router, tmp)
}

async fn app() -> (axum::Router, TempDir) {
    app_with(true, 1024 * 1024).await
}

async fn send(app: &axum::Router, req: Request<Body>) -> (StatusCode, Bytes) {
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body)
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
    let req = builder.body(Body::from(body.to_string())).unwrap();
    let (status, bytes) = send(app, req).await;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, value)
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
    let req = builder.body(Body::empty()).unwrap();
    let (status, bytes) = send(app, req).await;
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
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

async fn login(app: &axum::Router, username: &str, password: &str) -> String {
    let (status, body) = json_req(
        app,
        "POST",
        "/v1/auth/login",
        None,
        json!({ "username": username, "password": password }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_string()
}

fn config_at(path: &Path, allow_signup: bool, max_epub_bytes: u64) -> Config {
    Config {
        listen: "127.0.0.1:0".into(),
        data_dir: path.to_path_buf(),
        web_dir: None,
        allow_signup,
        max_epub_bytes,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits: test_limits(),
    }
}

/// The suite runs on small disks. `0` turns the free-space floor off.
fn test_limits() -> LimitsConfig {
    LimitsConfig {
        min_free_bytes: 0,
        ..LimitsConfig::default()
    }
}

/// Existing suites register dozens of accounts from one process. The default
/// per-IP burst would turn those into 429s. Auth tests build a tighter config.
fn roomy_auth() -> AuthConfig {
    let every = Duration::from_secs(1);
    let rate = Rate {
        burst: 10_000,
        every,
    };
    AuthConfig {
        max_concurrent: 2,
        queue_wait: Duration::from_secs(30),
        login_ip: rate,
        login_user: rate,
        register_ip: rate,
        register_user: rate,
        max_keys: 10_000,
        trusted_proxy_hops: 0,
    }
}

async fn admin_cli(path: &Path, cmd: AdminCommand, stdin: &str) -> (i32, String, String) {
    let mut input = std::io::Cursor::new(stdin.as_bytes().to_vec());
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = cli::run(
        cmd,
        &config_at(path, true, MIN_EPUB_BYTES),
        &mut input,
        &mut out,
        &mut err,
    )
    .await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

async fn grant(path: &Path, username: &str) {
    let (code, out, err) = admin_cli(path, AdminCommand::Grant(username.to_string()), "").await;
    assert_eq!(code, 0, "{out}{err}");
}

async fn settings_doc(path: &Path) -> String {
    let pool = open_db(path).await;
    sqlx::query_scalar("SELECT doc FROM settings WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap()
}

fn multipart(fields: &[(&str, Option<&str>, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "----mewotest";
    let mut body = Vec::new();
    for (name, filename, data) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(filename) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
            }
            None => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
            }
        }
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn upload(
    app: &axum::Router,
    token: &str,
    title: &str,
    author: &str,
    epub: &[u8],
    cover: Option<&[u8]>,
    is_sample: bool,
) -> (StatusCode, Value) {
    let mut fields: Vec<(&str, Option<&str>, &[u8])> = vec![
        ("title", None, title.as_bytes()),
        ("author", None, author.as_bytes()),
        ("handle", None, b"author"),
        (
            "isSample",
            None,
            if is_sample {
                b"true".as_slice()
            } else {
                b"false".as_slice()
            },
        ),
        ("epub", Some("book.epub"), epub),
    ];
    if let Some(cover) = cover {
        fields.push(("cover", Some("cover.jpg"), cover));
    }
    upload_fields(app, token, &fields).await
}

async fn share(app: &axum::Router, token: &str, id: &str) {
    let (status, body) = json_req(
        app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(token),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["shared"], true);
}

async fn upload_fields(
    app: &axum::Router,
    token: &str,
    fields: &[(&str, Option<&str>, &[u8])],
) -> (StatusCode, Value) {
    let (content_type, body) = multipart(fields);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/books")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();
    let (status, bytes) = send(app, req).await;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn version_is_public() {
    let (app, _tmp) = app().await;
    let (status, body) = empty_req(&app, "GET", "/v1/version", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_version(&body);

    let (status, body) = empty_req(&app, "GET", "/v1/version", Some("not-a-session")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_version(&body);
}

fn assert_version(body: &Value) {
    let object = body.as_object().expect("version json");
    assert_eq!(object.len(), 2);
    assert_eq!(body["version"], mewo_server::version::version());
    match mewo_server::version::commit() {
        Some(sha) => assert_eq!(body["commit"], sha),
        None => assert!(body["commit"].is_null()),
    }
}

#[tokio::test]
async fn register_login_me_logout() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;

    let (status, body) = empty_req(&app, "GET", "/v1/me", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "matt");

    let (status, body) = json_req(
        &app,
        "POST",
        "/v1/auth/login",
        None,
        json!({ "username": "Matt", "password": "password1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let login_token = body["token"].as_str().unwrap();

    let (status, _) = empty_req(&app, "POST", "/v1/auth/logout", Some(login_token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = empty_req(&app, "GET", "/v1/me", Some(login_token)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_duplicate_and_bad_login() {
    let (app, _tmp) = app().await;
    register(&app, "matt", "password1").await;
    let (status, body) = json_req(
        &app,
        "POST",
        "/v1/auth/register",
        None,
        json!({ "username": "MATT", "password": "password1" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let (status, _) = json_req(
        &app,
        "POST",
        "/v1/auth/login",
        None,
        json!({ "username": "matt", "password": "wrongpass" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn signup_can_be_turned_off() {
    let (app, _tmp) = app_with(false, 1024).await;
    let (status, body) = json_req(
        &app,
        "POST",
        "/v1/auth/register",
        None,
        json!({ "username": "matt", "password": "password1" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Signups are off.");
}

#[tokio::test]
async fn library_needs_a_session() {
    let (app, _tmp) = app().await;
    let (status, _) = empty_req(&app, "GET", "/v1/library", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn upload_list_progress_like_download_delete() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;

    let (status, book) = upload(
        &app,
        &token,
        "Pride",
        "Jane",
        &common::epub(),
        Some(b"jpeg"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    assert_eq!(book["title"], "Pride");
    assert_eq!(book["isSample"], false);
    assert_eq!(book["mine"], true);
    assert_eq!(book["shared"], false);
    assert_eq!(book["ownerUsername"], "matt");

    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(library["books"].as_array().unwrap().len(), 1);

    let (status, patched) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&token),
        json!({ "progressIndex": 4, "postCount": 12 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["progressIndex"], 4);
    assert_eq!(patched["postCount"], 3); // Client's postCount was ignored.

    let (status, likes) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(likes["ids"], json!(["p1"]));

    let (status, likes) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(likes["ids"], json!([]));

    let (status, likes) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p2"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, listed) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["ids"], likes["ids"]);

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/books/{id}/epub"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), common::epub());

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/books/{id}/cover"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), b"jpeg");

    let (status, _) = empty_req(&app, "DELETE", &format!("/v1/books/{id}"), Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn second_user_sees_shared_book_but_cannot_delete() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    assert_eq!(book["mine"], true);
    share(&app, &matt, id).await;

    let (status, seen) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["title"], "Pride");
    assert_eq!(seen["mine"], false);
    assert_eq!(seen["progressIndex"], 0);

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/books/{id}/epub"))
        .header(header::AUTHORIZATION, format!("Bearer {ted}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), common::epub());

    let (status, body) = empty_req(&app, "DELETE", &format!("/v1/books/{id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(library["books"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn progress_and_likes_stay_per_person() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    share(&app, &matt, id).await;

    let (status, patched) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&matt),
        json!({ "progressIndex": 4 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["progressIndex"], 4);

    let (status, ted_book) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ted_book["progressIndex"], 0);

    let (status, _) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, ted_likes) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ted_likes["ids"], json!([]));
}

#[tokio::test]
async fn comments_are_public_on_the_shared_book() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    share(&app, &matt, id).await;

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
        json!({ "text": "this line hits" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["comments"].as_array().unwrap().len(), 1);
    assert_eq!(thread["comments"][0]["username"], "matt");
    assert_eq!(thread["comments"][0]["mine"], true);

    let (status, ted_thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "same" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ted_thread}");
    assert_eq!(ted_thread["comments"].as_array().unwrap().len(), 2);
    assert_eq!(ted_thread["comments"][0]["mine"], false);
    assert_eq!(ted_thread["comments"][1]["mine"], true);

    let (status, index) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/comments"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(index["counts"]["0"], 2);
    assert_eq!(index["mine"], json!(["0"]));

    let comment_id = ted_thread["comments"][0]["id"].as_str().unwrap();
    let (status, body) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/comments/0/{comment_id}"),
        Some(&ted),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let ted_id = ted_thread["comments"][1]["id"].as_str().unwrap();
    let (status, after) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/comments/0/{ted_id}"),
        Some(&ted),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["comments"].as_array().unwrap().len(), 1);

    let (status, empty) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "   " }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{empty}");

    let long = "x".repeat(2001);
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": long }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(error.contains("2000"), "{error}");
    assert!(error.contains("asked 2001"), "{error}");
}

#[tokio::test]
async fn client_comment_ids_replay_and_conflict() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let carol = register(&app, "carol", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    share(&app, &matt, id).await;
    let (status, other) = upload(&app, &matt, "Emma", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{other}");
    let other_id = other["id"].as_str().unwrap();
    share(&app, &matt, other_id).await;

    let reply_id = "6f0b9c3e-1a2b-4c5d-8e9f-0a1b2c3d4e5f";
    let (status, first) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "queued", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["comments"].as_array().unwrap().len(), 1);
    assert_eq!(first["comments"][0]["id"], reply_id);
    assert_eq!(first["comments"][0]["text"], "queued");

    // A retry can carry different text. The stored row, and the thread, stay the first one.
    let (status, again) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "queued again", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, first);

    let pool = open_db(tmp.path()).await;
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE id = ?")
        .bind(reply_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);

    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/1"),
        Some(&ted),
        json!({ "text": "other post", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "That reply id is taken.");

    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{other_id}/comments/0"),
        Some(&ted),
        json!({ "text": "other book", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "That reply id is taken.");

    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&carol),
        json!({ "text": "stolen", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "That reply id is taken.");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE id = ?")
        .bind(reply_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    let (status, thread) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread, first);

    // Omitted id still mints a server id, so two posts are two rows.
    let (status, minted) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "no id" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{minted}");
    let comments = minted["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert!(comments
        .iter()
        .any(|row| row["text"] == "no id" && row["id"] != reply_id));
}

#[tokio::test]
async fn a_private_reply_stays_private_after_the_book_is_shared() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let carol = register(&app, "carol", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let pool = open_db(tmp.path()).await;
    let ted_id = user_id(&pool, "ted").await;
    insert_imported(&pool, "hold-ted", &ted_id, "/works/OL1W", id, 1).await;

    let comment_private = async |comment_id| -> i64 {
        sqlx::query_scalar("SELECT private FROM comments WHERE id = ?")
            .bind(comment_id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let texts = |thread: &Value| -> Vec<String> {
        thread["comments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["text"].as_str().unwrap().to_string())
            .collect()
    };

    // Ted holds the private book. `private: false` must not clear the trigger.
    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "while private", "private": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let early_id = thread["comments"][0]["id"].as_str().unwrap();
    assert_eq!(comment_private(early_id).await, 1);

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "also private", "private": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let flagged_id = thread["comments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["text"] == "also private")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(comment_private(flagged_id).await, 1);

    share(&app, &matt, id).await;

    // Composed while the book was private, delivered after the share. The
    // trigger would publish this row. The flag keeps it private.
    let late_id = "8a1c2d3e-4f50-4612-9abc-def012345678";
    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "synced late", "id": late_id, "private": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(comment_private(late_id).await, 1);

    let (status, carol_thread) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&carol),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{carol_thread}");
    assert!(texts(&carol_thread).is_empty(), "{carol_thread}");

    let (status, owner_thread) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{owner_thread}");
    assert!(texts(&owner_thread).is_empty(), "{owner_thread}");

    let mut ted_texts = texts(&thread);
    ted_texts.sort();
    assert_eq!(
        ted_texts,
        vec![
            "also private".to_string(),
            "synced late".to_string(),
            "while private".to_string(),
        ]
    );

    // An owner's reply stays public. The flag does not hide it.
    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
        json!({ "text": "owner note", "private": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let owner_id = thread["comments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["text"] == "owner note")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(comment_private(owner_id).await, 0);

    let (status, carol_thread) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&carol),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{carol_thread}");
    assert_eq!(texts(&carol_thread), vec!["owner note".to_string()]);
    let (status, owner_thread) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{owner_thread}");
    assert_eq!(texts(&owner_thread), vec!["owner note".to_string()]);
}

#[tokio::test]
async fn put_and_delete_likes_are_idempotent() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();

    let (status, likes) = empty_req(
        &app,
        "PUT",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{likes}");
    assert_eq!(likes["ids"], json!(["p1"]));
    let (status, again) = empty_req(
        &app,
        "PUT",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["ids"], json!(["p1"]));

    let (status, likes) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{likes}");
    assert_eq!(likes["ids"], json!([]));
    let (status, again) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["ids"], json!([]));

    let (status, likes) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{likes}");
    assert_eq!(likes["ids"], json!(["p1"]));
    let (status, likes) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{likes}");
    assert_eq!(likes["ids"], json!([]));
}

#[tokio::test]
async fn reply_ids_must_be_lowercase_uuids() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let reply_id = "6f0b9c3e-1a2b-4c5d-8e9f-0a1b2c3d4e5f";
    let pool = open_db(tmp.path()).await;
    let stored = async || -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE book_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };

    for bad in [
        reply_id.to_ascii_uppercase(),
        reply_id.replace('-', ""),
        "not-a-uuid".to_string(),
        String::new(),
    ] {
        let (status, body) = json_req(
            &app,
            "POST",
            &format!("/v1/books/{id}/comments/0"),
            Some(&token),
            json!({ "text": "nope", "id": bad }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} {body}");
        assert_eq!(body["error"], "Reply id must be a lowercase UUID.");
        assert_eq!(stored().await, 0, "{bad}");
    }

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&token),
        json!({ "text": "kept", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["comments"][0]["id"], reply_id);
    assert_eq!(stored().await, 1);

    let (status, after) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/comments/0/{reply_id}"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert!(after["comments"].as_array().unwrap().is_empty(), "{after}");
}

#[tokio::test]
async fn likes_and_comments_accept_an_uppercase_book_id() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    let upper = id.to_ascii_uppercase();

    let (status, upper_like) = empty_req(
        &app,
        "PUT",
        &format!("/v1/books/{upper}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{upper_like}");
    let (status, lower_like) = empty_req(
        &app,
        "PUT",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{lower_like}");
    assert_eq!(upper_like, lower_like);
    assert_eq!(lower_like["ids"], json!(["p1"]));

    let (status, upper_listed) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{upper}/likes"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{upper_listed}");
    let (status, lower_listed) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{lower_listed}");
    assert_eq!(upper_listed, lower_listed);
    assert_eq!(upper_listed, lower_like);

    let (status, upper_gone) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{upper}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{upper_gone}");
    let (status, lower_gone) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{lower_gone}");
    assert_eq!(upper_gone, lower_gone);
    assert_eq!(lower_gone["ids"], json!([]));

    let (status, upper_toggle) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{upper}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{upper_toggle}");
    assert_eq!(upper_toggle["ids"], json!(["p1"]));
    let (status, _) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, lower_toggle) = empty_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/likes/p1"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{lower_toggle}");
    assert_eq!(lower_toggle, upper_toggle);

    let (status, upper_thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{upper}/comments/0"),
        Some(&token),
        json!({ "text": "from the upper id" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{upper_thread}");
    let (status, lower_thread) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{lower_thread}");
    assert_eq!(upper_thread, lower_thread);
    assert_eq!(lower_thread["comments"][0]["text"], "from the upper id");
    let (status, upper_get) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{upper}/comments/0"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{upper_get}");
    assert_eq!(upper_get, lower_thread);

    let comment_id = lower_thread["comments"][0]["id"].as_str().unwrap();
    let (status, deleted) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{upper}/comments/0/{comment_id}"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert!(
        deleted["comments"].as_array().unwrap().is_empty(),
        "{deleted}"
    );
    let (status, gone) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/0"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{gone}");
    assert_eq!(gone, deleted);
}

#[tokio::test]
async fn replaying_a_reply_does_not_change_privacy() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    share(&app, &matt, id).await;
    let reply_id = "6f0b9c3e-1a2b-4c5d-8e9f-0a1b2c3d4e5f";

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "public", "id": reply_id, "private": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let pool = open_db(tmp.path()).await;
    let private_flag = async || -> i64 {
        sqlx::query_scalar("SELECT private FROM comments WHERE id = ?")
            .bind(reply_id)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    assert_eq!(private_flag().await, 0);

    let (status, again) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "public", "id": reply_id, "private": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, thread);
    assert_eq!(private_flag().await, 0);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE id = ?")
        .bind(reply_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn a_deleted_reply_id_can_be_posted_again() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let reply_id = "6f0b9c3e-1a2b-4c5d-8e9f-0a1b2c3d4e5f";

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&token),
        json!({ "text": "first", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let (status, _) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{id}/comments/0/{reply_id}"),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, again) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&token),
        json!({ "text": "second", "id": reply_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["comments"].as_array().unwrap().len(), 1);
    assert_eq!(again["comments"][0]["id"], reply_id);
    assert_eq!(again["comments"][0]["text"], "second");
    let pool = open_db(tmp.path()).await;
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE id = ?")
        .bind(reply_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn profile_lists_that_users_replies() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, book) = upload(&app, &matt, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    share(&app, &matt, id).await;

    let (status, _) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
        json!({ "text": "first" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/1"),
        Some(&matt),
        json!({ "text": "later" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "ted here" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, matt_feed) = empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{matt_feed}");
    assert_eq!(matt_feed["username"], "matt");
    let replies = matt_feed["replies"].as_array().unwrap();
    assert_eq!(replies.len(), 2);
    let texts: Vec<&str> = replies
        .iter()
        .map(|row| row["comment"]["text"].as_str().unwrap())
        .collect();
    assert!(texts.contains(&"later"), "{matt_feed}");
    assert!(texts.contains(&"first"), "{matt_feed}");
    assert!(replies.iter().all(|row| row["comment"]["mine"] == false));
    assert!(replies.iter().any(|row| row["postId"] == "1"));

    let (status, missing) =
        empty_req(&app, "GET", "/v1/profiles/nobody/replies", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{missing}");
    assert_eq!(missing["replies"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn second_sample_returns_the_first() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, first) = upload(&app, &token, "Sample", "Mewo", &common::epub(), None, true).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) =
        upload(&app, &token, "Sample", "Mewo", &common::epub(), None, true).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(first["id"], second["id"]);
    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(library["books"].as_array().unwrap().len(), 1);

    let ted = register(&app, "ted", "password1").await;
    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    assert_eq!(library["books"].as_array().unwrap().len(), 1);
    assert_eq!(library["books"][0]["id"], first["id"]);
    assert_eq!(library["books"][0]["isSample"], true);
    assert_eq!(library["books"][0]["shared"], false);
    assert_eq!(library["books"][0]["mine"], false);
    assert_eq!(library["books"][0]["ownerUsername"], "matt");
    let sample_id = first["id"].as_str().unwrap();
    let (status, seen) =
        empty_req(&app, "GET", &format!("/v1/books/{sample_id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["title"], "Sample");

    let (status, ted_sample) =
        upload(&app, &ted, "Sample", "Mewo", &common::epub(), None, true).await;
    assert_eq!(status, StatusCode::OK, "{ted_sample}");
    assert_eq!(ted_sample["id"], first["id"]);
}

#[tokio::test]
async fn oversized_json_auth_is_413() {
    let (app, _tmp) = app().await;
    let body = "a".repeat(1024 * 1024 + 64);
    for path in ["/v1/auth/register", "/v1/auth/login"] {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.clone()))
            .unwrap();
        let (status, bytes) = send(&app, req).await;
        assert_eq!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "{path} {}",
            String::from_utf8_lossy(&bytes)
        );
    }
}

#[tokio::test]
async fn upload_text_field_stops_at_its_cap() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let long = "t".repeat(513);
    let (status, body) = upload(&app, &token, &long, "A", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Field is longer than 512 bytes.");

    let ok = "t".repeat(512);
    let (status, book) = upload(&app, &token, &ok, "A", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    assert_eq!(book["title"], ok);
}

#[tokio::test]
async fn oversized_epub_is_413() {
    // A seed below the floor is stored as 1 MiB, so a 200-byte file is accepted
    // and a file past that floor is 413 naming the clamped limit.
    let (app, _tmp) = app_with(true, 64).await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Ok", "A", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let big = vec![0u8; (MIN_EPUB_BYTES as usize) + 8];
    let (status, body) = upload(&app, &token, "Big", "A", &big, None, false).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains(&format!("limit is {MIN_EPUB_BYTES} bytes")),
        "{error}"
    );
}

#[tokio::test]
async fn epub_over_two_megabytes_is_accepted() {
    let (app, _tmp) = app_with(true, 4 * 1024 * 1024).await;
    let token = register(&app, "matt", "password1").await;
    let epub = common::book(true, 3 * 1024 * 1024);
    let (status, book) = upload(&app, &token, "Big", "A", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/books/{id}/epub"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.len(), epub.len());
}

#[tokio::test]
async fn bad_book_id_is_404() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    for path in [
        "/v1/books/../secret",
        "/v1/books/not-a-uuid",
        "/v1/books/../secret/epub",
    ] {
        let (status, body) = empty_req(&app, "GET", path, Some(&token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} {body}");
    }
}

#[tokio::test]
async fn epub_only_upload_uses_metadata_and_exposes_canonical_posts_and_cover() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let epub = common::epub();
    let (status, book) = upload_fields(&app, &token, &[("epub", Some("book.epub"), &epub)]).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    assert_eq!(book["title"], "Fixture Book");
    assert_eq!(book["author"], "Jane A. Writer");
    assert_eq!(book["handle"], "janeawriter");
    assert_eq!(book["postCount"], 3);
    let id = book["id"].as_str().unwrap();
    let (status, feed) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/posts"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(
        feed["posts"],
        json!([
            { "id": "0", "kind": "Heading", "text": "Chapter I", "chapter": "Chapter I" },
            { "id": "1", "kind": "Body", "text": "First\u{a0} paragraph & second.", "chapter": "Chapter I" },
            { "id": "2", "kind": "Body", "text": "Last paragraph.", "chapter": "Chapter I" },
        ])
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/books/{id}/cover"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        b"\x89PNG\r\n\x1a\nfixture"
    );
}

#[tokio::test]
async fn missing_posts_cache_is_filled_once_for_concurrent_readers() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Book", "Writer", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let mut owners = tokio::fs::read_dir(tmp.path().join("books")).await.unwrap();
    let dir = owners.next_entry().await.unwrap().unwrap().path().join(id);
    tokio::fs::remove_file(dir.join("posts.json"))
        .await
        .unwrap();
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        tmp.path().join("mewo.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE books SET post_count = 99 WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let path = format!("/v1/books/{id}/posts");
    let (first, second) = tokio::join!(
        empty_req(&app, "GET", &path, Some(&token)),
        empty_req(&app, "GET", &path, Some(&token))
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first, second);
    assert_eq!(first.1["posts"].as_array().unwrap().len(), 3);
    let cached: Value =
        serde_json::from_slice(&tokio::fs::read(dir.join("posts.json")).await.unwrap()).unwrap();
    assert_eq!(cached, first.1);
    let (_, book) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&token)).await;
    assert_eq!(book["postCount"], 3);
}

#[tokio::test]
async fn invalid_epub_is_400_and_does_not_enter_the_library() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, body) =
        upload_fields(&app, &token, &[("epub", Some("bad.epub"), b"not an epub")]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .starts_with("Could not read that EPUB: "));
    let (_, shelf) = empty_req(&app, "GET", "/v1/library", Some(&token)).await;
    assert_eq!(shelf["books"], json!([]));
    assert!(tokio::fs::read_dir(tmp.path().join("tmp"))
        .await
        .unwrap()
        .next_entry()
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn static_history_fallback_does_not_swallow_unknown_api_routes() {
    let tmp = TempDir::new().unwrap();
    let web = tmp.path().join("web");
    tokio::fs::create_dir(&web).await.unwrap();
    tokio::fs::write(web.join("index.html"), "<h1>Mewo</h1>")
        .await
        .unwrap();
    tokio::fs::write(web.join("app.js"), "console.log('Mewo')")
        .await
        .unwrap();
    let app = build(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().join("data"),
        web_dir: Some(web),
        allow_signup: true,
        max_epub_bytes: 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits: test_limits(),
    })
    .await
    .unwrap();
    for path in ["/", "/some/route"] {
        let (status, bytes) = send(
            &app,
            Request::builder().uri(path).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes.as_ref(), b"<h1>Mewo</h1>");
    }
    let (status, bytes) = send(
        &app,
        Request::builder()
            .uri("/app.js")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), b"console.log('Mewo')");
    for path in ["/v1", "/v1/", "/v1/nope", "/v1/books/nope/missing"] {
        let (status, body) = empty_req(&app, "GET", path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert_eq!(body["error"], "Not found.");
    }
}

#[tokio::test]
async fn private_book_is_invisible_on_every_route() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, book) = upload(
        &app,
        &matt,
        "Diary",
        "Matt",
        &common::epub(),
        Some(b"jpeg"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{book}");
    assert_eq!(book["shared"], false);
    assert_eq!(book["mine"], true);
    assert_eq!(book["ownerUsername"], "matt");
    let id = book["id"].as_str().unwrap();

    let (status, mine) = empty_req(&app, "GET", "/v1/library", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["books"].as_array().unwrap().len(), 1);
    assert_eq!(mine["books"][0]["shared"], false);

    let owner_cover = Request::builder()
        .uri(format!("/v1/books/{id}/cover"))
        .header(header::AUTHORIZATION, format!("Bearer {matt}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, owner_cover).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), b"jpeg");

    let (status, shelf) = empty_req(&app, "GET", "/v1/library", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert_eq!(shelf["books"], json!([]));

    let hidden = [
        format!("/v1/books/{id}"),
        format!("/v1/books/{id}/epub"),
        format!("/v1/books/{id}/cover"),
        format!("/v1/books/{id}/posts"),
        format!("/v1/books/{id}/likes"),
        format!("/v1/books/{id}/comments"),
        format!("/v1/books/{id}/comments/0"),
    ];
    for path in &hidden {
        let (status, body) = empty_req(&app, "GET", path, Some(&ted)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "GET {path} {body}");
        assert_eq!(body["error"], "Not found.");
    }
    for (method, path) in [
        ("POST", format!("/v1/books/{id}/likes/0")),
        ("PUT", format!("/v1/books/{id}/likes/0")),
        ("DELETE", format!("/v1/books/{id}/likes/0")),
        ("DELETE", format!("/v1/books/{id}")),
        ("DELETE", format!("/v1/books/{id}/comments/0/missing")),
    ] {
        let (status, body) = empty_req(&app, method, &path, Some(&ted)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path} {body}");
        assert_eq!(body["error"], "Not found.");
    }
    for body in [
        json!({ "progressIndex": 2 }),
        json!({ "shared": true }),
        json!({ "shared": false, "progressIndex": 1 }),
    ] {
        let (status, response) =
            json_req(&app, "PATCH", &format!("/v1/books/{id}"), Some(&ted), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{response}");
    }
    let (status, response) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "nope" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{response}");

    let (status, patched) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&matt),
        json!({ "progressIndex": 2 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["progressIndex"], 2);
    assert_eq!(patched["shared"], false);

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
        json!({ "text": "secret" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");

    let (status, ted_feed) = empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_feed}");
    assert_eq!(ted_feed["replies"], json!([]));
    let (status, matt_feed) =
        empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{matt_feed}");
    assert_eq!(matt_feed["replies"].as_array().unwrap().len(), 1);
    assert_eq!(matt_feed["replies"][0]["comment"]["text"], "secret");
    assert_eq!(matt_feed["replies"][0]["bookId"], id);
}

#[tokio::test]
async fn owner_can_share_a_book_and_a_non_owner_cannot_toggle_it() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, private_book) = upload(
        &app,
        &matt,
        "Diary",
        "Matt",
        &common::epub(),
        Some(b"jpeg"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{private_book}");
    let private_id = private_book["id"].as_str().unwrap();
    let (status, _) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{private_id}/comments/0"),
        Some(&matt),
        json!({ "text": "secret" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, book) = upload(
        &app,
        &matt,
        "Pride",
        "Jane",
        &common::epub(),
        Some(b"jpeg"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{book}");
    assert_eq!(book["shared"], false);
    let id = book["id"].as_str().unwrap().to_string();
    let (status, shared) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&matt),
        json!({ "shared": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shared}");
    assert_eq!(shared["shared"], true);
    assert_eq!(shared["mine"], true);
    assert_eq!(shared["ownerUsername"], "matt");

    let (status, _) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/1"),
        Some(&matt),
        json!({ "text": "public" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, shelf) = empty_req(&app, "GET", "/v1/library", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert_eq!(shelf["books"].as_array().unwrap().len(), 1);
    assert_eq!(shelf["books"][0]["id"], id);
    assert_eq!(shelf["books"][0]["shared"], true);
    assert_eq!(shelf["books"][0]["mine"], false);
    assert_eq!(shelf["books"][0]["ownerUsername"], "matt");

    let (status, seen) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{seen}");
    assert_eq!(seen["title"], "Pride");
    assert_eq!(seen["progressIndex"], 0);

    let epub = Request::builder()
        .uri(format!("/v1/books/{id}/epub"))
        .header(header::AUTHORIZATION, format!("Bearer {ted}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, epub).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), common::epub());
    let cover = Request::builder()
        .uri(format!("/v1/books/{id}/cover"))
        .header(header::AUTHORIZATION, format!("Bearer {ted}"))
        .body(Body::empty())
        .unwrap();
    let (status, bytes) = send(&app, cover).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes.as_ref(), b"jpeg");
    let (status, posts) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/posts"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{posts}");
    assert_eq!(posts["posts"].as_array().unwrap().len(), 3);

    let (status, patched) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&ted),
        json!({ "progressIndex": 4 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["progressIndex"], 4);
    assert_eq!(patched["shared"], true);
    let (status, owner) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{owner}");
    assert_eq!(owner["progressIndex"], 0);

    let (status, likes) =
        empty_req(&app, "POST", &format!("/v1/books/{id}/likes/1"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{likes}");
    assert_eq!(likes["ids"], json!(["1"]));
    let (status, likes) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{likes}");
    assert_eq!(likes["ids"], json!(["1"]));

    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/1"),
        Some(&ted),
        json!({ "text": "same" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    assert_eq!(thread["comments"].as_array().unwrap().len(), 2);
    let (status, index) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/comments"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(index["counts"]["1"], 2);
    let (status, listed) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments/1"),
        Some(&ted),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["comments"].as_array().unwrap().len(), 2);

    let (status, feed) = empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    let texts: Vec<&str> = feed["replies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["comment"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, vec!["public"]);

    let (status, rejected) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&ted),
        json!({ "shared": false, "progressIndex": 9 }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{rejected}");
    let (status, still) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{still}");
    assert_eq!(still["shared"], true);
    assert_eq!(still["progressIndex"], 4);

    let (status, hidden) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(&matt),
        json!({ "shared": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    assert_eq!(hidden["shared"], false);
    let (status, gone) = empty_req(&app, "GET", &format!("/v1/books/{id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{gone}");
    let (status, shelf) = empty_req(&app, "GET", "/v1/library", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{shelf}");
    assert_eq!(shelf["books"], json!([]));
    let (status, feed) = empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert_eq!(feed["replies"], json!([]));
    let (status, owner_feed) =
        empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{owner_feed}");
    assert_eq!(owner_feed["replies"].as_array().unwrap().len(), 2);
    let (status, own) = empty_req(&app, "GET", "/v1/profiles/ted/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{own}");
    assert_eq!(own["replies"], json!([]));

    let (status, _) = empty_req(&app, "GET", &format!("/v1/books/{private_id}"), Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn migration_marks_existing_books_shared() {
    let tmp = TempDir::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    let migrator = sqlx::migrate!("./migrations");
    let mut conn = pool.acquire().await.unwrap();
    Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
    for migration in migrator.iter() {
        if migration.version < 4 {
            Migrate::apply(&mut *conn, migration).await.unwrap();
        }
    }
    drop(conn);

    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('user', 'matt', 'matt', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at)
         VALUES ('book', 'user', 'Old', 'Author', 'author', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before = sqlx::query_scalar::<_, i64>("SELECT shared FROM books WHERE id = 'book'")
        .fetch_one(&pool)
        .await;
    assert!(before.is_err(), "shared column exists before migration 004");

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let shared: i64 = sqlx::query_scalar("SELECT shared FROM books WHERE id = 'book'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(shared, 1);

    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at)
         VALUES ('new', 'user', 'New', 'Author', 'author', 2)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let shared: i64 = sqlx::query_scalar("SELECT shared FROM books WHERE id = 'new'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(shared, 0);

    let index: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'books_shared_owner'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(index, 1);
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

fn search_path(q: &str, kind: &str, cursor: Option<&str>) -> String {
    let mut path = format!("/v1/search?q={}&type={kind}", percent_encode(q));
    if let Some(cursor) = cursor {
        path.push_str("&cursor=");
        path.push_str(&percent_encode(cursor));
    }
    path
}

async fn search(
    app: &axum::Router,
    token: Option<&str>,
    q: &str,
    kind: &str,
    cursor: Option<&str>,
) -> (StatusCode, Value) {
    empty_req(app, "GET", &search_path(q, kind, cursor), token).await
}

async fn search_all(app: &axum::Router, token: &str, q: &str, kind: &str) -> Vec<Value> {
    let mut cursor = None;
    let mut items = Vec::new();
    for _ in 0..20 {
        let (status, body) = search(app, Some(token), q, kind, cursor.as_deref()).await;
        assert_eq!(status, StatusCode::OK, "{q} {kind} {body}");
        assert_eq!(body["type"], kind);
        assert!(body.get("nextCursor").is_some(), "{body}");
        assert!(body.get("next_cursor").is_none(), "{body}");
        let page = body["items"].as_array().expect("items").clone();
        if body["nextCursor"].is_null() {
            assert!(page.len() <= 20, "{body}");
            items.extend(page);
            return items;
        }
        assert_eq!(page.len(), 20, "{body}");
        items.extend(page);
        let next = body["nextCursor"].as_str().unwrap().to_string();
        assert_ne!(cursor.as_deref(), Some(next.as_str()));
        cursor = Some(next);
    }
    panic!("cursor did not end");
}

fn db_options(path: &Path) -> SqliteConnectOptions {
    let url = format!("sqlite://{}?mode=rwc", path.join("mewo.db").display());
    SqliteConnectOptions::from_str(&url)
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
}

async fn open_db(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(db_options(path))
        .await
        .unwrap()
}

async fn count(pool: &SqlitePool, sql: &str, bind: Option<&str>) -> i64 {
    let mut query = sqlx::query_scalar::<_, i64>(sql);
    if let Some(bind) = bind {
        query = query.bind(bind);
    }
    query.fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn search_accounts_rank_prefix_before_infix_ignoring_case() {
    let (app, _tmp) = app().await;
    let zed = register(&app, "zed", "password1").await;
    register(&app, "ali", "password1").await;
    let alice = register(&app, "Alice", "password1").await;
    register(&app, "xali", "password1").await;
    register(&app, "malice", "password1").await;
    register(&app, "bob", "password1").await;
    register(&app, "a_b", "password1").await;
    register(&app, "axb", "password1").await;

    let (status, _) = search(&app, None, "ali", "accounts", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for q in ["ali", "ALI", "aLi"] {
        let items = search_all(&app, &zed, q, "accounts").await;
        let names: Vec<&str> = items
            .iter()
            .map(|row| row["username"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["ali", "Alice", "xali", "malice"], "{q}");
        assert!(items.iter().all(|row| row["mine"] == false));
    }

    let items = search_all(&app, &alice, "alice", "accounts").await;
    assert_eq!(items[0]["username"], "Alice");
    assert_eq!(items[0]["mine"], true);
    assert!(items[1..].iter().all(|row| row["mine"] == false));

    let items = search_all(&app, &zed, "a_b", "accounts").await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["username"], "a_b");
    let items = search_all(&app, &zed, "A_B", "accounts").await;
    assert_eq!(items[0]["username"], "a_b");
}

#[tokio::test]
async fn search_hides_private_books_and_drops_them_when_deleted() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, secret) = upload(
        &app,
        &matt,
        "SecretTitle",
        "Nope",
        &common::epub(),
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{secret}");
    let secret_id = secret["id"].as_str().unwrap().to_string();
    let (status, sample) = upload(
        &app,
        &matt,
        "SampleTitle",
        "Mewo",
        &common::epub(),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sample}");
    let sample_id = sample["id"].as_str().unwrap();

    let titles = |items: &[Value]| -> Vec<String> {
        items
            .iter()
            .map(|row| row["title"].as_str().unwrap().to_string())
            .collect()
    };
    let post_books = |items: &[Value]| -> Vec<String> {
        items
            .iter()
            .map(|row| row["book"]["id"].as_str().unwrap().to_string())
            .collect()
    };

    let hidden = search_all(&app, &ted, "SecretTitle", "books").await;
    assert!(hidden.is_empty(), "{hidden:?}");
    let by_owner = search_all(&app, &ted, "matt", "books").await;
    assert_eq!(titles(&by_owner), vec!["SampleTitle".to_string()]);
    let sample_hits = search_all(&app, &ted, "SampleTitle", "books").await;
    assert_eq!(sample_hits.len(), 1);
    assert_eq!(sample_hits[0]["isSample"], true);
    assert_eq!(sample_hits[0]["mine"], false);
    assert_eq!(sample_hits[0]["ownerUsername"], "matt");

    let owner_books = search_all(&app, &matt, "SecretTitle", "books").await;
    assert_eq!(owner_books[0]["id"], secret_id);
    assert_eq!(owner_books[0]["mine"], true);

    let ted_posts = search_all(&app, &ted, "second", "posts").await;
    assert_eq!(post_books(&ted_posts), vec![sample_id.to_string()]);
    assert_eq!(ted_posts[0]["post"]["id"], "1");
    assert!(ted_posts[0]["post"]["text"]
        .as_str()
        .unwrap()
        .contains("second"));
    let matt_posts = search_all(&app, &matt, "second", "posts").await;
    let mut matt_ids = post_books(&matt_posts);
    matt_ids.sort();
    let mut expected = vec![secret_id.clone(), sample_id.to_string()];
    expected.sort();
    assert_eq!(matt_ids, expected);

    share(&app, &matt, &secret_id).await;
    let shared = search_all(&app, &ted, "SecretTitle", "books").await;
    assert_eq!(shared[0]["id"], secret_id);
    assert_eq!(shared[0]["shared"], true);
    let ted_posts = search_all(&app, &ted, "second", "posts").await;
    assert_eq!(ted_posts.len(), 2, "{ted_posts:?}");

    let (status, hidden) = json_req(
        &app,
        "PATCH",
        &format!("/v1/books/{secret_id}"),
        Some(&matt),
        json!({ "shared": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{hidden}");
    assert!(search_all(&app, &ted, "SecretTitle", "books")
        .await
        .is_empty());
    assert_eq!(search_all(&app, &ted, "second", "posts").await.len(), 1);
    assert_eq!(
        search_all(&app, &matt, "SecretTitle", "books").await.len(),
        1
    );

    let (status, _) = empty_req(
        &app,
        "DELETE",
        &format!("/v1/books/{secret_id}"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(search_all(&app, &matt, "SecretTitle", "books")
        .await
        .is_empty());
    assert_eq!(search_all(&app, &matt, "second", "posts").await.len(), 1);

    let pool = open_db(tmp.path()).await;
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM posts WHERE book_id = ?",
            Some(&secret_id)
        )
        .await,
        0
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM posts", None).await, 3);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM post_fts", None).await, 3);
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM post_fts WHERE post_fts MATCH '\"second\"'",
            None,
        )
        .await,
        1
    );
}

#[tokio::test]
async fn search_books_rank_title_prefix_and_escape_like() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let mut uploaded = Vec::new();
    for (title, author, handle) in [
        ("The Cat Book", "Nope", "h1"),
        ("Cat", "Nope", "h2"),
        ("Catalog", "Nope", "h3"),
        ("Dog", "Cat Person", "h4"),
        ("Zebra", "Nope", "uniquehandle"),
        ("100%_done", "Nope", "h5"),
        ("100Xd", "Nope", "h6"),
    ] {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let (status, book) = upload_fields(
            &app,
            &matt,
            &[
                ("title", None, title.as_bytes()),
                ("author", None, author.as_bytes()),
                ("handle", None, handle.as_bytes()),
                ("isSample", None, b"false"),
                ("epub", Some("book.epub"), &common::epub()),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{book}");
        uploaded.push(book);
    }

    let items = search_all(&app, &matt, "cat", "books").await;
    let mut expected: Vec<&Value> = uploaded
        .iter()
        .filter(|book| {
            let title = book["title"].as_str().unwrap();
            let author = book["author"].as_str().unwrap();
            let handle = book["handle"].as_str().unwrap();
            let owner = book["ownerUsername"].as_str().unwrap();
            [title, author, handle, owner]
                .iter()
                .any(|field| field.to_ascii_lowercase().contains("cat"))
        })
        .collect();
    expected.sort_by_key(|book| {
        let title = book["title"].as_str().unwrap().to_ascii_lowercase();
        let prefix = if title.starts_with("cat") { 0 } else { 1 };
        let imported = book["importedAt"].as_i64().unwrap();
        (prefix, -imported, book["id"].as_str().unwrap().to_string())
    });
    let got: Vec<&str> = items
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    let want: Vec<&str> = expected
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(got, want, "{items:?}");
    assert_eq!(items[0]["title"], "Catalog");
    assert_eq!(items[1]["title"], "Cat");

    let again = search_all(&app, &matt, "CAT", "books").await;
    let again_ids: Vec<&str> = again
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(again_ids, got);

    let handle_hits = search_all(&app, &matt, "uniquehandle", "books").await;
    assert_eq!(handle_hits.len(), 1);
    assert_eq!(handle_hits[0]["title"], "Zebra");
    let author_hits = search_all(&app, &matt, "CAT PERSON", "books").await;
    assert_eq!(author_hits.len(), 1);
    assert_eq!(author_hits[0]["title"], "Dog");

    let literal = search_all(&app, &matt, "100%_", "books").await;
    assert_eq!(literal.len(), 1, "{literal:?}");
    assert_eq!(literal[0]["title"], "100%_done");
    assert!(search_all(&app, &matt, "100_d", "books").await.is_empty());
    let plain = search_all(&app, &matt, "100Xd", "books").await;
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0]["title"], "100Xd");
}

#[tokio::test]
async fn search_cursors_list_every_match_once() {
    let (app, _tmp) = app().await;
    let viewer = register(&app, "viewer", "password1").await;
    let mut accounts = Vec::new();
    for i in 0..25 {
        let name = format!("s{i:02}");
        register(&app, &name, "password1").await;
        accounts.push(name);
    }
    let once = search_all(&app, &viewer, "s", "accounts").await;
    let twice = search_all(&app, &viewer, "s", "accounts").await;
    let names = |items: &[Value]| -> Vec<String> {
        items
            .iter()
            .map(|row| row["username"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names(&once), accounts);
    assert_eq!(names(&once), names(&twice));
    assert!(once.iter().all(|row| row["mine"] == false));

    let mut books = Vec::new();
    for i in 0..21 {
        let title = format!("PageBook {i:02}");
        let (status, book) =
            upload(&app, &viewer, &title, "Nope", &common::epub(), None, false).await;
        assert_eq!(status, StatusCode::OK, "{book}");
        books.push(book["id"].as_str().unwrap().to_string());
    }
    let book_items = search_all(&app, &viewer, "PageBook", "books").await;
    let book_again = search_all(&app, &viewer, "PageBook", "books").await;
    let ids = |items: &[Value]| -> Vec<String> {
        items
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_string())
            .collect()
    };
    let mut found = ids(&book_items);
    let mut expect = books.clone();
    found.sort();
    expect.sort();
    assert_eq!(found, expect);
    assert_eq!(ids(&book_items), ids(&book_again));

    let post_key = |items: &[Value]| -> Vec<String> {
        items
            .iter()
            .map(|row| {
                format!(
                    "{}:{}",
                    row["book"]["id"].as_str().unwrap(),
                    row["post"]["id"].as_str().unwrap()
                )
            })
            .collect()
    };
    let posts = search_all(&app, &viewer, "second", "posts").await;
    let posts_again = search_all(&app, &viewer, "second", "posts").await;
    assert_eq!(post_key(&posts), post_key(&posts_again));
    let mut keys = post_key(&posts);
    keys.sort();
    let mut want = Vec::new();
    for id in &books {
        want.push(format!("{id}:1"));
    }
    want.sort();
    assert_eq!(keys, want);
    assert!(posts
        .iter()
        .all(|row| row["post"]["text"].as_str().unwrap().contains("second")));
}

#[tokio::test]
async fn search_keyset_does_not_repeat_a_row_when_a_match_is_inserted() {
    let (app, _tmp) = app().await;
    let viewer = register(&app, "viewer", "password1").await;
    for i in 1..=21 {
        register(&app, &format!("s{i:02}"), "password1").await;
    }
    let (status, first) = search(&app, Some(&viewer), "s", "accounts", None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let names = |body: &Value| -> Vec<String> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["username"].as_str().unwrap().to_string())
            .collect()
    };
    let page1 = names(&first);
    assert_eq!(page1.len(), 20, "{page1:?}");
    assert_eq!(page1[0], "s01");
    assert_eq!(page1[19], "s20");
    let cursor = first["nextCursor"].as_str().expect("cursor");
    // s00 sorts ahead of the first page. An offset cursor would repeat s20.
    register(&app, "s00", "password1").await;
    let (status, second) = search(&app, Some(&viewer), "s", "accounts", Some(cursor)).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let page2 = names(&second);
    assert!(
        page1.iter().all(|name| !page2.contains(name)),
        "page1 {page1:?} page2 {page2:?}"
    );
    assert_eq!(page2, vec!["s21".to_string()]);
}

#[tokio::test]
async fn search_rejects_bad_queries_and_sanitizes_fts() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Book", "Writer", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");

    for kind in ["accounts", "books", "posts"] {
        for q in ["", "   ", "\t"] {
            let (status, body) = search(&app, Some(&token), q, kind, None).await;
            assert_eq!(status, StatusCode::OK, "{kind} {q:?} {body}");
            assert!(body["nextCursor"].is_null(), "{body}");
            assert_eq!(body["type"], kind);
        }
        let (status, body) = empty_req(
            &app,
            "GET",
            &format!("/v1/search?type={kind}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind} {body}");
        assert!(body["nextCursor"].is_null(), "{body}");
        let (status, body) = search(&app, Some(&token), "d", kind, None).await;
        assert_eq!(status, StatusCode::OK, "{kind} one character {body}");
        assert!(body["items"].is_array(), "{kind} {body}");
    }
    // A cursor belongs to a typed query. Browse ignores it, including a junk one.
    let (status, body) = search(&app, Some(&token), "", "books", Some("%%%")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    let long = "a".repeat(101);
    let (status, body) = search(&app, Some(&token), &long, "accounts", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(error.contains("100"), "{error}");
    assert!(error.contains("asked 101"), "{error}");
    let (status, body) = search(&app, Some(&token), &"a".repeat(100), "accounts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["items"].as_array().unwrap().is_empty());

    for q in [
        "\"",
        "*",
        "NEAR",
        "-",
        "(",
        ")",
        "\" * NEAR - ( )",
        "first OR second",
        "para*",
    ] {
        let (status, body) = search(&app, Some(&token), q, "posts", None).await;
        assert_eq!(status, StatusCode::OK, "{q} {body}");
        assert!(body["items"].is_array(), "{q} {body}");
    }

    let hits = search_all(&app, &token, "first para", "posts").await;
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0]["post"]["text"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("first"));
    assert!(hits[0]["post"]["text"]
        .as_str()
        .unwrap()
        .to_ascii_lowercase()
        .contains("para"));

    let (status, body) = search(&app, Some(&token), "para", "posts", Some("%%%")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("page token"));
}

async fn user_id(pool: &SqlitePool, username: &str) -> String {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn insert_user(pool: &SqlitePool, id: &str, username: &str, created_at: i64) {
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES (?, ?, ?, 'x', ?)",
    )
    .bind(id)
    .bind(username)
    .bind(username.to_ascii_lowercase())
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_book(
    pool: &SqlitePool,
    id: &str,
    user_id: &str,
    title: &str,
    imported_at: i64,
    shared: i64,
    sample: i64,
) {
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at, post_count, is_sample, shared)
         VALUES (?, ?, ?, 'Author', 'author', ?, 1, ?, ?)",
    )
    .bind(id)
    .bind(user_id)
    .bind(title)
    .bind(imported_at)
    .bind(sample)
    .bind(shared)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_post(pool: &SqlitePool, book_id: &str, idx: i64) {
    sqlx::query(
        "INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES (?, ?, 'Body', '', ?)",
    )
    .bind(book_id)
    .bind(idx)
    .bind(format!("post {idx}"))
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_comment(
    pool: &SqlitePool,
    id: &str,
    user_id: &str,
    book_id: &str,
    post_id: &str,
    at: i64,
) {
    sqlx::query(
        "INSERT INTO comments (id, user_id, book_id, post_id, text, created_at)
         VALUES (?, ?, ?, ?, 'hi', ?)",
    )
    .bind(id)
    .bind(user_id)
    .bind(book_id)
    .bind(post_id)
    .bind(at)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_repost(pool: &SqlitePool, user_id: &str, book_id: &str, post_id: &str, at: i64) {
    sqlx::query("INSERT INTO reposts (user_id, book_id, post_id, created_at) VALUES (?, ?, ?, ?)")
        .bind(user_id)
        .bind(book_id)
        .bind(post_id)
        .bind(at)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_progress(pool: &SqlitePool, user_id: &str, book_id: &str, index: i64) {
    sqlx::query("INSERT INTO book_progress (user_id, book_id, progress_index) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(book_id)
        .bind(index)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_imported(
    pool: &SqlitePool,
    id: &str,
    user_id: &str,
    work_key: &str,
    book_id: &str,
    at: i64,
) {
    sqlx::query(
        "INSERT INTO book_requests (
            id, user_id, work_key, title, author, state, progress, tried_guids, attempts,
            book_id, next_check_at, created_at, updated_at
         ) VALUES (?, ?, ?, 'T', 'A', 'imported', 100, '[]', 0, ?, 0, ?, ?)",
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

fn book_titles(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["title"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn browse_books_rank_readers_then_talk_then_reposts() {
    let (app, tmp) = app().await;
    let alice_token = register(&app, "alice", "password1").await;
    let bob_token = register(&app, "bob", "password1").await;
    let pool = open_db(tmp.path()).await;
    let alice = user_id(&pool, "alice").await;
    let bob = user_id(&pool, "bob").await;
    insert_user(&pool, "cara", "cara", 20).await;
    insert_user(&pool, "ada", "ada", 10).await;

    insert_book(&pool, "read", &alice, "Read", 10, 1, 0).await;
    insert_book(&pool, "talk", &alice, "Talk", 10, 1, 0).await;
    insert_book(&pool, "share", &alice, "Share", 10, 1, 0).await;
    insert_book(&pool, "held", &alice, "Held", 10, 0, 0).await;
    insert_book(&pool, "quiet", &alice, "Quiet", 50, 0, 0).await;
    insert_book(&pool, "older", &alice, "Older", 40, 0, 0).await;
    insert_book(&pool, "sample", &alice, "Sample", 9_000, 0, 1).await;
    insert_book(&pool, "secret", &bob, "Secret", 9_000, 0, 0).await;

    let now = mewo_server::db::now_ms();
    insert_progress(&pool, &alice, "read", 3).await;
    insert_progress(&pool, &bob, "read", 3).await;
    insert_comment(&pool, "t1", &alice, "talk", "0", now).await;
    insert_comment(&pool, "t2", &bob, "talk", "0", now).await;
    for user in [alice.as_str(), bob.as_str(), "cara"] {
        insert_repost(&pool, user, "share", "0", now).await;
    }
    insert_comment(&pool, "h-alice", &alice, "held", "0", now).await;
    for n in 0..4 {
        insert_comment(&pool, &format!("h-bob-{n}"), &bob, "held", "0", now).await;
    }
    for user in [alice.as_str(), bob.as_str(), "cara", "ada"] {
        insert_progress(&pool, user, "sample", 5).await;
    }
    for n in 0..8 {
        insert_comment(&pool, &format!("sec-{n}"), &bob, "secret", "0", now).await;
    }
    insert_imported(&pool, "hold-bob", &bob, "/works/OL1W", "held", now).await;

    let (status, body) = search(&app, Some(&alice_token), "", "books", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    assert_eq!(
        book_titles(&body),
        vec!["Read", "Talk", "Share", "Held", "Quiet", "Older", "Sample"]
    );
    let held = &body["items"][3];
    assert_eq!(held["mine"], true);
    assert_eq!(held["held"], false);
    assert_eq!(held["ownerUsername"], "alice");
    assert_eq!(body["items"][6]["isSample"], true);

    let (status, body) = search(&app, Some(&bob_token), "   ", "books", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let titles = book_titles(&body);
    let held_at = titles.iter().position(|title| title == "Held").unwrap();
    let read_at = titles.iter().position(|title| title == "Read").unwrap();
    assert!(held_at < read_at, "{titles:?}");
    assert!(titles.contains(&"Secret".to_string()), "{titles:?}");
    assert!(!titles.contains(&"Quiet".to_string()), "{titles:?}");
    let held = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["title"] == "Held")
        .unwrap();
    assert_eq!(held["mine"], false);
    assert_eq!(held["held"], true);
    assert_eq!(held["ownerUsername"], "");
    assert_eq!(held["shared"], false);
}

#[tokio::test]
async fn browse_accounts_ignore_private_activity_and_skip_self() {
    let (app, tmp) = app().await;
    let alice_token = register(&app, "alice", "password1").await;
    let pool = open_db(tmp.path()).await;
    let alice = user_id(&pool, "alice").await;
    let (status, body) = search(&app, Some(&alice_token), "", "accounts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    assert_eq!(body["items"].as_array().unwrap().len(), 0);

    insert_user(&pool, "bob", "bob", 400).await;
    insert_user(&pool, "cara", "cara", 200).await;
    insert_user(&pool, "ada", "ada", 300).await;
    insert_book(&pool, "plaza", &alice, "Plaza", 1, 1, 0).await;
    insert_book(&pool, "diary", &alice, "Diary", 1, 0, 0).await;
    let now = mewo_server::db::now_ms();
    insert_comment(&pool, "c-cara", "cara", "plaza", "0", now).await;
    insert_comment(&pool, "c-ada", "ada", "plaza", "0", now).await;
    for n in 0..5 {
        insert_comment(&pool, &format!("c-bob-{n}"), "bob", "diary", "0", now).await;
    }

    let (status, body) = search(&app, Some(&alice_token), "", "accounts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    let names: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["ada", "cara", "bob"]);
    assert!(body["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["mine"] == false));
}

#[tokio::test]
async fn browse_posts_use_the_window_then_the_newest() {
    let (app, tmp) = app().await;
    let alice_token = register(&app, "alice", "password1").await;
    let bob_token = register(&app, "bob", "password1").await;
    let pool = open_db(tmp.path()).await;
    let alice = user_id(&pool, "alice").await;
    let bob = user_id(&pool, "bob").await;
    insert_user(&pool, "cara", "cara", 1).await;
    let now = mewo_server::db::now_ms();
    let day = 24 * 60 * 60 * 1000;

    insert_book(&pool, "hot", &alice, "Hot", 1, 1, 0).await;
    insert_book(&pool, "busy", &alice, "Busy", 1, 1, 0).await;
    insert_book(&pool, "newer", &alice, "Newer", 1, 1, 0).await;
    insert_book(&pool, "older", &alice, "Older", 1, 1, 0).await;
    insert_book(&pool, "secret", &bob, "Secret", 1, 0, 0).await;
    insert_book(&pool, "diary", &alice, "Diary", 1, 0, 0).await;
    for id in ["hot", "busy", "newer", "older", "secret"] {
        insert_post(&pool, id, 0).await;
    }
    insert_post(&pool, "diary", 0).await;
    insert_post(&pool, "diary", 1).await;

    insert_comment(&pool, "hot-c", &alice, "hot", "0", now - day).await;
    for n in 0..3 {
        insert_comment(
            &pool,
            &format!("busy-{n}"),
            &alice,
            "busy",
            "0",
            now - 40 * day,
        )
        .await;
    }
    insert_comment(&pool, "newer-c", &alice, "newer", "0", now - 31 * day).await;
    for n in 0..4 {
        insert_comment(
            &pool,
            &format!("older-{n}"),
            &alice,
            "older",
            "0",
            now - 60 * day,
        )
        .await;
    }
    insert_comment(&pool, "sec-c", &bob, "secret", "0", now).await;
    insert_comment(&pool, "diary-a", &alice, "diary", "0", now).await;
    insert_comment(&pool, "diary-b", &bob, "diary", "1", now).await;
    insert_imported(&pool, "hold", &bob, "/works/OL1W", "diary", now).await;

    let posts = |body: &Value| -> Vec<(String, String)> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["book"]["title"].as_str().unwrap().to_string(),
                    row["post"]["id"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };

    let (status, body) = search(&app, Some(&alice_token), "", "posts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    let alice_posts = posts(&body);
    let hot = alice_posts.iter().position(|row| row.0 == "Hot").unwrap();
    let busy = alice_posts.iter().position(|row| row.0 == "Busy").unwrap();
    assert!(hot < busy, "{alice_posts:?}");
    assert!(alice_posts.contains(&("Diary".into(), "0".into())));
    assert!(!alice_posts.iter().any(|row| row.0 == "Secret"));
    assert!(!alice_posts.iter().any(|row| row.1 == "1"));

    let (status, body) = search(&app, Some(&bob_token), "", "posts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let bob_posts = posts(&body);
    assert!(bob_posts.contains(&("Diary".into(), "1".into())));
    assert!(!bob_posts.contains(&("Diary".into(), "0".into())));
    assert!(bob_posts.iter().any(|row| row.0 == "Secret"));

    sqlx::query("DELETE FROM comments WHERE created_at >= ?")
        .bind(now - 30 * day)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = search(&app, Some(&alice_token), "", "posts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let quiet = posts(&body);
    assert_eq!(quiet.first().map(|row| row.0.as_str()), Some("Newer"));
    let newer = quiet.iter().position(|row| row.0 == "Newer").unwrap();
    let older = quiet.iter().position(|row| row.0 == "Older").unwrap();
    assert!(newer < older, "{quiet:?}");
    assert!(!quiet.iter().any(|row| row.0 == "Hot"));
}

#[tokio::test]
async fn browse_pages_are_a_single_head() {
    let (app, tmp) = app().await;
    let token = register(&app, "alice", "password1").await;
    let pool = open_db(tmp.path()).await;
    let alice = user_id(&pool, "alice").await;
    for n in 1..=21 {
        insert_user(&pool, &format!("u{n:02}"), &format!("u{n:02}"), n).await;
    }
    for n in 1..=31 {
        insert_book(
            &pool,
            &format!("b{n:02}"),
            &alice,
            &format!("B{n:02}"),
            n,
            0,
            0,
        )
        .await;
    }
    let (status, body) = search(&app, Some(&token), "", "accounts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    let names: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["username"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 20);
    assert_eq!(names[0], "u21");
    assert_eq!(names[19], "u02");
    assert!(!names.contains(&"u01"));

    let (status, body) = search(&app, Some(&token), "", "books", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["nextCursor"].is_null(), "{body}");
    let titles = book_titles(&body);
    assert_eq!(titles.len(), 30);
    assert_eq!(titles[0], "B31");
    assert_eq!(titles[29], "B02");
    assert!(!titles.iter().any(|title| title == "B01"));
}

#[tokio::test]
async fn profile_hides_private_replies_and_pages_newest_first() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let (status, shared) = upload(&app, &matt, "Shared", "A", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{shared}");
    let shared_id = shared["id"].as_str().unwrap();
    share(&app, &matt, shared_id).await;
    let (status, private) = upload(&app, &matt, "Private", "A", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{private}");
    let private_id = private["id"].as_str().unwrap();

    for i in 0..31 {
        tokio::time::sleep(Duration::from_millis(5)).await;
        let (status, body) = json_req(
            &app,
            "POST",
            &format!("/v1/books/{shared_id}/comments/0"),
            Some(&matt),
            json!({ "text": format!("c{i}") }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{private_id}/comments/1"),
        Some(&matt),
        json!({ "text": "secret-reply" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, missing) = empty_req(&app, "GET", "/v1/users/nobody", Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, missing_posts) =
        empty_req(&app, "GET", "/v1/users/nobody/posts", Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing_posts}");
    let (status, old_missing) =
        empty_req(&app, "GET", "/v1/profiles/nobody/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{old_missing}");
    assert_eq!(old_missing["username"], "nobody");
    assert_eq!(old_missing["replies"], json!([]));

    let (status, ted_view) = empty_req(&app, "GET", "/v1/users/matt", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_view}");
    assert_eq!(ted_view["username"], "matt");
    assert_eq!(ted_view["postCount"], 31);
    assert_eq!(ted_view["mine"], false);
    assert!(ted_view["joinedAt"].as_i64().unwrap() > 0);
    assert!(ted_view.get("joined_at").is_none());
    let (status, mine) = empty_req(&app, "GET", "/v1/users/Matt", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["postCount"], 32);
    assert_eq!(mine["mine"], true);

    let load = |token: String, cursor: Option<String>| {
        let app = app.clone();
        async move {
            let path = match cursor {
                Some(cursor) => format!("/v1/users/matt/posts?cursor={}", percent_encode(&cursor)),
                None => "/v1/users/matt/posts".to_string(),
            };
            empty_req(&app, "GET", &path, Some(&token)).await
        }
    };
    let mut ted_items = Vec::new();
    let mut cursor = None;
    for _ in 0..5 {
        let (status, page) = load(ted.clone(), cursor.clone()).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.get("nextCursor").is_some());
        let items = page["items"].as_array().unwrap();
        if page["nextCursor"].is_null() {
            ted_items.extend(items.clone());
            break;
        }
        assert_eq!(items.len(), 30, "{page}");
        ted_items.extend(items.clone());
        cursor = Some(page["nextCursor"].as_str().unwrap().to_string());
    }
    assert_eq!(ted_items.len(), 31);
    let texts: Vec<&str> = ted_items
        .iter()
        .map(|row| row["comment"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts[0], "c30");
    assert_eq!(texts[30], "c0");
    assert!(texts.iter().all(|text| *text != "secret-reply"));
    for pair in ted_items.windows(2) {
        assert!(
            pair[0]["comment"]["createdAt"].as_i64().unwrap()
                >= pair[1]["comment"]["createdAt"].as_i64().unwrap()
        );
    }
    assert!(ted_items.iter().all(|row| {
        row["book"]["id"] == shared_id
            && row["post"]["id"] == "0"
            && row["post"]["kind"] == "Heading"
            && row["post"]["text"] == "Chapter I"
            && row["comment"]["mine"] == false
            && row["comment"]["username"] == "matt"
            && row["comment"].get("created_at").is_none()
    }));
    let mut ids: Vec<&str> = ted_items
        .iter()
        .map(|row| row["comment"]["id"].as_str().unwrap())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 31);

    let (status, owner_page) = empty_req(&app, "GET", "/v1/users/matt/posts", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{owner_page}");
    let owner_texts: Vec<&str> = owner_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["comment"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(owner_texts[0], "secret-reply");
    assert_eq!(owner_page["items"][0]["book"]["id"], private_id);
    assert_eq!(owner_page["items"][0]["post"]["id"], "1");
    assert!(owner_page["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["comment"]["mine"] == true));

    let (status, old) = empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{old}");
    assert_eq!(old["username"], "matt");
    let old_replies = old["replies"].as_array().unwrap();
    assert_eq!(old_replies.len(), 31);
    assert!(old_replies.iter().all(|row| {
        row["bookId"] == shared_id
            && row["comment"]["text"] != "secret-reply"
            && row.get("post").is_none()
    }));

    let (status, bad) = empty_req(&app, "GET", "/v1/users/matt/posts?cursor=%%%", Some(&ted)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
}

#[tokio::test]
async fn migration_adds_search_on_an_existing_database() {
    let tmp = TempDir::new().unwrap();
    let pool = open_db(tmp.path()).await;
    let migrator = sqlx::migrate!("./migrations");
    let mut conn = pool.acquire().await.unwrap();
    Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
    for migration in migrator.iter() {
        if migration.version < 5 {
            Migrate::apply(&mut *conn, migration).await.unwrap();
        }
    }
    drop(conn);

    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('user', 'matt', 'matt', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at)
         VALUES ('book', 'user', 'Old', 'Author', 'author', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT posts_indexed FROM books WHERE id = 'book'")
            .fetch_one(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'posts'",
            None
        )
        .await,
        0
    );

    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    assert_eq!(
        count(
            &pool,
            "SELECT posts_indexed FROM books WHERE id = 'book'",
            None
        )
        .await,
        0
    );
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at)
         VALUES ('new', 'user', 'New', 'Author', 'author', 2)",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT posts_indexed FROM books WHERE id = 'new'",
            None
        )
        .await,
        0
    );
    let triggers: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'posts' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        triggers,
        vec!["posts_ad".to_string(), "posts_ai".to_string()]
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'post_fts'",
            None
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'reposts'",
            None
        )
        .await,
        1
    );
    let repost_indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master
         WHERE type = 'index' AND tbl_name = 'reposts' AND sql IS NOT NULL
         ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        repost_indexes,
        vec!["reposts_created".to_string(), "reposts_user".to_string()]
    );

    sqlx::query(
        "INSERT INTO posts (book_id, idx, kind, chapter, text)
         VALUES ('book', 0, 'Body', 'Ch', 'secretword')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM post_fts WHERE post_fts MATCH 'secretword'",
            None,
        )
        .await,
        1
    );
    sqlx::query(
        "INSERT INTO reposts (user_id, book_id, post_id, created_at)
         VALUES ('user', 'book', '0', 1), ('user', 'new', '0', 2)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM books WHERE id = 'book'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM posts WHERE book_id = 'book'",
            None
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM reposts WHERE book_id = 'book'",
            None
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM reposts WHERE book_id = 'new'",
            None
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM post_fts WHERE post_fts MATCH 'secretword'",
            None,
        )
        .await,
        0
    );
}

#[tokio::test]
async fn backfill_indexes_unindexed_books_once() {
    let tmp = TempDir::new().unwrap();
    let pool = open_db(tmp.path()).await;
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('owner', 'owner', 'owner', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sessions (token, user_id, created_at) VALUES ('backfill-token', 'owner', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES ('cached-book', 'owner', 'Cached', 'Author', 'author', 1, 1, 0, 0, 0, 0),
                ('legacy-book', 'owner', 'Legacy', 'Author', 'author', 2, 0, 0, 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let cached = tmp.path().join("books").join("owner").join("cached-book");
    let legacy = tmp.path().join("books").join("owner").join("legacy-book");
    tokio::fs::create_dir_all(&cached).await.unwrap();
    tokio::fs::create_dir_all(&legacy).await.unwrap();
    let epub = common::epub();
    tokio::fs::write(cached.join("book.epub"), &epub)
        .await
        .unwrap();
    tokio::fs::write(legacy.join("book.epub"), &epub)
        .await
        .unwrap();
    tokio::fs::write(
        cached.join("posts.json"),
        r#"{"posts":[{"id":"0","kind":"Body","text":"cacheonlyword","chapter":"Cache"}]}"#,
    )
    .await
    .unwrap();

    let config = Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits: test_limits(),
    };
    let app = build(config.clone()).await.unwrap();
    let pool = open_db(tmp.path()).await;
    let start = std::time::Instant::now();
    loop {
        let pending = count(
            &pool,
            "SELECT COUNT(*) FROM books WHERE posts_indexed = 0",
            None,
        )
        .await;
        if pending == 0 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "backfill did not finish"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    let token = "backfill-token";
    let cached_hits = search_all(&app, token, "cacheonlyword", "posts").await;
    assert_eq!(cached_hits.len(), 1, "{cached_hits:?}");
    assert_eq!(cached_hits[0]["book"]["id"], "cached-book");
    assert_eq!(cached_hits[0]["post"]["text"], "cacheonlyword");
    let parsed = search_all(&app, token, "paragraph", "posts").await;
    assert_eq!(parsed.len(), 2, "{parsed:?}");
    assert!(parsed.iter().all(|row| row["book"]["id"] == "legacy-book"));
    let legacy_json: Value =
        serde_json::from_slice(&tokio::fs::read(legacy.join("posts.json")).await.unwrap()).unwrap();
    assert!(legacy_json["posts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|post| post["text"].as_str().unwrap().contains("Last paragraph")));
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM posts WHERE book_id = 'cached-book'",
            None
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM posts WHERE book_id = 'legacy-book'",
            None
        )
        .await,
        3
    );

    let signature: Vec<String> = sqlx::query_scalar(
        "SELECT rowid || ':' || book_id || ':' || idx || ':' || text
         FROM posts ORDER BY book_id, idx",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let cached_mtime = tokio::fs::metadata(cached.join("posts.json"))
        .await
        .unwrap()
        .modified()
        .unwrap();
    let legacy_mtime = tokio::fs::metadata(legacy.join("posts.json"))
        .await
        .unwrap()
        .modified()
        .unwrap();

    let third = tmp.path().join("books").join("owner").join("third-book");
    tokio::fs::create_dir_all(&third).await.unwrap();
    tokio::fs::write(third.join("book.epub"), &epub)
        .await
        .unwrap();
    tokio::fs::write(
        third.join("posts.json"),
        r#"{"posts":[{"id":"0","kind":"Body","text":"thirdword","chapter":"Later"}]}"#,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES ('third-book', 'owner', 'Third', 'Author', 'author', 3, 1, 0, 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let app = build(config).await.unwrap();
    let start = std::time::Instant::now();
    loop {
        let indexed = count(
            &pool,
            "SELECT posts_indexed FROM books WHERE id = 'third-book'",
            None,
        )
        .await;
        if indexed == 1 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "second backfill did not index the new book"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let third_hits = search_all(&app, token, "thirdword", "posts").await;
    assert_eq!(third_hits.len(), 1);
    assert_eq!(third_hits[0]["book"]["id"], "third-book");

    let again: Vec<String> = sqlx::query_scalar(
        "SELECT rowid || ':' || book_id || ':' || idx || ':' || text
         FROM posts WHERE book_id != 'third-book' ORDER BY book_id, idx",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(again, signature);
    assert_eq!(
        tokio::fs::metadata(cached.join("posts.json"))
            .await
            .unwrap()
            .modified()
            .unwrap(),
        cached_mtime
    );
    assert_eq!(
        tokio::fs::metadata(legacy.join("posts.json"))
            .await
            .unwrap()
            .modified()
            .unwrap(),
        legacy_mtime
    );
}

fn repost_path(id: &str, post: &str) -> String {
    format!("/v1/books/{id}/posts/{}/repost", percent_encode(post))
}

async fn unshare(app: &axum::Router, token: &str, id: &str) {
    let (status, body) = json_req(
        app,
        "PATCH",
        &format!("/v1/books/{id}"),
        Some(token),
        json!({ "shared": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["shared"], false);
}

async fn pause() {
    tokio::time::sleep(Duration::from_millis(8)).await;
}

async fn feed_all(app: &axum::Router, token: &str) -> Vec<Value> {
    let mut cursor: Option<String> = None;
    let mut items = Vec::new();
    for _ in 0..20 {
        let path = match &cursor {
            Some(cursor) => format!("/v1/feed?cursor={}", percent_encode(cursor)),
            None => "/v1/feed".to_string(),
        };
        let (status, page) = empty_req(app, "GET", &path, Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert!(page.get("nextCursor").is_some(), "{page}");
        assert!(page.get("next_cursor").is_none(), "{page}");
        let batch = page["items"].as_array().expect("items");
        if page["nextCursor"].is_null() {
            assert!(batch.len() <= 20, "{page}");
            items.extend(batch.clone());
            return items;
        }
        assert_eq!(batch.len(), 20, "{page}");
        items.extend(batch.clone());
        let next = page["nextCursor"].as_str().unwrap().to_string();
        assert_ne!(cursor.as_deref(), Some(next.as_str()));
        cursor = Some(next);
    }
    panic!("feed cursor did not end");
}

fn feed_identity(item: &Value) -> String {
    let kind = item["type"].as_str().unwrap();
    let actor = item["actor"]["username"].as_str().unwrap();
    let book = item["book"]["id"].as_str().unwrap();
    let post = item["post"]["id"].as_str().unwrap();
    match kind {
        "comment" => format!(
            "comment:{actor}:{book}:{post}:{}",
            item["comment"]["text"].as_str().unwrap()
        ),
        "repost" => format!("repost:{actor}:{book}:{post}"),
        other => panic!("unexpected feed type {other}"),
    }
}

fn assert_feed_order(items: &[Value]) {
    for pair in items.windows(2) {
        let a_at = pair[0]["at"].as_i64().unwrap();
        let b_at = pair[1]["at"].as_i64().unwrap();
        assert!(a_at >= b_at, "{pair:?}");
        if a_at != b_at {
            continue;
        }
        let rank = |item: &Value| if item["type"] == "repost" { 1 } else { 0 };
        assert!(rank(&pair[0]) >= rank(&pair[1]), "{pair:?}");
        if pair[0]["type"] == "comment" && pair[1]["type"] == "comment" {
            assert!(
                pair[0]["comment"]["id"].as_str().unwrap()
                    >= pair[1]["comment"]["id"].as_str().unwrap()
            );
        }
    }
}

#[tokio::test]
async fn repost_put_and_delete_are_idempotent() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Shared", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    share(&app, &matt, id).await;
    let path = repost_path(id, "0");

    let (status, _) = empty_req(&app, "PUT", &path, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = empty_req(&app, "PUT", &path, Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reposts"], 1);
    assert_eq!(body["reposted"], true);
    let (status, again) = empty_req(&app, "PUT", &path, Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, body);

    let (status, index) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(index["reposts"]["0"], 1);
    assert_eq!(index["reposted"], json!(["0"]));
    assert!(index["counts"].as_object().unwrap().is_empty());
    assert_eq!(index["mine"], json!([]));

    let (status, ted_index) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/comments"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_index}");
    assert_eq!(ted_index["reposts"]["0"], 1);
    assert_eq!(ted_index["reposted"], json!([]));

    let (status, ted_put) = empty_req(&app, "PUT", &path, Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_put}");
    assert_eq!(ted_put["reposts"], 2);
    assert_eq!(ted_put["reposted"], true);
    let (status, matt_index) = empty_req(
        &app,
        "GET",
        &format!("/v1/books/{id}/comments"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{matt_index}");
    assert_eq!(matt_index["reposts"]["0"], 2);
    assert_eq!(matt_index["reposted"], json!(["0"]));

    let (status, other) = empty_req(&app, "PUT", &repost_path(id, "1"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{other}");
    assert_eq!(other["reposts"], 1);
    let (status, gone) = empty_req(&app, "DELETE", &repost_path(id, "1"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{gone}");
    assert_eq!(gone["reposts"], 0);
    assert_eq!(gone["reposted"], false);

    let (status, deleted) = empty_req(&app, "DELETE", &path, Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{deleted}");
    assert_eq!(deleted["reposts"], 1);
    assert_eq!(deleted["reposted"], false);
    let (status, deleted_again) = empty_req(&app, "DELETE", &path, Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{deleted_again}");
    assert_eq!(deleted_again, deleted);

    let (status, clear) = empty_req(&app, "DELETE", &path, Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{clear}");
    assert_eq!(clear["reposts"], 0);
    assert_eq!(clear["reposted"], false);
    let (status, empty) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/comments"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["reposts"], json!({}));
    assert_eq!(empty["reposted"], json!([]));
}

#[tokio::test]
async fn repost_on_another_users_private_book_is_404() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Private", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    let path = repost_path(&id, "0");

    let (status, body) = empty_req(&app, "PUT", &path, Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = empty_req(&app, "DELETE", &path, Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, mine) = empty_req(&app, "PUT", &path, Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["reposts"], 1);
    assert_eq!(mine["reposted"], true);
    let (status, body) = empty_req(&app, "DELETE", &path, Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let pool = open_db(tmp.path()).await;
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM reposts WHERE book_id = ?",
            Some(&id)
        )
        .await,
        1
    );
}

#[tokio::test]
async fn private_reposts_leave_other_feeds_and_counts() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, private_book) = upload(&app, &matt, "Private", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{private_book}");
    let private_id = private_book["id"].as_str().unwrap().to_string();
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{private_id}/comments/0"),
        Some(&matt),
        json!({ "text": "own-private" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    pause().await;
    let (status, body) = empty_req(&app, "PUT", &repost_path(&private_id, "1"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, shared_book) = upload(&app, &matt, "Shared", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{shared_book}");
    let shared_id = shared_book["id"].as_str().unwrap().to_string();
    share(&app, &matt, &shared_id).await;
    pause().await;
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{shared_id}/comments/0"),
        Some(&matt),
        json!({ "text": "matt-shared" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    pause().await;
    let (status, body) = empty_req(&app, "PUT", &repost_path(&shared_id, "0"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    pause().await;
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{shared_id}/comments/1"),
        Some(&ted),
        json!({ "text": "ted-shared" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    pause().await;
    let (status, body) = empty_req(&app, "PUT", &repost_path(&shared_id, "1"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let ted_feed = feed_all(&app, &ted).await;
    assert!(
        ted_feed.iter().all(|row| row["book"]["id"] == shared_id),
        "{ted_feed:?}"
    );
    assert_eq!(ted_feed.len(), 4, "{ted_feed:?}");

    let matt_feed = feed_all(&app, &matt).await;
    assert_eq!(matt_feed.len(), 6, "{matt_feed:?}");
    assert!(matt_feed
        .iter()
        .any(|row| { row["book"]["id"] == private_id && row["actor"]["username"] == "matt" }));
    assert!(matt_feed
        .iter()
        .any(|row| { row["book"]["id"] == shared_id && row["actor"]["username"] == "ted" }));
    assert!(matt_feed
        .iter()
        .any(|row| row["type"] == "comment" && row["comment"]["mine"] == true));

    let (status, ted_profile) = empty_req(&app, "GET", "/v1/users/ted", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_profile}");
    assert_eq!(ted_profile["repostCount"], 1);
    let (status, matt_as_ted) = empty_req(&app, "GET", "/v1/users/matt", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{matt_as_ted}");
    assert_eq!(matt_as_ted["repostCount"], 1);
    let (status, matt_as_matt) = empty_req(&app, "GET", "/v1/users/matt", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{matt_as_matt}");
    assert_eq!(matt_as_matt["repostCount"], 2);

    unshare(&app, &matt, &shared_id).await;

    let ted_after = feed_all(&app, &ted).await;
    assert!(ted_after.is_empty(), "{ted_after:?}");
    // Unsharing hides everyone else's rows again, including from the owner.
    let matt_after = feed_all(&app, &matt).await;
    assert_eq!(matt_after.len(), 4, "{matt_after:?}");
    assert!(matt_after
        .iter()
        .all(|row| row["actor"]["username"] == "matt"));
    assert!(matt_after
        .iter()
        .any(|row| { row["book"]["id"] == shared_id && row["type"] == "repost" }));
    assert!(matt_after.iter().any(|row| row["book"]["id"] == private_id));

    let (status, ted_reposts) = empty_req(&app, "GET", "/v1/users/ted/reposts", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_reposts}");
    assert_eq!(ted_reposts["items"], json!([]));
    assert!(ted_reposts["nextCursor"].is_null());
    let (status, ted_self) = empty_req(&app, "GET", "/v1/users/ted", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{ted_self}");
    assert_eq!(ted_self["repostCount"], 0);
    let (status, matt_hidden) = empty_req(&app, "GET", "/v1/users/matt", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{matt_hidden}");
    assert_eq!(matt_hidden["repostCount"], 0);
    let (status, matt_reposts_hidden) =
        empty_req(&app, "GET", "/v1/users/matt/reposts", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{matt_reposts_hidden}");
    assert_eq!(matt_reposts_hidden["items"], json!([]));

    let (status, owner_count) = empty_req(&app, "GET", "/v1/users/matt", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{owner_count}");
    assert_eq!(owner_count["repostCount"], 2);
    let (status, owner_reposts) =
        empty_req(&app, "GET", "/v1/users/matt/reposts", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{owner_reposts}");
    let rows = owner_reposts["items"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0]["repostedAt"].as_i64().unwrap() >= rows[1]["repostedAt"].as_i64().unwrap());
    assert!(rows[0].get("reposted_at").is_none());
    let mut pairs: Vec<(String, String)> = rows
        .iter()
        .map(|row| {
            (
                row["book"]["id"].as_str().unwrap().to_string(),
                row["post"]["id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    pairs.sort();
    let mut expected = vec![(private_id, "1".to_string()), (shared_id, "0".to_string())];
    expected.sort();
    assert_eq!(pairs, expected);
}

enum Made {
    Comment {
        text: String,
    },
    Repost {
        actor: &'static str,
        post: &'static str,
    },
}

#[tokio::test]
async fn feed_orders_and_pages_a_comment_repost_mix() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Feed", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    share(&app, &matt, &id).await;
    let (status, quiet) = upload(&app, &matt, "Quiet", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{quiet}");
    let quiet_id = quiet["id"].as_str().unwrap();
    share(&app, &matt, quiet_id).await;

    let (status, _) = empty_req(&app, "GET", "/v1/feed", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let mut made = Vec::new();
    let mut reposts = vec![
        ("ted", "0"),
        ("ted", "1"),
        ("ted", "2"),
        ("matt", "0"),
        ("matt", "1"),
        ("matt", "2"),
    ];
    let mut comments = 0;
    while made.len() < 22 {
        let text = format!("c{comments}");
        let (status, body) = json_req(
            &app,
            "POST",
            &format!("/v1/books/{id}/comments/0"),
            Some(&matt),
            json!({ "text": text }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        made.push(Made::Comment { text });
        comments += 1;
        if made.len() < 22 && !reposts.is_empty() {
            let (actor, post) = reposts.remove(0);
            let token = if actor == "matt" { &matt } else { &ted };
            let (status, body) = empty_req(&app, "PUT", &repost_path(&id, post), Some(token)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            made.push(Made::Repost { actor, post });
        }
    }

    let pool = open_db(tmp.path()).await;
    let matt_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username_norm = 'matt'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let ted_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username_norm = 'ted'")
        .fetch_one(&pool)
        .await
        .unwrap();
    for (index, event) in made.iter().enumerate() {
        let at = index as i64 + 1;
        match event {
            Made::Comment { text } => {
                sqlx::query("UPDATE comments SET created_at = ? WHERE book_id = ? AND text = ?")
                    .bind(at)
                    .bind(&id)
                    .bind(text)
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            Made::Repost { actor, post } => {
                let user = if *actor == "ted" { &ted_id } else { &matt_id };
                sqlx::query(
                    "UPDATE reposts SET created_at = ? WHERE user_id = ? AND book_id = ? AND post_id = ?",
                )
                .bind(at)
                .bind(user)
                .bind(&id)
                .bind(post)
                .execute(&pool)
                .await
                .unwrap();
            }
        }
    }
    let mut expected = Vec::new();
    for event in made.iter().rev() {
        expected.push(match event {
            Made::Comment { text } => format!("comment:matt:{id}:0:{text}"),
            Made::Repost { actor, post } => format!("repost:{actor}:{id}:{post}"),
        });
    }

    let first = feed_all(&app, &ted).await;
    let second = feed_all(&app, &matt).await;
    let third = feed_all(&app, &ted).await;
    let keys = |items: &[Value]| items.iter().map(feed_identity).collect::<Vec<_>>();
    assert_eq!(keys(&first), expected, "{first:?}");
    assert_eq!(first, third);
    assert_eq!(keys(&second), keys(&first));
    assert_feed_order(&first);
    assert!(first.iter().any(|row| row["type"] == "comment"));
    assert!(first.iter().any(|row| row["type"] == "repost"));
    assert!(first.iter().all(|row| row["book"]["id"] != quiet_id));
    assert!(first.iter().all(|row| row["post"]["text"].is_string()));
    assert!(second
        .iter()
        .any(|row| { row["actor"]["username"] == "matt" && row["comment"]["mine"] == true }));
    let (status, bad) = empty_req(&app, "GET", "/v1/feed?cursor=%%%", Some(&ted)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");

    sqlx::query("UPDATE comments SET created_at = 100000 WHERE book_id = ? AND text = 'c0'")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE reposts SET created_at = 100000 WHERE user_id = ? AND book_id = ? AND post_id = '0'",
    )
    .bind(&ted_id)
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    let tied = feed_all(&app, &ted).await;
    assert_eq!(tied, feed_all(&app, &ted).await);
    assert_eq!(tied[0]["at"], 100000);
    assert_eq!(tied[0]["type"], "repost");
    assert_eq!(tied[0]["actor"]["username"], "ted");
    assert_eq!(tied[0]["post"]["id"], "0");
    assert_eq!(tied[1]["at"], 100000);
    assert_eq!(tied[1]["type"], "comment");
    assert_eq!(tied[1]["comment"]["text"], "c0");
    assert_feed_order(&tied);
}

#[tokio::test]
async fn profile_reposts_page_newest_first() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Pages", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    share(&app, &matt, &id).await;
    let pool = open_db(tmp.path()).await;
    for idx in 3..31 {
        sqlx::query(
            "INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES (?, ?, 'Body', 'Ch', ?)",
        )
        .bind(&id)
        .bind(idx)
        .bind(format!("fill {idx}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    for idx in 0..31 {
        let (status, body) = empty_req(
            &app,
            "PUT",
            &repost_path(&id, &idx.to_string()),
            Some(&matt),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    sqlx::query("UPDATE reposts SET created_at = CAST(post_id AS INTEGER) WHERE book_id = ?")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();

    let (status, profile) = empty_req(&app, "GET", "/v1/users/matt", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{profile}");
    assert_eq!(profile["repostCount"], 31);

    let mut cursor: Option<String> = None;
    let mut posts = Vec::new();
    for _ in 0..4 {
        let path = match &cursor {
            Some(cursor) => format!("/v1/users/matt/reposts?cursor={}", percent_encode(cursor)),
            None => "/v1/users/matt/reposts".to_string(),
        };
        let (status, page) = empty_req(&app, "GET", &path, Some(&matt)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let items = page["items"].as_array().unwrap();
        if page["nextCursor"].is_null() {
            posts.extend(
                items
                    .iter()
                    .map(|row| row["post"]["id"].as_str().unwrap().to_string()),
            );
            break;
        }
        assert_eq!(items.len(), 30, "{page}");
        posts.extend(
            items
                .iter()
                .map(|row| row["post"]["id"].as_str().unwrap().to_string()),
        );
        cursor = Some(page["nextCursor"].as_str().unwrap().to_string());
    }
    let expected: Vec<String> = (0..31).rev().map(|idx| idx.to_string()).collect();
    assert_eq!(posts, expected);
    let (status, bad) = empty_req(
        &app,
        "GET",
        "/v1/users/matt/reposts?cursor=%%%",
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
}

#[tokio::test]
async fn profile_books_are_shared_and_not_samples() {
    let (app, _tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, private_book) = upload(&app, &matt, "Private", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{private_book}");
    let private_id = private_book["id"].as_str().unwrap();
    let (status, sample) = upload(&app, &matt, "Sample", "Author", &epub, None, true).await;
    assert_eq!(status, StatusCode::OK, "{sample}");
    let sample_id = sample["id"].as_str().unwrap();
    assert_eq!(sample["isSample"], true);
    share(&app, &matt, sample_id).await;
    let (status, shared_book) = upload(&app, &matt, "Shared", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{shared_book}");
    let shared_id = shared_book["id"].as_str().unwrap();
    share(&app, &matt, shared_id).await;

    for token in [&matt, &ted] {
        let (status, page) = empty_req(&app, "GET", "/v1/users/matt/books", Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let ids: Vec<&str> = page["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec![shared_id], "{page}");
        assert_eq!(page["items"][0]["shared"], true);
        assert_eq!(page["items"][0]["isSample"], false);
        assert_eq!(page["items"][0]["ownerUsername"], "matt");
    }
    let (status, owner) = empty_req(&app, "GET", "/v1/users/matt/books", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{owner}");
    assert_eq!(owner["items"][0]["mine"], true);
    let (status, other) = empty_req(&app, "GET", "/v1/users/matt/books", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{other}");
    assert_eq!(other["items"][0]["mine"], false);

    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    let shelf: Vec<&str> = library["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert!(shelf.contains(&sample_id), "{library}");
    assert!(!shelf.contains(&private_id));

    let (status, missing) = empty_req(&app, "GET", "/v1/users/nobody/books", Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    let (status, missing_reposts) =
        empty_req(&app, "GET", "/v1/users/nobody/reposts", Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing_reposts}");
}

#[tokio::test]
async fn deleting_a_book_cascades_reposts() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Gone", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    share(&app, &matt, &id).await;
    let (status, body) = empty_req(&app, "PUT", &repost_path(&id, "0"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = empty_req(&app, "PUT", &repost_path(&id, "1"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&ted),
        json!({ "text": "bye" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let pool = open_db(tmp.path()).await;
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM reposts WHERE book_id = ?",
            Some(&id)
        )
        .await,
        2
    );
    let (status, _) = empty_req(&app, "DELETE", &format!("/v1/books/{id}"), Some(&matt)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM reposts WHERE book_id = ?",
            Some(&id)
        )
        .await,
        0
    );
    let feed = feed_all(&app, &matt).await;
    assert!(feed.iter().all(|row| row["book"]["id"] != id), "{feed:?}");
    assert!(feed_all(&app, &ted).await.is_empty());
}

fn posts_json(n: usize) -> String {
    let mut body = String::from("{\"posts\":[");
    for i in 0..n {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            "{{\"id\":\"{i}\",\"kind\":\"Body\",\"text\":\"t{i}\",\"chapter\":\"c\"}}"
        ));
    }
    body.push_str("]}");
    body
}

#[tokio::test]
async fn backfill_lets_writers_through() {
    let tmp = TempDir::new().unwrap();
    let pool = open_db(tmp.path()).await;
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('owner', 'owner', 'owner', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sessions (token, user_id, created_at) VALUES ('writer-token', 'owner', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES ('11111111-1111-4111-8111-111111111111', 'owner', 'Ready', 'Author', 'author', 1, 1, 0, 0, 1, 1),
                ('bulk-a', 'owner', 'A', 'Author', 'author', 2, 0, 0, 0, 0, 0),
                ('bulk-b', 'owner', 'B', 'Author', 'author', 3, 0, 0, 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES ('11111111-1111-4111-8111-111111111111', 0, 'Body', 'c', 'ready')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    for id in ["bulk-a", "bulk-b"] {
        let dir = tmp.path().join("books").join("owner").join(id);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("posts.json"), posts_json(24_000))
            .await
            .unwrap();
    }

    let config = Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits: test_limits(),
    };
    let app = build(config).await.unwrap();
    let pool = open_db(tmp.path()).await;
    let token = "writer-token";
    let bound = Duration::from_millis(1000);
    let started = std::time::Instant::now();
    let mut saw_pending = false;
    let mut samples = 0u32;
    loop {
        let pending = count(
            &pool,
            "SELECT COUNT(*) FROM books WHERE id IN ('bulk-a', 'bulk-b') AND posts_indexed = 0",
            None,
        )
        .await;
        if pending > 0 {
            saw_pending = true;
        }
        let call_started = std::time::Instant::now();
        let (status, body) = if samples % 2 == 0 {
            json_req(
                &app,
                "POST",
                "/v1/books/11111111-1111-4111-8111-111111111111/comments/0",
                Some(token),
                json!({ "text": format!("n{samples}") }),
            )
            .await
        } else {
            json_req(
                &app,
                "PATCH",
                "/v1/books/11111111-1111-4111-8111-111111111111",
                Some(token),
                json!({ "progressIndex": samples }),
            )
            .await
        };
        let elapsed = call_started.elapsed();
        assert_ne!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(
            elapsed < bound,
            "write waited {elapsed:?} during backfill (pending {pending})"
        );
        samples += 1;
        if pending == 0 && saw_pending {
            break;
        }
        // A gap so backfill can take the write lock. Much shorter than the
        // stall this test is watching for.
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "backfill did not finish"
        );
    }
    assert!(saw_pending);
    assert!(samples > 1, "no writes overlapped backfill");
}

#[tokio::test]
async fn post_ids_must_be_canonical_and_counts_match_lists() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Ids", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    share(&app, &matt, &id).await;

    for post_id in ["00", "000", "+0", "abc", "0 ", "01"] {
        let (status, body) = empty_req(&app, "PUT", &repost_path(&id, post_id), Some(&matt)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{post_id} {body}");
        let (status, body) = json_req(
            &app,
            "POST",
            &format!("/v1/books/{id}/comments/{}", percent_encode(post_id)),
            Some(&matt),
            json!({ "text": "nope" }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{post_id} {body}");
    }
    for post_id in ["3", "999"] {
        let (status, body) = empty_req(&app, "PUT", &repost_path(&id, post_id), Some(&matt)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{post_id} {body}");
        let (status, body) = json_req(
            &app,
            "POST",
            &format!("/v1/books/{id}/comments/{}", percent_encode(post_id)),
            Some(&matt),
            json!({ "text": "missing" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{post_id} {body}");
    }

    let (status, body) = empty_req(&app, "PUT", &repost_path(&id, "0"), Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
        json!({ "text": "real-comment" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let pool = open_db(tmp.path()).await;
    let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username_norm = 'matt'")
        .fetch_one(&pool)
        .await
        .unwrap();
    for post_id in ["00", "000", "+0", "abc", "999"] {
        sqlx::query(
            "INSERT INTO reposts (user_id, book_id, post_id, created_at) VALUES (?, ?, ?, 1)",
        )
        .bind(&user_id)
        .bind(&id)
        .bind(post_id)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO comments (id, user_id, book_id, post_id, text, created_at)
         VALUES ('legacy-abc', ?, ?, 'abc', 'alias-comment', 2),
                ('legacy-00', ?, ?, '00', 'zero-alias', 3)",
    )
    .bind(&user_id)
    .bind(&id)
    .bind(&user_id)
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();

    let (status, profile) = empty_req(&app, "GET", "/v1/users/matt", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{profile}");
    let posts = profile_page_all(&app, &matt, "/v1/users/matt/posts").await;
    let reposts = profile_page_all(&app, &matt, "/v1/users/matt/reposts").await;
    assert_eq!(profile["postCount"], posts.len() as i64, "{posts:?}");
    assert_eq!(profile["repostCount"], reposts.len() as i64, "{reposts:?}");
    let comment_texts: Vec<&str> = posts
        .iter()
        .map(|row| row["comment"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(comment_texts, vec!["real-comment"]);
    assert_eq!(reposts.len(), 1, "{reposts:?}");
    assert_eq!(reposts[0]["post"]["id"], "0");

    let feed = feed_all(&app, &matt).await;
    let identities: Vec<String> = feed.iter().map(feed_identity).collect();
    let mut unique = identities.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), identities.len(), "{identities:?}");
    assert!(identities.iter().all(|row| !row.contains("alias-comment")));
    assert!(identities.iter().all(|row| !row.contains("zero-alias")));
    assert_eq!(
        identities
            .iter()
            .filter(|row| row.starts_with("repost:"))
            .count(),
        1,
        "{identities:?}"
    );
}

async fn profile_page_all(app: &axum::Router, token: &str, path: &str) -> Vec<Value> {
    let mut cursor: Option<String> = None;
    let mut items = Vec::new();
    for _ in 0..10 {
        let page_path = match &cursor {
            Some(cursor) => format!("{path}?cursor={}", percent_encode(cursor)),
            None => path.to_string(),
        };
        let (status, page) = empty_req(app, "GET", &page_path, Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        let batch = page["items"].as_array().unwrap();
        items.extend(batch.clone());
        match page["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => return items,
        }
    }
    panic!("profile cursor did not end");
}

#[tokio::test]
async fn delete_own_repost_works_after_unshare() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ted = register(&app, "ted", "password1").await;
    let epub = common::epub();
    let (status, book) = upload(&app, &matt, "Undo", "Author", &epub, None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    share(&app, &matt, &id).await;
    let (status, body) = empty_req(&app, "PUT", &repost_path(&id, "0"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    unshare(&app, &matt, &id).await;

    let (status, body) = empty_req(&app, "DELETE", &repost_path(&id, "0"), Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["reposted"], false);
    assert_eq!(body["reposts"], 0);
    let (status, again) = empty_req(&app, "DELETE", &repost_path(&id, "0"), Some(&ted)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{again}");

    let pool = open_db(tmp.path()).await;
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM reposts WHERE book_id = ?",
            Some(&id)
        )
        .await,
        0
    );
    share(&app, &matt, &id).await;
    let (status, profile) = empty_req(&app, "GET", "/v1/users/ted", Some(&ted)).await;
    assert_eq!(status, StatusCode::OK, "{profile}");
    assert_eq!(profile["repostCount"], 0);
    let reposts = profile_page_all(&app, &ted, "/v1/users/ted/reposts").await;
    assert!(reposts.is_empty(), "{reposts:?}");
    let feed = feed_all(&app, &matt).await;
    assert!(
        feed.iter()
            .all(|row| row["type"] != "repost" || row["actor"]["username"] != "ted"),
        "{feed:?}"
    );
}

#[tokio::test]
async fn backfill_skips_an_unreadable_epub_after_recording_it() {
    let tmp = TempDir::new().unwrap();
    let pool = open_db(tmp.path()).await;
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('owner', 'owner', 'owner', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES ('bad-book', 'owner', 'Bad', 'Author', 'author', 1, 0, 0, 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let dir = tmp.path().join("books").join("owner").join("bad-book");
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("book.epub"), b"not an epub")
        .await
        .unwrap();

    let config = Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits: test_limits(),
    };
    let _app = build(config.clone()).await.unwrap();
    let pool = open_db(tmp.path()).await;
    let started = std::time::Instant::now();
    loop {
        let flag = count(
            &pool,
            "SELECT posts_indexed FROM books WHERE id = 'bad-book'",
            None,
        )
        .await;
        if flag == -1 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "backfill left posts_indexed at {flag}"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    drop(_app);

    tokio::fs::write(dir.join("book.epub"), common::epub())
        .await
        .unwrap();
    let pool = open_db(tmp.path()).await;
    sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES ('sentinel', 'owner', 'Sentinel', 'Author', 'author', 2, 1, 0, 0, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    tokio::fs::create_dir_all(tmp.path().join("books").join("owner").join("sentinel"))
        .await
        .unwrap();
    tokio::fs::write(
        tmp.path()
            .join("books")
            .join("owner")
            .join("sentinel")
            .join("posts.json"),
        posts_json(1),
    )
    .await
    .unwrap();
    pool.close().await;

    let _app = build(config).await.unwrap();
    let pool = open_db(tmp.path()).await;
    let started = std::time::Instant::now();
    loop {
        let flag = count(
            &pool,
            "SELECT posts_indexed FROM books WHERE id = 'sentinel'",
            None,
        )
        .await;
        if flag == 1 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "second backfill did not finish"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(
        count(
            &pool,
            "SELECT posts_indexed FROM books WHERE id = 'bad-book'",
            None
        )
        .await,
        -1
    );
    assert!(!dir.join("posts.json").exists());
}

#[tokio::test]
async fn vacuum_keeps_fts_aligned_with_post_ids() {
    let tmp = TempDir::new().unwrap();
    let pool = open_db(tmp.path()).await;
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('user', 'matt', 'matt', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO books (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES ('book', 'user', 'B', 'A', 'a', 1, 2, 0, 0, 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO posts (book_id, idx, kind, chapter, text)
         VALUES ('book', 0, 'Body', 'c', 'secretword'), ('book', 1, 'Body', 'c', 'otherword')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM posts WHERE idx = 0")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO posts (book_id, idx, kind, chapter, text) VALUES ('book', 0, 'Body', 'c', 'secretword')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("VACUUM").execute(&pool).await.unwrap();
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM post_fts WHERE post_fts MATCH 'secretword'",
            None
        )
        .await,
        1
    );
    let posts_sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'posts'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(posts_sql.contains("id INTEGER PRIMARY KEY"), "{posts_sql}");
    let fts_sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'post_fts'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(fts_sql.contains("content_rowid='id'"), "{fts_sql}");
    let comments_index: i64 = count(
        &pool,
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'comments_created'",
        None,
    )
    .await;
    assert_eq!(comments_index, 1);
}

#[tokio::test]
async fn me_reports_admin() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, body) = empty_req(&app, "GET", "/v1/me", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["isAdmin"], false);

    grant(tmp.path(), "matt").await;
    let (status, body) = empty_req(&app, "GET", "/v1/me", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["username"], "matt");
    assert_eq!(body["isAdmin"], true);
}

#[tokio::test]
async fn instance_is_public_and_live() {
    let (app, tmp) = app().await;
    let (status, body) = empty_req(&app, "GET", "/v1/instance", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "allowSignup": true }));
    let (status, body) = empty_req(&app, "GET", "/v1/instance", Some("not-a-session")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_object().unwrap().len(), 1);

    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "allowSignup": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = empty_req(&app, "GET", "/v1/instance", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], false);
}

#[tokio::test]
async fn admin_routes_require_admin() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ada = register(&app, "ada", "password1").await;
    grant(tmp.path(), "matt").await;

    let cases = [
        ("GET", "/v1/admin/settings", None),
        ("PATCH", "/v1/admin/settings", Some(json!({}))),
        ("GET", "/v1/admin/users", None),
        (
            "PATCH",
            "/v1/admin/users/ada",
            Some(json!({ "isAdmin": false })),
        ),
        ("GET", "/v1/admin/server", None),
        ("POST", "/v1/admin/requests/test", None),
    ];
    for (method, path, body) in cases {
        let (status, response) = match &body {
            Some(body) => json_req(&app, method, path, None, body.clone()).await,
            None => empty_req(&app, method, path, None).await,
        };
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{method} {path} {response}"
        );

        let (status, response) = match &body {
            Some(body) => json_req(&app, method, path, Some(&ada), body.clone()).await,
            None => empty_req(&app, method, path, Some(&ada)).await,
        };
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path} {response}");
        assert_eq!(response["error"], "Admins only.");

        let (status, response) = match body {
            Some(body) => json_req(&app, method, path, Some(&matt), body).await,
            None => empty_req(&app, method, path, Some(&matt)).await,
        };
        assert!(status.is_success(), "{method} {path} {status} {response}");
    }

    let (code, _, err) = admin_cli(tmp.path(), AdminCommand::Revoke("matt".into()), "").await;
    assert_eq!(code, 0, "{err}");
    let (status, body) = empty_req(&app, "GET", "/v1/admin/settings", Some(&matt)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Admins only.");
}

#[tokio::test]
async fn settings_seed_from_config_once() {
    let tmp = TempDir::new().unwrap();
    let seeded = config_at(tmp.path(), false, MIN_EPUB_BYTES);
    let app = build(seeded.clone()).await.unwrap();
    let (code, _, err) = {
        let mut input = std::io::Cursor::new(b"password1\n".to_vec());
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = cli::run(
            AdminCommand::Create("matt".into()),
            &seeded,
            &mut input,
            &mut out,
            &mut err,
        )
        .await;
        (code, out, err)
    };
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let token = login(&app, "matt", "password1").await;
    let (status, body) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], false);

    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "allowSignup": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], true);

    let app = build(config_at(tmp.path(), false, MIN_EPUB_BYTES))
        .await
        .unwrap();
    let (status, body) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], true);
}

#[tokio::test]
async fn signup_toggle_is_live() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "allowSignup": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = json_req(
        &app,
        "POST",
        "/v1/auth/register",
        None,
        json!({ "username": "ada", "password": "password1" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Signups are off.");

    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "allowSignup": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = json_req(
        &app,
        "POST",
        "/v1/auth/register",
        None,
        json!({ "username": "ada", "password": "password1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn max_epub_setting_is_live() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "maxEpubBytes": MIN_EPUB_BYTES }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["maxEpubBytes"], MIN_EPUB_BYTES);

    let big = vec![1u8; (MIN_EPUB_BYTES as usize) + 8];
    let (status, body) = upload(&app, &token, "Big", "A", &big, None, false).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains(&format!("limit is {MIN_EPUB_BYTES} bytes")),
        "{error}"
    );

    let (status, book) = upload(&app, &token, "Ok", "A", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
}

#[tokio::test]
async fn settings_patch_validation() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, before) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    let original = settings_doc(tmp.path()).await;

    let cases = [
        (json!({ "future": 1 }), "Unknown setting \"future\"."),
        (
            json!({ "allowSignup": "no" }),
            "allowSignup is the wrong type.",
        ),
        (
            json!({ "maxEpubBytes": 10 }),
            "maxEpubBytes must be 1 to 1024 MB.",
        ),
        (
            json!({ "maxEpubBytes": 1_073_741_825u64 }),
            "maxEpubBytes must be 1 to 1024 MB.",
        ),
        (json!([1]), "Settings patch must be a JSON object."),
    ];
    for (body, expected) in cases {
        let (status, response) =
            json_req(&app, "PATCH", "/v1/admin/settings", Some(&token), body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
        assert_eq!(response["error"], expected);
        assert_eq!(settings_doc(tmp.path()).await, original, "{expected}");
    }

    let (status, body) =
        json_req(&app, "PATCH", "/v1/admin/settings", Some(&token), json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], before["allowSignup"]);
    assert_eq!(body["maxEpubBytes"], before["maxEpubBytes"]);
    assert_eq!(settings_doc(tmp.path()).await, original);
}

#[tokio::test]
async fn languages_patch_round_trips() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, body) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["languages"], json!(["en"]));
    let codes: Vec<&str> = body["supportedLanguages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        ["en", "es", "fr", "de", "it", "pt", "nl", "sv", "pl", "ru", "ja", "zh", "ko"]
    );
    assert_eq!(body["supportedLanguages"][3]["name"], "German");

    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "languages": ["en", "de"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["languages"], json!(["en", "de"]));
    assert_eq!(body["supportedLanguages"][0]["code"], "en");
    let doc = settings_doc(tmp.path()).await;
    assert!(doc.contains("\"languages\":[\"en\",\"de\"]"), "{doc}");
    assert!(!doc.contains("supportedLanguages"), "{doc}");

    let (status, again) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["languages"], json!(["en", "de"]));

    let (status, rejected) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "supportedLanguages": [{ "code": "fr", "name": "French" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    assert_eq!(rejected["error"], "Unknown setting \"supportedLanguages\".");
    assert_eq!(settings_doc(tmp.path()).await, doc);

    let (status, rejected) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "languages": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected}");
    let max = codes.len();
    assert_eq!(
        rejected["error"],
        format!("languages must list 1 to {max} supported language codes.")
    );
    assert_eq!(settings_doc(tmp.path()).await, doc);
}

#[tokio::test]
async fn settings_doc_forward_compatible() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    {
        let pool = open_db(tmp.path()).await;
        sqlx::query("UPDATE settings SET doc = ? WHERE id = 1")
            .bind(r#"{"future":1,"maxEpubBytes":67108864}"#)
            .execute(&pool)
            .await
            .unwrap();
    }
    let (status, body) = empty_req(&app, "GET", "/v1/admin/settings", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], false);
    assert_eq!(body["maxEpubBytes"], 67_108_864);
    assert!(body.get("future").is_none(), "{body}");

    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "allowSignup": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], true);
    assert_eq!(body["maxEpubBytes"], 67_108_864);
    let doc = settings_doc(tmp.path()).await;
    assert!(doc.contains("\"future\""), "{doc}");
}

#[tokio::test]
async fn admin_users_list_and_toggle() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    let ada = register(&app, "ada", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, book) = upload(&app, &ada, "Emma", "Austen", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    {
        let pool = open_db(tmp.path()).await;
        sqlx::query("UPDATE users SET created_at = 1 WHERE username_norm = 'ada'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET created_at = 2 WHERE username_norm = 'matt'")
            .execute(&pool)
            .await
            .unwrap();
    }

    let (status, body) = empty_req(&app, "GET", "/v1/admin/users", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["username"], "ada");
    assert_eq!(items[0]["isAdmin"], false);
    assert_eq!(items[0]["mine"], false);
    assert_eq!(items[0]["books"], 1);
    assert!(items[0]["joinedAt"].is_number());
    assert_eq!(items[1]["username"], "matt");
    assert_eq!(items[1]["isAdmin"], true);
    assert_eq!(items[1]["mine"], true);
    assert_eq!(items[1]["books"], 0);

    let (status, row) = json_req(
        &app,
        "PATCH",
        "/v1/admin/users/ada",
        Some(&matt),
        json!({ "isAdmin": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["isAdmin"], true);
    assert_eq!(row["books"], 1);
    assert_eq!(row["mine"], false);
    let (status, again) = json_req(
        &app,
        "PATCH",
        "/v1/admin/users/Ada",
        Some(&matt),
        json!({ "isAdmin": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["isAdmin"], true);

    let (status, row) = json_req(
        &app,
        "PATCH",
        "/v1/admin/users/ada",
        Some(&matt),
        json!({ "isAdmin": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["isAdmin"], false);

    let (status, missing) = json_req(
        &app,
        "PATCH",
        "/v1/admin/users/nobody",
        Some(&matt),
        json!({ "isAdmin": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");

    let (status, self_row) = json_req(
        &app,
        "PATCH",
        "/v1/admin/users/Matt",
        Some(&matt),
        json!({ "isAdmin": false }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{self_row}");
    assert_eq!(self_row["error"], "You can't change your own admin access.");
}

#[tokio::test]
async fn user_patch_rejections_are_json() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    register(&app, "ada", "password1").await;
    grant(tmp.path(), "matt").await;
    for body in [
        r#"{"isAdmin":"no"}"#,
        r#"{"isAdmin":true,"extra":1}"#,
        "not-json",
        "[1]",
    ] {
        let req = Request::builder()
            .method("PATCH")
            .uri("/v1/admin/users/ada")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let (status, bytes) = send(&app, req).await;
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("{body} was not json: {}", String::from_utf8_lossy(&bytes)));
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} {value}");
        assert_eq!(value["error"], "User patch must be a JSON object.");
    }
}

#[tokio::test]
async fn server_info_counts() {
    let (app, tmp) = app().await;
    let matt = register(&app, "matt", "password1").await;
    register(&app, "ada", "password1").await;
    grant(tmp.path(), "matt").await;
    let (status, book) = upload(&app, &matt, "Emma", "Austen", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let (status, thread) = json_req(
        &app,
        "POST",
        &format!("/v1/books/{id}/comments/0"),
        Some(&matt),
        json!({ "text": "A note." }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    let (status, repost) = empty_req(
        &app,
        "PUT",
        &format!("/v1/books/{id}/posts/0/repost"),
        Some(&matt),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{repost}");

    let (status, info) = empty_req(&app, "GET", "/v1/admin/server", Some(&matt)).await;
    assert_eq!(status, StatusCode::OK, "{info}");
    assert_eq!(info["users"], 2);
    assert_eq!(info["books"], 1);
    assert_eq!(info["comments"], 1);
    assert_eq!(info["reposts"], 1);
    assert!(info["storageBytes"].as_u64().unwrap() > 0, "{info}");
}

#[tokio::test]
async fn migration_006_existing_users_not_admin() {
    let tmp = TempDir::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    let migrator = sqlx::migrate!("./migrations");
    let mut conn = pool.acquire().await.unwrap();
    Migrate::ensure_migrations_table(&mut *conn).await.unwrap();
    for migration in migrator.iter() {
        if migration.version < 6 {
            Migrate::apply(&mut *conn, migration).await.unwrap();
        }
    }
    drop(conn);
    sqlx::query(
        "INSERT INTO users (id, username, username_norm, password_hash, created_at)
         VALUES ('user', 'matt', 'matt', 'x', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before = sqlx::query_scalar::<_, i64>("SELECT is_admin FROM users WHERE id = 'user'")
        .fetch_one(&pool)
        .await;
    assert!(before.is_err(), "is_admin exists before migration 006");
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let is_admin: i64 = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = 'user'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(is_admin, 0);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    drop(pool);

    let _app = build(config_at(tmp.path(), false, 32 * 1024 * 1024))
        .await
        .unwrap();
    let doc = settings_doc(tmp.path()).await;
    let value: Value = serde_json::from_str(&doc).unwrap();
    assert_eq!(value["allowSignup"], false);
    assert_eq!(value["maxEpubBytes"], 32 * 1024 * 1024);
}

async fn auth_app(auth: AuthConfig) -> (mewo_server::AppState, axum::Router, TempDir) {
    let tmp = TempDir::new().unwrap();
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes: 1024 * 1024,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth,
        limits: test_limits(),
    })
    .await
    .unwrap();
    state.passwords.warm().await;
    let app = router(state.clone());
    (state, app, tmp)
}

fn rate(burst: u32, every_secs: u64) -> Rate {
    Rate {
        burst,
        every: Duration::from_secs(every_secs),
    }
}

fn with_ip(mut req: Request<Body>, ip: &str) -> Request<Body> {
    let addr = SocketAddr::new(ip.parse::<IpAddr>().unwrap(), 9);
    req.extensions_mut().insert(ConnectInfo(addr));
    req
}

async fn call(
    app: &axum::Router,
    req: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Bytes) {
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body)
}

async fn post_auth(
    app: &axum::Router,
    path: &str,
    ip: &str,
    forwarded: Option<&str>,
    username: &str,
    password: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(forwarded) = forwarded {
        builder = builder.header("x-forwarded-for", forwarded);
    }
    let req = builder
        .body(Body::from(
            json!({ "username": username, "password": password }).to_string(),
        ))
        .unwrap();
    let (status, headers, bytes) = call(app, with_ip(req, ip)).await;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, value)
}

async fn post_auth_raw(
    app: &axum::Router,
    path: &str,
    ip: &str,
    forwarded: &[u8],
    username: &str,
    password: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let value = axum::http::HeaderValue::from_bytes(forwarded).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", value)
        .body(Body::from(
            json!({ "username": username, "password": password }).to_string(),
        ))
        .unwrap();
    let (status, headers, bytes) = call(app, with_ip(req, ip)).await;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, value)
}

fn assert_limited(status: StatusCode, headers: &axum::http::HeaderMap, body: &Value) {
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    let header = headers
        .get(header::RETRY_AFTER)
        .expect("Retry-After")
        .to_str()
        .unwrap();
    let secs: u64 = header.parse().unwrap();
    assert!(secs >= 1, "{header}");
    assert_eq!(
        body["error"],
        format!("Too many attempts. Try again in {secs} seconds.")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn at_most_two_password_ops_run_at_once() {
    let (state, app, _tmp) = auth_app(roomy_auth()).await;
    let start = std::sync::Arc::new(tokio::sync::Barrier::new(12));
    let mut handles = Vec::new();
    for i in 0..12 {
        let app = app.clone();
        let start = std::sync::Arc::clone(&start);
        handles.push(tokio::spawn(async move {
            start.wait().await;
            json_req(
                &app,
                "POST",
                "/v1/auth/register",
                None,
                json!({ "username": format!("user{i:02}"), "password": "password1" }),
            )
            .await
        }));
    }
    for handle in handles {
        let (status, body) = handle.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let stats = state.passwords.stats();
    assert_eq!(stats.peak_in_flight, 2);
    assert_eq!(stats.hashes, 12);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_password_queue_returns_503() {
    let mut auth = roomy_auth();
    auth.queue_wait = Duration::from_millis(1);
    let (state, app, _tmp) = auth_app(auth).await;
    let start = std::sync::Arc::new(tokio::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for i in 0..8 {
        let app = app.clone();
        let start = std::sync::Arc::clone(&start);
        handles.push(tokio::spawn(async move {
            start.wait().await;
            post_auth(
                &app,
                "/v1/auth/register",
                "203.0.113.7",
                None,
                &format!("busy{i:02}"),
                "password1",
            )
            .await
        }));
    }
    let mut saw_ok = false;
    let mut saw_busy = false;
    for handle in handles {
        let (status, headers, body) = handle.await.unwrap();
        match status {
            StatusCode::OK => saw_ok = true,
            StatusCode::SERVICE_UNAVAILABLE => {
                saw_busy = true;
                assert_eq!(
                    headers.get(header::RETRY_AFTER).unwrap().to_str().unwrap(),
                    "1"
                );
                assert_eq!(body["error"], "Server is busy. Try again in a moment.");
            }
            other => panic!("{other} {body}"),
        }
    }
    assert!(saw_ok, "a permit should still serve a register");
    assert!(saw_busy, "the queue wait should reject someone");
    assert!(state.passwords.stats().peak_in_flight <= 2);
}

#[tokio::test]
async fn a_public_ip_is_throttled_after_its_burst() {
    let mut auth = roomy_auth();
    auth.login_ip = rate(3, 30);
    auth.login_user = rate(100, 60);
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/register",
        "1.1.1.1",
        None,
        "matt",
        "password1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for n in 0..3 {
        let (status, _, body) =
            post_auth(&app, "/v1/auth/login", "1.1.1.1", None, "matt", "wrongpass").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{n} {body}");
    }
    let (status, headers, body) =
        post_auth(&app, "/v1/auth/login", "1.1.1.1", None, "matt", "wrongpass").await;
    assert_limited(status, &headers, &body);
    let (status, _, body) =
        post_auth(&app, "/v1/auth/login", "8.8.8.8", None, "matt", "wrongpass").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
async fn login_username_limit_holds_across_ips_and_a_success_refunds_it() {
    let mut auth = roomy_auth();
    auth.login_user = rate(2, 60);
    auth.login_ip = rate(100, 30);
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/register",
        "203.0.113.4",
        None,
        "matt",
        "password1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for (n, ip) in ["1.1.1.1", "8.8.8.8"].into_iter().enumerate() {
        let (status, _, body) =
            post_auth(&app, "/v1/auth/login", ip, None, "matt", "wrongpass").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{n} {body}");
    }
    let (status, headers, body) =
        post_auth(&app, "/v1/auth/login", "9.9.9.9", None, "matt", "wrongpass").await;
    assert_limited(status, &headers, &body);
    let (status, headers, body) =
        post_auth(&app, "/v1/auth/login", "1.0.0.1", None, "matt", "password1").await;
    assert_limited(status, &headers, &body);

    let mut auth = roomy_auth();
    auth.login_user = rate(2, 60);
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/register",
        "203.0.113.4",
        None,
        "matt",
        "password1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for n in 0..5 {
        let (status, _, body) = post_auth(
            &app,
            "/v1/auth/login",
            "203.0.113.4",
            None,
            "matt",
            "password1",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{n} {body}");
        assert_eq!(body["username"], "matt");
        let token = body["token"].as_str().expect("token");
        assert!(!token.is_empty());
        let (status, me) = empty_req(&app, "GET", "/v1/me", Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        assert_eq!(me["username"], "matt");
    }
}

#[tokio::test]
async fn register_ip_limit_returns_429() {
    let mut auth = roomy_auth();
    auth.register_ip = rate(2, 600);
    let (_state, app, _tmp) = auth_app(auth).await;
    for (n, name) in ["ada", "bea"].into_iter().enumerate() {
        let (status, _, body) = post_auth(
            &app,
            "/v1/auth/register",
            "203.0.113.7",
            None,
            name,
            "password1",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{n} {body}");
    }
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/register",
        "203.0.113.7",
        None,
        "cyd",
        "password1",
    )
    .await;
    assert_limited(status, &headers, &body);
}

#[tokio::test]
async fn forwarded_for_is_ignored_at_zero_hops_and_used_at_one() {
    let mut auth = roomy_auth();
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(100, 60);
    auth.trusted_proxy_hops = 0;
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("1.1.1.1"),
        "matt",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("8.8.8.8"),
        "ada",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);

    let mut auth = roomy_auth();
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(100, 60);
    auth.trusted_proxy_hops = 1;
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("1.1.1.1, 203.0.113.9"),
        "matt",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("1.1.1.1, 203.0.113.10"),
        "ada",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("1.1.1.1, 203.0.113.9"),
        "bea",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("not-an-ip"),
        "cyd",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("also-bad"),
        "dee",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);
}

#[tokio::test]
async fn a_short_forwarded_for_uses_the_leftmost_address() {
    let mut auth = roomy_auth();
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(100, 60);
    auth.trusted_proxy_hops = 2;
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("1.1.1.1"),
        "matt",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("8.8.8.8"),
        "ada",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "9.9.9.9",
        Some("1.1.1.1"),
        "bea",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);
    let (status, _, body) =
        post_auth(&app, "/v1/auth/login", "9.9.9.9", None, "cyd", "wrongpass").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) =
        post_auth(&app, "/v1/auth/login", "9.9.9.9", None, "dee", "wrongpass").await;
    assert_limited(status, &headers, &body);
}

#[tokio::test]
async fn private_peer_skips_the_ip_limit_but_not_the_username() {
    let mut auth = roomy_auth();
    auth.trusted_proxy_hops = 0;
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(2, 60);
    let (_state, app, _tmp) = auth_app(auth).await;
    for name in ["matt", "ada"] {
        let (status, _, body) = post_auth(
            &app,
            "/v1/auth/register",
            "10.42.1.5",
            None,
            name,
            "password1",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{name} {body}");
    }
    for n in 0..2 {
        let (status, _, body) = post_auth(
            &app,
            "/v1/auth/login",
            "10.42.1.5",
            Some("1.1.1.1"),
            "matt",
            "wrongpass",
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{n} {body}");
    }
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.1.5",
        Some("8.8.8.8"),
        "matt",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.1.5",
        None,
        "ada",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
async fn a_non_ascii_forwarded_for_is_still_ip_limited() {
    let mut auth = roomy_auth();
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(100, 60);
    auth.trusted_proxy_hops = 1;
    let (_state, app, _tmp) = auth_app(auth).await;
    let header = b"\xff, 203.0.113.9, 162.158.1.1";
    let (status, _, body) = post_auth_raw(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        header,
        "matt",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth_raw(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        header,
        "ada",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);

    let (status, _, body) = post_auth_raw(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        b"\xff",
        "bea",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth_raw(
        &app,
        "/v1/auth/login",
        "10.42.0.9",
        b"\xff, not-an-ip",
        "cyd",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);
}

#[tokio::test]
async fn an_unparseable_hop_does_not_slide_to_a_neighbor() {
    let mut auth = roomy_auth();
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(100, 60);
    auth.trusted_proxy_hops = 2;
    let (_state, app, _tmp) = auth_app(auth).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        Some("evil, unknown, 162.158.1.1"),
        "matt",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.9",
        Some("1.2.3.4, unknown, 162.158.1.1"),
        "ada",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);

    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        Some("203.0.113.9:1234, 162.158.1.1"),
        "bea",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        Some("203.0.113.9:9, 8.8.8.8"),
        "cyd",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);

    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        Some("[2001:db8::1]:443, 162.158.1.1"),
        "dee",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let (status, headers, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        Some("[2001:db8::ffff]:443, 162.158.1.1"),
        "fay",
        "wrongpass",
    )
    .await;
    assert_limited(status, &headers, &body);
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "10.42.0.8",
        Some("[2001:db8:0:1::1]:443, 162.158.1.1"),
        "gus",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
async fn a_busy_password_service_does_not_spend_the_attempt() {
    let mut auth = roomy_auth();
    auth.max_concurrent = 0;
    auth.queue_wait = Duration::from_millis(1);
    auth.login_ip = rate(1, 60);
    auth.login_user = rate(1, 60);
    auth.register_ip = rate(1, 600);
    auth.register_user = rate(1, 600);
    let (state, app, _tmp) = auth_app(auth).await;
    for n in 0..3 {
        let (status, headers, body) = post_auth(
            &app,
            "/v1/auth/login",
            "203.0.113.9",
            None,
            "matt",
            "password1",
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{n} {body}");
        assert_eq!(
            headers.get(header::RETRY_AFTER).unwrap().to_str().unwrap(),
            "1"
        );
    }
    for n in 0..3 {
        let (status, _, body) = post_auth(
            &app,
            "/v1/auth/register",
            "203.0.113.9",
            None,
            "matt",
            "password1",
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{n} {body}");
    }
    let stats = state.passwords.stats();
    assert_eq!(stats.hashes, 0);
    assert_eq!(stats.verifies, 0);
}

#[tokio::test]
async fn a_missing_user_still_verifies_and_a_taken_name_does_not_hash() {
    let (state, app, _tmp) = auth_app(roomy_auth()).await;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "203.0.113.7",
        None,
        "nobody",
        "password1",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let stats = state.passwords.stats();
    assert_eq!(stats.verifies, 1);
    assert_eq!(stats.dummy_verifies, 1);
    assert_eq!(stats.hashes, 0);

    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/register",
        "203.0.113.7",
        None,
        "matt",
        "password1",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/login",
        "203.0.113.8",
        None,
        "matt",
        "wrongpass",
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    let stats = state.passwords.stats();
    assert_eq!(stats.verifies, 2);
    assert_eq!(stats.dummy_verifies, 1);
    assert_eq!(stats.hashes, 1);

    let hashes = state.passwords.stats().hashes;
    let (status, _, body) = post_auth(
        &app,
        "/v1/auth/register",
        "203.0.113.7",
        None,
        "MATT",
        "password1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "That username is taken.");
    assert_eq!(state.passwords.stats().hashes, hashes);

    let left = app.clone();
    let right = app.clone();
    let (first, second) = tokio::join!(
        post_auth(
            &left,
            "/v1/auth/register",
            "203.0.113.7",
            None,
            "ada",
            "password1"
        ),
        post_auth(
            &right,
            "/v1/auth/register",
            "203.0.113.8",
            None,
            "ada",
            "password1"
        ),
    );
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "{} {}",
        first.2,
        second.2
    );
}

fn table_froms(sql: &str) -> usize {
    let lower = sql.to_ascii_lowercase();
    ["from comments", "from reposts"]
        .into_iter()
        .map(|needle| {
            let mut count = 0;
            let mut start = 0;
            while let Some(pos) = lower[start..].find(needle) {
                let at = start + pos + needle.len();
                let boundary = lower
                    .as_bytes()
                    .get(at)
                    .is_none_or(|byte| !byte.is_ascii_alphanumeric());
                if boundary {
                    count += 1;
                }
                start += pos + needle.len();
            }
            count
        })
        .sum()
}

struct SqlLiteral {
    value: String,
    end: usize,
}

/// String literals in `source`, so the social-read guard looks at SQL text
/// and not at comments. Byte index `end` is just past the closing quote.
fn rust_string_literals(source: &str) -> Vec<SqlLiteral> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !source.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if bytes[i] >= 128 {
            i += source[i..].chars().next().unwrap().len_utf8();
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'\'' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'\\' {
                i += 1;
            }
            if i < bytes.len() && source.is_char_boundary(i) {
                i += source[i..].chars().next().unwrap().len_utf8();
            }
            if i < bytes.len() && bytes[i] == b'\'' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'r' {
            let hashes = source[i + 1..]
                .bytes()
                .take_while(|byte| *byte == b'#')
                .count();
            let quote = i + 1 + hashes;
            if quote < bytes.len() && bytes[quote] == b'"' {
                let content = quote + 1;
                let closer = format!("\"{}", "#".repeat(hashes));
                if let Some(rel) = source[content..].find(&closer) {
                    let end_content = content + rel;
                    out.push(SqlLiteral {
                        value: source[content..end_content].to_string(),
                        end: end_content + closer.len(),
                    });
                    i = end_content + closer.len();
                    continue;
                }
            }
        }
        if bytes[i] == b'"' {
            let mut j = i + 1;
            let mut value = String::new();
            while j < bytes.len() {
                if bytes[j] == b'\\' && j + 1 < bytes.len() {
                    value.push(bytes[j + 1] as char);
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' {
                    out.push(SqlLiteral { value, end: j + 1 });
                    i = j + 1;
                    break;
                }
                let ch = source[j..].chars().next().unwrap();
                value.push(ch);
                j += ch.len_utf8();
            }
            if j >= bytes.len() {
                break;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// SQL strings that read `comments` or `reposts` without `social_visible`
/// and without an `-- own rows only` tag. The fragment may be a `format!`
/// argument after the string; the scan stops at the next query.
fn uncovered_social_reads(source: &str) -> Vec<String> {
    let mut misses = Vec::new();
    for literal in rust_string_literals(source) {
        let froms = table_froms(&literal.value);
        if froms == 0 || literal.value.contains("-- own rows only") {
            continue;
        }
        let after = &source[literal.end..];
        let end = [after.find("format!"), after.find("sqlx::query")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(after.len());
        let visibles = after[..end].matches("social_visible").count()
            + literal.value.matches("social_visible").count();
        if visibles < froms {
            let snippet: String = literal.value.chars().take(80).collect();
            misses.push(format!(
                "{froms} FROM, {visibles} social_visible: {snippet}"
            ));
        }
    }
    misses
}

#[test]
fn every_social_read_uses_social_visible() {
    let bare = "fn read() { sqlx::query(\"SELECT * FROM comments c\"); }\n";
    assert!(
        !uncovered_social_reads(bare).is_empty(),
        "a comments query with no fragment must fail the scan"
    );
    let tagged =
        "fn read() { sqlx::query(\"DELETE FROM comments WHERE id = ? -- own rows only\"); }\n";
    assert!(uncovered_social_reads(tagged).is_empty(), "{tagged}");
    let formatted = "fn read() { let _ = format!(\"SELECT * FROM comments c WHERE {s}\", s = social_visible(\"c\")); }\n";
    assert!(uncovered_social_reads(formatted).is_empty(), "{formatted}");
    let one_short = "fn read() { let _ = format!(\"SELECT * FROM comments c UNION ALL SELECT * FROM reposts r\", s = social_visible(\"c\")); }\n";
    assert!(
        !uncovered_social_reads(one_short).is_empty(),
        "two reads need two fragments"
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut misses = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for miss in uncovered_social_reads(&text) {
                    misses.push(format!("{}: {miss}", path.display()));
                }
            }
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}

#[tokio::test]
async fn many_epub_parts_share_one_request_budget() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let part = common::with_stored_padding(600 * 1024);
    assert!(
        part.len() < MIN_EPUB_BYTES as usize && part.len() >= 600 * 1024,
        "{}",
        part.len()
    );
    let envelope = request_body_limit(MIN_EPUB_BYTES);
    assert!(part.len() * 12 > envelope);
    assert!(part.len() * 2 < envelope);

    let (status, body) = upload_fields(&app, &token, &repeat("epub", &part, 12)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["error"], "Request body is too large.");
    assert!(tmp_entries(tmp.path()).await.is_empty(), "tmp left behind");

    let (status, book) = upload_fields(&app, &token, &repeat("epub", &part, 2)).await;
    assert_eq!(status, StatusCode::OK, "{book}");
}

#[tokio::test]
async fn the_envelope_follows_the_live_epub_cap() {
    let (app, tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    grant(tmp.path(), "matt").await;
    let junk = vec![0u8; 600 * 1024];
    let epub = common::epub();
    let mut fields = repeat("epub", &junk, 9);
    fields.push(("epub", Some("book.epub"), epub.as_slice()));
    let payload: usize = fields.iter().map(|(_, _, bytes)| bytes.len()).sum();
    assert!(payload > request_body_limit(MIN_EPUB_BYTES));
    assert!(payload < request_body_limit(4 * 1024 * 1024));

    let (status, body) = upload_fields(&app, &token, &fields).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["error"], "Request body is too large.");

    let (status, body) = json_req(
        &app,
        "PATCH",
        "/v1/admin/settings",
        Some(&token),
        json!({ "maxEpubBytes": 4 * 1024 * 1024 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, book) = upload_fields(&app, &token, &fields).await;
    assert_eq!(status, StatusCode::OK, "{book}");
}

#[tokio::test]
async fn many_small_fields_share_the_envelope() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let chunk = vec![b'a'; 8 * 1024];
    let fields = repeat("extra", &chunk, 700);
    assert!(700 * chunk.len() > request_body_limit(MIN_EPUB_BYTES));
    let (status, body) = upload_fields(&app, &token, &fields).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["error"], "Request body is too large.");
}

#[tokio::test]
async fn declared_length_is_rejected_before_any_work() {
    let (mut state, tmp) = open_state(test_limits(), MIN_EPUB_BYTES).await;
    let calls = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&calls);
    state.disk = DiskGuard::with_probe(1, move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(u64::MAX)
    });
    let app = router(state);
    let token = register(&app, "matt", "password1").await;
    let declared = request_body_limit(MIN_EPUB_BYTES) as u64 + 1;
    let (status, _headers, body) =
        post_books(&app, &token, Some(declared), b"short".to_vec()).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(body["error"], "Request body is too large.");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(tmp_entries(tmp.path()).await.is_empty());
}

#[tokio::test]
async fn an_over_envelope_length_is_413_before_the_body() {
    let tmp = TempDir::new().unwrap();
    let app = build(config_at(tmp.path(), true, MIN_EPUB_BYTES))
        .await
        .unwrap();
    let token = register(&app, "matt", "password1").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app.clone();
    tokio::spawn(async move {
        axum::serve(listener, served).await.unwrap();
    });
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let declared = request_body_limit(MIN_EPUB_BYTES) as u64 + 1;
    let head = format!(
        "POST /v1/books HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Authorization: Bearer {token}\r\n\
         Content-Type: multipart/form-data; boundary=x\r\n\
         Content-Length: {declared}\r\n\
         Connection: close\r\n\
         \r\n"
    );
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(head.as_bytes()).await.unwrap();
    let mut buf = [0u8; 128];
    let n = tokio::time::timeout(Duration::from_secs(1), stream.read(&mut buf))
        .await
        .expect("413 did not arrive within a second")
        .unwrap();
    let text = String::from_utf8_lossy(&buf[..n]);
    assert!(text.starts_with("HTTP/1.1 413"), "{text}");
    assert!(tmp_entries(tmp.path()).await.is_empty());
}

#[tokio::test]
async fn an_epub_under_the_cap_plus_a_full_cover_fits_the_envelope() {
    let (app, _tmp) = app_with(true, MIN_EPUB_BYTES).await;
    let token = register(&app, "matt", "password1").await;
    let slack = 8 * 1024;
    let epub = common::with_stored_padding((MIN_EPUB_BYTES as usize) - slack);
    assert!(epub.len() < MIN_EPUB_BYTES as usize, "{}", epub.len());
    assert!(
        epub.len() > MIN_EPUB_BYTES as usize - slack * 2,
        "{}",
        epub.len()
    );
    let cover = vec![0u8; MAX_COVER_BYTES as usize];
    let payload = epub.len()
        + cover.len()
        + "Just under".len()
        + "Author".len()
        + "author".len()
        + "false".len();
    assert!(payload < request_body_limit(MIN_EPUB_BYTES), "{payload}");
    let (status, book) = upload(
        &app,
        &token,
        "Just under",
        "Author",
        &epub,
        Some(&cover),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{book}");
}

#[tokio::test]
async fn a_low_disk_refuses_the_upload_and_not_a_read() {
    let floor = 1024 * 1024u64;
    let (mut state, tmp) = open_state(test_limits(), MIN_EPUB_BYTES).await;
    let free = Arc::new(AtomicU64::new(floor + 64 * 1024 * 1024));
    let reported = Arc::clone(&free);
    state.disk = DiskGuard::with_probe(floor, move |_| Ok(reported.load(Ordering::SeqCst)));
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Pride", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    let (status, likes) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{likes}");

    free.store(floor - 1, Ordering::SeqCst);
    let (status, body) = upload(&app, &token, "More", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{body}");
    assert_eq!(
        body["error"],
        "The server is low on disk space. Try again later."
    );
    assert_eq!(book_count(&state.pool).await, 1);
    assert!(tmp_entries(tmp.path()).await.is_empty());

    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{library}");
    let (status, _) = send(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/v1/books/{id}/epub"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, likes) =
        empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{likes}");

    free.store(floor + 64 * 1024 * 1024, Ordering::SeqCst);
    let (status, book) = upload(&app, &token, "Again", "Jane", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
}

#[tokio::test]
async fn declared_length_counts_against_free_disk() {
    let floor = 1024 * 1024u64;
    let (mut state, tmp) = open_state(test_limits(), MIN_EPUB_BYTES).await;
    state.disk = DiskGuard::with_probe(floor, move |_| Ok(floor + 100));
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let (content_type, body) = multipart(&[
        ("title", None, b"Pride"),
        ("author", None, b"Jane"),
        ("epub", Some("book.epub"), &common::epub()),
    ]);
    let (status, _, value) =
        post_books_typed(&app, &token, &content_type, Some(5000), body.clone()).await;
    assert_eq!(status, StatusCode::INSUFFICIENT_STORAGE, "{value}");
    assert_eq!(book_count(&state.pool).await, 0);
    assert!(tmp_entries(tmp.path()).await.is_empty());

    let (status, _, value) = post_books_typed(&app, &token, &content_type, Some(50), body).await;
    assert_eq!(status, StatusCode::OK, "{value}");
}

#[tokio::test(flavor = "multi_thread")]
async fn one_parse_permit_is_shared_by_parallel_uploads() {
    let limits = LimitsConfig {
        max_parses: 1,
        parse_wait: Duration::from_secs(30),
        min_free_bytes: 0,
    };
    let (state, tmp) = open_state(limits, 8 * 1024 * 1024).await;
    let app = router(state.clone());
    let epub = common::book(true, 1024 * 1024);
    let start = Arc::new(tokio::sync::Barrier::new(6));
    let mut handles = Vec::new();
    for i in 0..6 {
        let app = app.clone();
        let epub = epub.clone();
        let start = Arc::clone(&start);
        handles.push(tokio::spawn(async move {
            let token = register(&app, &format!("user{i:02}"), "password1").await;
            start.wait().await;
            upload(&app, &token, "Book", "Author", &epub, None, false).await
        }));
    }
    for handle in handles {
        let (status, book) = handle.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{book}");
    }
    let stats = state.parsers.stats();
    assert_eq!(stats.peak_in_flight, 1);
    assert_eq!(stats.parses, 6);
    assert_eq!(book_count(&state.pool).await, 6);
    drop(tmp);
}

#[tokio::test]
async fn a_full_parse_queue_is_503_and_leaves_no_book() {
    let limits = LimitsConfig {
        max_parses: 0,
        parse_wait: Duration::from_millis(10),
        min_free_bytes: 0,
    };
    let (state, tmp) = open_state(limits, MIN_EPUB_BYTES).await;
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let (content_type, body) = multipart(&[("epub", Some("book.epub"), &common::epub())]);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/books")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();
    let (status, headers, bytes) = call(&app, req).await;
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
    assert_eq!(value["error"], "Server is busy. Try again in a moment.");
    assert_eq!(
        headers.get(header::RETRY_AFTER).unwrap().to_str().unwrap(),
        "1"
    );
    assert_eq!(book_count(&state.pool).await, 0);
    assert!(tmp_entries(tmp.path()).await.is_empty());
    assert_eq!(state.parsers.stats().busy, 1);
    assert_eq!(state.parsers.stats().parses, 0);
}

#[tokio::test]
async fn two_lazy_fills_wait_together_when_the_parser_is_busy() {
    let wait = Duration::from_millis(300);
    let limits = LimitsConfig {
        max_parses: 0,
        parse_wait: wait,
        min_free_bytes: 0,
    };
    let (state, _tmp) = open_state(limits, MIN_EPUB_BYTES).await;
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'matt'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    for id in [
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
    ] {
        sqlx::query(
            "INSERT INTO books
             (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
             VALUES (?, ?, 'Legacy', 'Author', 'author', 1, 0, 0, 0, 0, 0)",
        )
        .bind(id)
        .bind(&user_id)
        .execute(&state.pool)
        .await
        .unwrap();
        let dir = state.data_dir.join("books").join(&user_id).join(id);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("book.epub"), common::epub())
            .await
            .unwrap();
    }
    let started = std::time::Instant::now();
    let first = posts_status(&app, &token, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1");
    let second = posts_status(&app, &token, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2");
    let (left, right) = tokio::join!(first, second);
    let elapsed = started.elapsed();
    assert_eq!(left, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(right, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        elapsed < wait + Duration::from_millis(150),
        "fills waited one after another: {elapsed:?}"
    );
}

async fn posts_status(app: &axum::Router, token: &str, id: &str) -> StatusCode {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/books/{id}/posts"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = call(app, req).await;
    status
}

#[tokio::test]
async fn a_legacy_fill_waits_on_the_parse_pool() {
    let limits = LimitsConfig {
        max_parses: 0,
        parse_wait: Duration::from_millis(10),
        min_free_bytes: 0,
    };
    let (state, _tmp) = open_state(limits, MIN_EPUB_BYTES).await;
    let app = router(state.clone());
    let token = register(&app, "matt", "password1").await;
    let user_id: String = sqlx::query_scalar("SELECT id FROM users WHERE username = 'matt'")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    let id = "11111111-1111-4111-8111-111111111111";
    sqlx::query(
        "INSERT INTO books
         (id, user_id, title, author, handle, imported_at, post_count, progress_index, is_sample, shared, posts_indexed)
         VALUES (?, ?, 'Legacy', 'Author', 'author', 1, 0, 0, 0, 0, 0)",
    )
    .bind(id)
    .bind(&user_id)
    .execute(&state.pool)
    .await
    .unwrap();
    let dir = state.data_dir.join("books").join(&user_id).join(id);
    tokio::fs::create_dir_all(&dir).await.unwrap();
    tokio::fs::write(dir.join("book.epub"), common::epub())
        .await
        .unwrap();
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/books/{id}/posts"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let (status, headers, bytes) = call(&app, req).await;
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
    assert_eq!(value["error"], "Server is busy. Try again in a moment.");
    assert_eq!(
        headers.get(header::RETRY_AFTER).unwrap().to_str().unwrap(),
        "1"
    );
    let indexed: i64 = sqlx::query_scalar("SELECT posts_indexed FROM books WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(indexed, 0);
    assert!(!dir.join("posts.json").exists());
}

#[tokio::test]
async fn fresh_signups_are_off_and_an_existing_row_wins() {
    let (app, _tmp) = app_with(false, MIN_EPUB_BYTES).await;
    let (status, body) = empty_req(&app, "GET", "/v1/instance", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], false);
    let (status, body) = json_req(
        &app,
        "POST",
        "/v1/auth/register",
        None,
        json!({ "username": "matt", "password": "password1" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "Signups are off.");

    let (app, tmp) = app_with(true, MIN_EPUB_BYTES).await;
    let (status, body) = empty_req(&app, "GET", "/v1/instance", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], true);
    let app = build(config_at(tmp.path(), false, MIN_EPUB_BYTES))
        .await
        .unwrap();
    let (status, body) = empty_req(&app, "GET", "/v1/instance", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["allowSignup"], true);
}

fn repeat<'a>(
    name: &'a str,
    bytes: &'a [u8],
    n: usize,
) -> Vec<(&'a str, Option<&'a str>, &'a [u8])> {
    let file = (name == "epub").then_some("book.epub");
    (0..n).map(|_| (name, file, bytes)).collect()
}

async fn open_state(limits: LimitsConfig, max_epub_bytes: u64) -> (mewo_server::AppState, TempDir) {
    let tmp = TempDir::new().unwrap();
    let state = prepare(Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
        web_dir: None,
        allow_signup: true,
        max_epub_bytes,
        downloads_dir: None,
        openlibrary_url: "https://openlibrary.org".into(),
        auth: roomy_auth(),
        limits,
    })
    .await
    .unwrap();
    (state, tmp)
}

async fn book_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM books")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn post_books(
    app: &axum::Router,
    token: &str,
    content_length: Option<u64>,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    post_books_typed(
        app,
        token,
        "multipart/form-data; boundary=x",
        content_length,
        body,
    )
    .await
}

async fn post_books_typed(
    app: &axum::Router,
    token: &str,
    content_type: &str,
    content_length: Option<u64>,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/books")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type);
    if let Some(len) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, len.to_string());
    }
    let (status, headers, bytes) = call(app, builder.body(Body::from(body)).unwrap()).await;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, value)
}

async fn tmp_entries(data: &Path) -> Vec<String> {
    let tmp = data.join("tmp");
    let mut names = Vec::new();
    let mut dir = match tokio::fs::read_dir(&tmp).await {
        Ok(dir) => dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return names,
        Err(err) => panic!("read {tmp:?}: {err}"),
    };
    while let Some(entry) = dir.next_entry().await.unwrap() {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names
}
