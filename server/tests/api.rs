use axum::body::{Body, Bytes};
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use mewo_server::build;
use mewo_server::config::Config;
use serde_json::{json, Value};
use sqlx::migrate::Migrate;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;
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
async fn oversized_epub_is_413() {
    let (app, _tmp) = app_with(true, 64).await;
    let token = register(&app, "matt", "password1").await;
    let (status, body) = upload(&app, &token, "Big", "A", &[0u8; 200], None, false).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(error.contains("MEWO_MAX_EPUB_BYTES=64"), "{error}");
    assert!(
        error.contains("asked 65") || error.contains("asked 200"),
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
async fn search_rejects_bad_queries_and_sanitizes_fts() {
    let (app, _tmp) = app().await;
    let token = register(&app, "matt", "password1").await;
    let (status, book) = upload(&app, &token, "Book", "Writer", &common::epub(), None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");

    for q in ["", "   ", "\t"] {
        let (status, body) = search(&app, Some(&token), q, "posts", None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{q:?} {body}");
        assert!(body["error"].as_str().unwrap().contains("Enter a search."));
    }
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
