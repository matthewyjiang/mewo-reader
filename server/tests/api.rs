use axum::body::{Body, Bytes};
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use mewo_server::config::Config;
use mewo_server::build;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

async fn app_with(allow_signup: bool, max_epub_bytes: u64) -> (axum::Router, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = Config {
        listen: "127.0.0.1:0".into(),
        data_dir: tmp.path().to_path_buf(),
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
        serde_json::from_slice(&bytes).unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
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
            if is_sample { b"true".as_slice() } else { b"false".as_slice() },
        ),
        ("epub", Some("book.epub"), epub),
    ];
    if let Some(cover) = cover {
        fields.push(("cover", Some("cover.jpg"), cover));
    }
    let (content_type, body) = multipart(&fields);
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
        b"epub-bytes",
        Some(b"jpeg"),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap().to_string();
    assert_eq!(book["title"], "Pride");
    assert_eq!(book["isSample"], false);
    assert_eq!(book["mine"], true);

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
    assert_eq!(patched["postCount"], 12);

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
    let (status, listed) = empty_req(&app, "GET", &format!("/v1/books/{id}/likes"), Some(&token)).await;
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
    assert_eq!(bytes.as_ref(), b"epub-bytes");

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
    let (status, book) = upload(&app, &matt, "Pride", "Jane", b"epub", None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();
    assert_eq!(book["mine"], true);

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
    assert_eq!(bytes.as_ref(), b"epub");

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
    let (status, book) = upload(&app, &matt, "Pride", "Jane", b"epub", None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();

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
    let (status, book) = upload(&app, &matt, "Pride", "Jane", b"epub", None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();

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
    let (status, book) = upload(&app, &matt, "Pride", "Jane", b"epub", None, false).await;
    assert_eq!(status, StatusCode::OK, "{book}");
    let id = book["id"].as_str().unwrap();

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

    let (status, matt_feed) =
        empty_req(&app, "GET", "/v1/profiles/matt/replies", Some(&ted)).await;
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
    let (status, first) = upload(&app, &token, "Sample", "Mewo", b"one", None, true).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = upload(&app, &token, "Sample", "Mewo", b"two", None, true).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(first["id"], second["id"]);
    let (status, library) = empty_req(&app, "GET", "/v1/library", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(library["books"].as_array().unwrap().len(), 1);

    let ted = register(&app, "ted", "password1").await;
    let (status, ted_sample) = upload(&app, &ted, "Sample", "Mewo", b"three", None, true).await;
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
    assert!(error.contains("asked 65") || error.contains("asked 200"), "{error}");
}

#[tokio::test]
async fn epub_over_two_megabytes_is_accepted() {
    let (app, _tmp) = app_with(true, 4 * 1024 * 1024).await;
    let token = register(&app, "matt", "password1").await;
    let epub = vec![0u8; 3 * 1024 * 1024];
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
