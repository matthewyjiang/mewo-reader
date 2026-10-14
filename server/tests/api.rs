use axum::body::{Body, Bytes};
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use mewo_server::build;
use mewo_server::config::Config;
use serde_json::{json, Value};
use sqlx::migrate::Migrate;
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
