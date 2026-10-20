use crate::admin::{self, AdminChange};
use crate::config::Config;
use crate::error::ApiError;
use sqlx::SqlitePool;
use std::io::{self, BufRead, BufReader, Cursor, IsTerminal, Write};

/// Where `create` reads the password. A terminal prompts with no echo. A pipe
/// or a test cursor yields one line. The process TTY is not consulted, so an
/// injected reader is never ignored.
pub trait LineIn: BufRead + Send {
    fn interactive(&self) -> bool;
}

impl LineIn for BufReader<io::Stdin> {
    fn interactive(&self) -> bool {
        io::stdin().is_terminal()
    }
}

impl<T: AsRef<[u8]> + Send> LineIn for Cursor<T> {
    fn interactive(&self) -> bool {
        false
    }
}

pub const USAGE: &str = "\
Usage:
  mewo-server
  mewo-server admin list
  mewo-server admin grant <username>
  mewo-server admin revoke <username>
  mewo-server admin create <username>
";

#[derive(Debug)]
pub enum Command {
    Serve,
    Admin(AdminCommand),
    Help,
}

#[derive(Debug)]
pub enum AdminCommand {
    List,
    Grant(String),
    Revoke(String),
    Create(String),
}

/// `argv[1..]`. No args → [`Command::Serve`]. A bad command returns usage text;
/// the process prints it to stderr and exits 2.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(Command::Serve),
        [flag] if flag == "help" || flag == "--help" || flag == "-h" => Ok(Command::Help),
        [cmd, rest @ ..] if cmd == "admin" => parse_admin(rest),
        _ => Err(USAGE.to_string()),
    }
}

fn parse_admin(args: &[String]) -> Result<Command, String> {
    let command = match args {
        [verb] if verb == "list" => AdminCommand::List,
        [verb, name] if verb == "grant" => AdminCommand::Grant(name.clone()),
        [verb, name] if verb == "revoke" => AdminCommand::Revoke(name.clone()),
        [verb, name] if verb == "create" => AdminCommand::Create(name.clone()),
        _ => return Err(USAGE.to_string()),
    };
    Ok(Command::Admin(command))
}

/// Connects with [`crate::db::connect`], which also runs migrations. Exit 0 on
/// success or a no-op, 1 on a user error, 2 on a database error. `stdin` /
/// `out` are injected so tests drive `create` without a terminal.
pub async fn run(
    cmd: AdminCommand,
    config: &Config,
    stdin: &mut dyn LineIn,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let pool = match crate::db::connect(&config.database_url(), &config.data_dir).await {
        Ok(pool) => pool,
        Err(error) => {
            let _ = writeln!(err, "{error}");
            return 2;
        }
    };
    match dispatch(&pool, cmd, stdin, out, err).await {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(err, "{error}");
            2
        }
    }
}

async fn dispatch(
    pool: &SqlitePool,
    cmd: AdminCommand,
    stdin: &mut dyn LineIn,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    match cmd {
        AdminCommand::List => list(pool, out, err).await,
        AdminCommand::Grant(name) => change(pool, &name, true, out, err).await,
        AdminCommand::Revoke(name) => change(pool, &name, false, out, err).await,
        AdminCommand::Create(name) => create(pool, &name, stdin, out, err).await,
    }
}

async fn list(
    pool: &SqlitePool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let accounts = match admin::accounts(pool).await {
        Ok(accounts) => accounts,
        Err(error) => return Ok(user_or_db(error, err)),
    };
    for (username, is_admin) in accounts {
        if is_admin {
            writeln!(out, "{username:<8} admin")?;
        } else {
            writeln!(out, "{username}")?;
        }
    }
    Ok(0)
}

async fn change(
    pool: &SqlitePool,
    name: &str,
    admin: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    match admin::set_admin(pool, name, admin).await {
        Ok(AdminChange::Changed | AdminChange::Unchanged) => {
            if admin {
                writeln!(out, "{name} is an admin.")?;
            } else {
                writeln!(out, "{name} is not an admin.")?;
            }
            Ok(0)
        }
        Err(ApiError::NotFound) => {
            writeln!(err, "No account named {name}.")?;
            Ok(1)
        }
        Err(error) => Ok(user_or_db(error, err)),
    }
}

async fn create(
    pool: &SqlitePool,
    name: &str,
    stdin: &mut dyn LineIn,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let password = read_password(stdin)?;
    match admin::create_admin(pool, name, &password).await {
        Ok(()) => {
            writeln!(out, "{name} is an admin.")?;
            Ok(0)
        }
        Err(error) => Ok(user_or_db(error, err)),
    }
}

fn user_or_db(error: ApiError, err: &mut dyn Write) -> i32 {
    match error {
        ApiError::Internal => {
            let _ = writeln!(err, "Internal error.");
            2
        }
        other => {
            let _ = writeln!(err, "{other}");
            1
        }
    }
}

/// A terminal prompts with no echo. Anything else reads one line from `stdin`.
/// The password is never taken from argv or the environment.
fn read_password(stdin: &mut dyn LineIn) -> io::Result<String> {
    if stdin.interactive() {
        return rpassword::prompt_password("Password: ");
    }
    let mut line = String::new();
    stdin.read_line(&mut line)?;
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build;
    use crate::config::{Config, MIN_EPUB_BYTES};
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::io::Cursor;
    use std::path::Path;
    use tempfile::TempDir;
    use tower::ServiceExt;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_string()).collect()
    }

    fn config_at(path: &Path) -> Config {
        Config {
            listen: "127.0.0.1:0".into(),
            data_dir: path.to_path_buf(),
            web_dir: None,
            allow_signup: false,
            max_epub_bytes: MIN_EPUB_BYTES,
            downloads_dir: None,
            openlibrary_url: "https://openlibrary.org".into(),
        }
    }

    async fn cli(path: &Path, cmd: AdminCommand, stdin: &str) -> (i32, String, String) {
        let mut input = Cursor::new(stdin.as_bytes().to_vec());
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(cmd, &config_at(path), &mut input, &mut out, &mut err).await;
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    async fn post_json(app: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    #[test]
    fn parse_serve_admin_and_rejects_junk() {
        assert!(matches!(parse(words(&[])), Ok(Command::Serve)));
        assert!(matches!(parse(words(&["--help"])), Ok(Command::Help)));
        assert!(matches!(
            parse(words(&["admin", "list"])),
            Ok(Command::Admin(AdminCommand::List))
        ));
        match parse(words(&["admin", "grant", "matt"])) {
            Ok(Command::Admin(AdminCommand::Grant(name))) => assert_eq!(name, "matt"),
            other => panic!("{other:?}"),
        }
        match parse(words(&["admin", "revoke", "ada"])) {
            Ok(Command::Admin(AdminCommand::Revoke(name))) => assert_eq!(name, "ada"),
            other => panic!("{other:?}"),
        }
        match parse(words(&["admin", "create", "matt"])) {
            Ok(Command::Admin(AdminCommand::Create(name))) => assert_eq!(name, "matt"),
            other => panic!("{other:?}"),
        }
        assert!(parse(words(&["nope"])).is_err());
        assert!(parse(words(&["admin"])).is_err());
        assert!(parse(words(&["admin", "grant"])).is_err());
        assert!(parse(words(&["admin", "grant", "matt", "extra"])).is_err());
    }

    #[tokio::test]
    async fn grant_twice_is_the_same_line_and_an_unknown_name_exits_1() {
        let tmp = TempDir::new().unwrap();
        let (code, out, err) = cli(
            tmp.path(),
            AdminCommand::Create("matt".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "matt is an admin.\n");

        let (code, out, err) = cli(tmp.path(), AdminCommand::Grant("matt".into()), "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "matt is an admin.\n");
        let (code, again, err) = cli(tmp.path(), AdminCommand::Grant("matt".into()), "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(again, out);

        let (code, out, err) = cli(tmp.path(), AdminCommand::Grant("nobody".into()), "").await;
        assert_eq!(code, 1, "{out}");
        assert_eq!(err, "No account named nobody.\n");
    }

    #[tokio::test]
    async fn revoke_of_a_non_admin_exits_0() {
        let tmp = TempDir::new().unwrap();
        let (code, _, err) = cli(
            tmp.path(),
            AdminCommand::Create("matt".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 0, "{err}");
        let (code, _, err) = cli(
            tmp.path(),
            AdminCommand::Create("ada".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 0, "{err}");
        let (code, _, err) = cli(tmp.path(), AdminCommand::Revoke("ada".into()), "").await;
        assert_eq!(code, 0, "{err}");
        let (code, out, err) = cli(tmp.path(), AdminCommand::Revoke("ada".into()), "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "ada is not an admin.\n");

        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query("UPDATE users SET created_at = 1 WHERE username_norm = 'matt'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET created_at = 2 WHERE username_norm = 'ada'")
            .execute(&pool)
            .await
            .unwrap();
        drop(pool);

        let (code, out, err) = cli(tmp.path(), AdminCommand::List, "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "matt     admin\nada\n");
    }

    #[tokio::test]
    async fn create_can_sign_in_when_signups_are_off() {
        let tmp = TempDir::new().unwrap();
        let (code, out, err) = cli(
            tmp.path(),
            AdminCommand::Create("matt".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "matt is an admin.\n");

        let (code, _, err) = cli(
            tmp.path(),
            AdminCommand::Create("matt".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(err, "That username is taken.\n");

        let (code, _, err) = cli(tmp.path(), AdminCommand::Create("ada".into()), "short\n").await;
        assert_eq!(code, 1);
        assert_eq!(err, "Password must be at least 8 characters.\n");

        let app = build(config_at(tmp.path())).await.unwrap();
        let (status, body) = post_json(
            &app,
            "/v1/auth/register",
            json!({ "username": "ada", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

        let (status, body) = post_json(
            &app,
            "/v1/auth/login",
            json!({ "username": "matt", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let token = body["token"].as_str().unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/me")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let me: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(me["isAdmin"], true);
    }

    #[tokio::test]
    async fn a_bad_database_exits_2() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("mewo.db");
        std::fs::create_dir(&db).unwrap();
        let (code, _, err) = cli(tmp.path(), AdminCommand::List, "").await;
        assert_eq!(code, 2, "{err}");
        assert!(!err.is_empty());
    }

    #[tokio::test]
    async fn grant_while_the_server_reads_the_same_file() {
        let tmp = TempDir::new().unwrap();
        let mut open = config_at(tmp.path());
        open.allow_signup = true;
        let app = build(open).await.unwrap();
        let (status, body) = post_json(
            &app,
            "/v1/auth/register",
            json!({ "username": "matt", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let token = body["token"].as_str().unwrap().to_string();

        let reader = app.clone();
        let token_for_reads = token.clone();
        let reads = tokio::spawn(async move {
            for _ in 0..30 {
                let response = reader
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri("/v1/me")
                            .header(header::AUTHORIZATION, format!("Bearer {token_for_reads}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                let text = String::from_utf8_lossy(&bytes);
                assert_eq!(status, StatusCode::OK, "{text}");
                assert!(!text.contains("SQLITE_BUSY"), "{text}");
            }
        });

        let (code, _, err) = cli(tmp.path(), AdminCommand::Grant("matt".into()), "").await;
        assert_eq!(code, 0, "{err}");
        reads.await.unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/me")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let me: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(me["isAdmin"], true);
    }
}
