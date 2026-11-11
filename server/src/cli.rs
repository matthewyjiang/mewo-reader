use crate::admin::{self, AdminChange};
use crate::auth::invite::{self, AdminInvite, InviteState};
use crate::config::Config;
use crate::error::ApiError;
use crate::parse_pool::ParsePool;
use crate::reparse::{self, Mode, Outcome, Report};
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
  mewo-server admin password <username>
  mewo-server admin invites create [--days <n>] [--note <text>]
  mewo-server admin invites list
  mewo-server admin invites revoke <id>
  mewo-server admin reparse <book-id> [--dry-run] [--force]
  mewo-server admin reparse --all [--dry-run] [--force]
  mewo-server admin works aliases [--dry-run] [--merge]
  mewo-server admin sessions revoke <username>
  mewo-server admin sessions revoke --all
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
    /// Sets a temporary password and prints it once.
    Password(String),
    Invites(InviteCommand),
    Reparse(ReparseArgs),
    Works(WorksCommand),
    /// `None` is `--all`. A username cannot be `--all`.
    RevokeSessions(Option<String>),
}

#[derive(Debug)]
pub enum WorksCommand {
    /// Dry-run unless `--merge` is set without `--dry-run`.
    Aliases { dry_run: bool, merge: bool },
}

#[derive(Debug)]
pub enum InviteCommand {
    Create { days: u32, note: Option<String> },
    List,
    Revoke(String),
}

#[derive(Debug)]
pub struct ReparseArgs {
    pub target: ReparseTarget,
    pub dry_run: bool,
    pub force: bool,
}

#[derive(Debug)]
pub enum ReparseTarget {
    One(String),
    All,
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
        [verb, name] if verb == "password" => AdminCommand::Password(name.clone()),
        [verb, rest @ ..] if verb == "invites" => parse_invites(rest)?,
        [verb, rest @ ..] if verb == "reparse" => parse_reparse(rest)?,
        [verb, rest @ ..] if verb == "works" => parse_works(rest)?,
        [verb, action, flag] if verb == "sessions" && action == "revoke" && flag == "--all" => {
            AdminCommand::RevokeSessions(None)
        }
        [verb, action, name] if verb == "sessions" && action == "revoke" => {
            AdminCommand::RevokeSessions(Some(name.clone()))
        }
        _ => return Err(USAGE.to_string()),
    };
    Ok(Command::Admin(command))
}

fn parse_invites(args: &[String]) -> Result<AdminCommand, String> {
    match args {
        [verb] if verb == "list" => Ok(AdminCommand::Invites(InviteCommand::List)),
        [verb, id] if verb == "revoke" && !id.starts_with('-') => {
            Ok(AdminCommand::Invites(InviteCommand::Revoke(id.clone())))
        }
        [verb, rest @ ..] if verb == "create" => {
            let mut days = None;
            let mut note = None;
            let mut index = 0;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--days" => {
                        if days.is_some() {
                            return Err(USAGE.to_string());
                        }
                        let Some(value) = rest.get(index + 1) else {
                            return Err(USAGE.to_string());
                        };
                        days = Some(parse_days(value)?);
                        index += 2;
                    }
                    "--note" => {
                        if note.is_some() {
                            return Err(USAGE.to_string());
                        }
                        let Some(value) = rest.get(index + 1) else {
                            return Err(USAGE.to_string());
                        };
                        if value.starts_with('-') {
                            return Err(USAGE.to_string());
                        }
                        note = Some(value.clone());
                        index += 2;
                    }
                    _ => return Err(USAGE.to_string()),
                }
            }
            Ok(AdminCommand::Invites(InviteCommand::Create {
                days: days.unwrap_or(invite::DEFAULT_DAYS),
                note,
            }))
        }
        _ => Err(USAGE.to_string()),
    }
}

fn parse_days(raw: &str) -> Result<u32, String> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(USAGE.to_string());
    }
    let days: u32 = raw.parse().map_err(|_| USAGE.to_string())?;
    if !(1..=invite::MAX_DAYS).contains(&days) {
        return Err(USAGE.to_string());
    }
    Ok(days)
}

fn parse_works(args: &[String]) -> Result<AdminCommand, String> {
    match args {
        [verb, rest @ ..] if verb == "aliases" => {
            let mut dry_run = false;
            let mut merge = false;
            for arg in rest {
                match arg.as_str() {
                    "--dry-run" => dry_run = true,
                    "--merge" => merge = true,
                    _ => return Err(USAGE.to_string()),
                }
            }
            Ok(AdminCommand::Works(WorksCommand::Aliases {
                dry_run,
                merge,
            }))
        }
        _ => Err(USAGE.to_string()),
    }
}

fn parse_reparse(args: &[String]) -> Result<AdminCommand, String> {
    let mut target = None;
    let mut dry_run = false;
    let mut force = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "--all" if target.is_none() => target = Some(ReparseTarget::All),
            id if !id.starts_with('-') && target.is_none() => {
                target = Some(ReparseTarget::One(id.to_owned()))
            }
            _ => return Err(USAGE.to_string()),
        }
    }
    let Some(target) = target else {
        return Err(USAGE.to_string());
    };
    Ok(AdminCommand::Reparse(ReparseArgs {
        target,
        dry_run,
        force,
    }))
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
    // Same upgrade as `prepare`, so revoke works before the server has started
    // on this binary. A second call finds no legacy table and returns 0.
    if let Err(error) = crate::auth::session::upgrade_legacy(&pool, crate::db::now_ms()).await {
        let _ = writeln!(err, "{error}");
        return 2;
    }
    match dispatch(&pool, config, cmd, stdin, out, err).await {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(err, "{error}");
            2
        }
    }
}

async fn dispatch(
    pool: &SqlitePool,
    config: &Config,
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
        AdminCommand::Password(name) => password(pool, &name, out, err).await,
        AdminCommand::Invites(command) => invites(pool, command, out, err).await,
        AdminCommand::Reparse(args) => reparse(pool, config, args, out, err).await,
        AdminCommand::Works(command) => works(config, command, out, err).await,
        AdminCommand::RevokeSessions(name) => revoke_sessions(pool, name, out, err).await,
    }
}

async fn reparse(
    pool: &SqlitePool,
    config: &Config,
    args: ReparseArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let data_dir = tokio::fs::canonicalize(&config.data_dir).await?;
    let parsers = ParsePool::new(&config.limits);
    let reparser = reparse::Reparser {
        pool,
        data_dir: &data_dir,
        parsers: &parsers,
        locks: None,
    };
    let mode = if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Apply
    };
    match args.target {
        ReparseTarget::One(id) => match reparser.one(&id, mode, args.force).await {
            Ok(report) => {
                write_report(out, err, &report)?;
                Ok(reparse_code(&[report]))
            }
            Err(error) => Ok(explain_reparse(&id, error, err)),
        },
        ReparseTarget::All => {
            let mut code = 0;
            match reparser
                .all(mode, args.force, |report| {
                    write_report(out, err, report)?;
                    if matches!(report.outcome, Outcome::Failed(_) | Outcome::Skipped(_)) {
                        code = 1;
                    }
                    Ok(())
                })
                .await
            {
                Ok(()) => Ok(code),
                Err(error) => Ok(user_or_db(error, err)),
            }
        }
    }
}

fn write_report(out: &mut dyn Write, err: &mut dyn Write, report: &Report) -> io::Result<()> {
    writeln!(out, "{}", format_reparse(report))?;
    if matches!(report.outcome, Outcome::Changed) {
        writeln!(
            err,
            "{}: posts were renumbered. Reload the book. A phone with an offline copy still has the old post ids.",
            report.book_id
        )?;
    }
    Ok(())
}

fn explain_reparse(id: &str, error: ApiError, err: &mut dyn Write) -> i32 {
    match error {
        ApiError::NotFound => {
            let _ = writeln!(err, "No book with id {id}.");
            1
        }
        other => user_or_db(other, err),
    }
}

fn format_reparse(report: &Report) -> String {
    format!(
        "{id}  {title}  {owner}  {old}->{new} posts  headings -{removed}/+{added}  snapped {snapped}  anchors {anchors}  {outcome}",
        id = report.book_id,
        title = report.title,
        owner = report.owner,
        old = report.old_posts,
        new = report.new_posts,
        removed = report.headings_removed,
        added = report.headings_added,
        snapped = report.snapped,
        anchors = report.anchors_moved.total(),
        outcome = outcome_label(&report.outcome),
    )
}

fn outcome_label(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Unchanged => "unchanged".to_owned(),
        Outcome::WouldChange => "would change".to_owned(),
        Outcome::Changed => "changed".to_owned(),
        Outcome::Skipped(reason) => format!("skipped: {reason}"),
        Outcome::Failed(message) => format!("failed: {message}"),
    }
}

fn reparse_code(reports: &[Report]) -> i32 {
    if reports
        .iter()
        .any(|report| matches!(report.outcome, Outcome::Failed(_) | Outcome::Skipped(_)))
    {
        1
    } else {
        0
    }
}

async fn works(
    config: &Config,
    command: WorksCommand,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let WorksCommand::Aliases { dry_run, merge } = command;
    let state = match crate::prepare(config.clone()).await {
        Ok(state) => state,
        Err(error) => {
            writeln!(err, "{error}")?;
            return Ok(2);
        }
    };
    // Default is a dry run. `--merge` performs it. `--dry-run` still wins.
    let apply = merge && !dry_run;
    match crate::requests::aliases::consolidate(&state, apply, out).await {
        Ok(()) => Ok(0),
        Err(error) => Ok(user_or_db(error, err)),
    }
}

async fn revoke_sessions(
    pool: &SqlitePool,
    name: Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let result = match &name {
        Some(name) => match crate::auth::session::revoke_username(pool, name).await {
            Err(ApiError::NotFound) => {
                writeln!(err, "No account named {name}.")?;
                return Ok(1);
            }
            other => other,
        },
        None => crate::auth::session::revoke_everyone(pool).await,
    };
    match result {
        Ok(count) => {
            writeln!(out, "revoked {count}.")?;
            Ok(0)
        }
        Err(error) => Ok(user_or_db(error, err)),
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
    match crate::accounts::set_admin(pool, None, name, admin).await {
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
    let passwords = crate::auth::Passwords::new(&crate::config::AuthConfig::default());
    match admin::create_admin(pool, &passwords, name, &password).await {
        Ok(()) => {
            writeln!(out, "{name} is an admin.")?;
            Ok(0)
        }
        Err(error) => Ok(user_or_db(error, err)),
    }
}

async fn password(
    pool: &SqlitePool,
    name: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let passwords = crate::auth::Passwords::new(&crate::config::AuthConfig::default());
    match crate::accounts::reset_password(pool, &passwords, None, name).await {
        Ok(password) => {
            writeln!(out, "Temporary password: {password}")?;
            Ok(0)
        }
        Err(ApiError::NotFound) => {
            writeln!(err, "No account named {name}.")?;
            Ok(1)
        }
        Err(error) => Ok(user_or_db(error, err)),
    }
}

async fn invites(
    pool: &SqlitePool,
    command: InviteCommand,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, io::Error> {
    let now = crate::db::now_ms();
    match command {
        InviteCommand::Create { days, note } => {
            match invite::create(pool, None, note.as_deref(), days, now).await {
                Ok((invite, token)) => {
                    writeln!(out, "id {}", invite.id)?;
                    writeln!(out, "expires_at {}", invite.expires_at)?;
                    writeln!(out, "Link: <your server>/signup?invite={token}")?;
                    Ok(0)
                }
                Err(error) => Ok(user_or_db(error, err)),
            }
        }
        InviteCommand::List => match invite::list(pool, now).await {
            Ok(items) => {
                for invite in items {
                    writeln!(out, "{}", invite_line(&invite))?;
                }
                Ok(0)
            }
            Err(error) => Ok(user_or_db(error, err)),
        },
        InviteCommand::Revoke(id) => match invite::revoke(pool, &id, now).await {
            Ok(_) => Ok(0),
            Err(ApiError::NotFound) => {
                writeln!(err, "No invite with id {id}.")?;
                Ok(1)
            }
            Err(error) => Ok(user_or_db(error, err)),
        },
    }
}

fn invite_line(invite: &AdminInvite) -> String {
    let note = invite
        .note
        .as_deref()
        .map(one_line)
        .unwrap_or_else(|| "-".to_owned());
    let used_by = invite.used_by.as_deref().unwrap_or("-");
    format!(
        "{}  {}  {}  {}  {}",
        invite.id,
        invite_state(invite.state),
        invite.expires_at,
        note,
        used_by
    )
}

fn invite_state(state: InviteState) -> &'static str {
    match state {
        InviteState::Active => "active",
        InviteState::Used => "used",
        InviteState::Expired => "expired",
        InviteState::Revoked => "revoked",
    }
}

fn one_line(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
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
    use crate::auth::Passwords;
    use crate::build;
    use crate::config::{AuthConfig, Config, LimitsConfig, MIN_EPUB_BYTES};
    use crate::error::ApiError;
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
            auth: AuthConfig::default(),
            limits: LimitsConfig {
                min_free_bytes: 0,
                ..LimitsConfig::default()
            },
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
        match parse(words(&["admin", "password", "ada"])) {
            Ok(Command::Admin(AdminCommand::Password(name))) => assert_eq!(name, "ada"),
            other => panic!("{other:?}"),
        }
        assert!(parse(words(&["admin", "password"])).is_err());
        assert!(parse(words(&["admin", "password", "ada", "extra"])).is_err());
        match parse(words(&["admin", "sessions", "revoke", "matt"])) {
            Ok(Command::Admin(AdminCommand::RevokeSessions(Some(name)))) => {
                assert_eq!(name, "matt")
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse(words(&["admin", "sessions", "revoke", "--all"])),
            Ok(Command::Admin(AdminCommand::RevokeSessions(None)))
        ));
        assert!(parse(words(&["nope"])).is_err());
        assert!(parse(words(&["admin"])).is_err());
        assert!(parse(words(&["admin", "grant"])).is_err());
        assert!(parse(words(&["admin", "grant", "matt", "extra"])).is_err());
        match parse(words(&["admin", "works", "aliases"])) {
            Ok(Command::Admin(AdminCommand::Works(WorksCommand::Aliases { dry_run, merge }))) => {
                assert!(!dry_run);
                assert!(!merge);
            }
            other => panic!("{other:?}"),
        }
        match parse(words(&[
            "admin",
            "works",
            "aliases",
            "--merge",
            "--dry-run",
        ])) {
            Ok(Command::Admin(AdminCommand::Works(WorksCommand::Aliases { dry_run, merge }))) => {
                assert!(dry_run);
                assert!(merge);
            }
            other => panic!("{other:?}"),
        }
        assert!(parse(words(&["admin", "works", "aliases", "--apply"])).is_err());
        match parse(words(&["admin", "reparse", "book", "--dry-run", "--force"])) {
            Ok(Command::Admin(AdminCommand::Reparse(args))) => {
                assert!(matches!(args.target, ReparseTarget::One(id) if id == "book"));
                assert!(args.dry_run);
                assert!(args.force);
            }
            other => panic!("{other:?}"),
        }
        match parse(words(&["admin", "reparse", "--all", "--dry-run"])) {
            Ok(Command::Admin(AdminCommand::Reparse(args))) => {
                assert!(matches!(args.target, ReparseTarget::All));
                assert!(args.dry_run);
                assert!(!args.force);
            }
            other => panic!("{other:?}"),
        }
        assert!(parse(words(&["admin", "reparse"])).is_err());
        assert!(parse(words(&["admin", "reparse", "--dry-run"])).is_err());
        assert!(parse(words(&["admin", "reparse", "--all", "book"])).is_err());
        assert!(parse(words(&["admin", "reparse", "book", "--nope"])).is_err());
        assert!(parse(words(&["admin", "sessions"])).is_err());
        assert!(parse(words(&["admin", "sessions", "revoke"])).is_err());
        assert!(parse(words(&["admin", "sessions", "revoke", "matt", "extra"])).is_err());
        assert!(parse(words(&["admin", "sessions", "revoke", "--all", "matt"])).is_err());
        match parse(words(&["admin", "invites", "create"])) {
            Ok(Command::Admin(AdminCommand::Invites(InviteCommand::Create { days, note }))) => {
                assert_eq!(days, 7);
                assert!(note.is_none());
            }
            other => panic!("{other:?}"),
        }
        match parse(words(&[
            "admin", "invites", "create", "--days", "3", "--note", "for ada",
        ])) {
            Ok(Command::Admin(AdminCommand::Invites(InviteCommand::Create { days, note }))) => {
                assert_eq!(days, 3);
                assert_eq!(note.as_deref(), Some("for ada"));
            }
            other => panic!("{other:?}"),
        }
        match parse(words(&[
            "admin", "invites", "create", "--note", "for ada", "--days", "14",
        ])) {
            Ok(Command::Admin(AdminCommand::Invites(InviteCommand::Create { days, note }))) => {
                assert_eq!(days, 14);
                assert_eq!(note.as_deref(), Some("for ada"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse(words(&["admin", "invites", "list"])),
            Ok(Command::Admin(AdminCommand::Invites(InviteCommand::List)))
        ));
        match parse(words(&["admin", "invites", "revoke", "abc"])) {
            Ok(Command::Admin(AdminCommand::Invites(InviteCommand::Revoke(id)))) => {
                assert_eq!(id, "abc")
            }
            other => panic!("{other:?}"),
        }
        for bad in [
            ["admin", "invites"].as_slice(),
            ["admin", "invites", "create", "extra"].as_slice(),
            ["admin", "invites", "create", "--days", "x"].as_slice(),
            ["admin", "invites", "create", "--days", "0"].as_slice(),
            ["admin", "invites", "create", "--days", "91"].as_slice(),
            ["admin", "invites", "create", "--days"].as_slice(),
            ["admin", "invites", "create", "--days", "3", "--days", "4"].as_slice(),
            ["admin", "invites", "create", "--note"].as_slice(),
            ["admin", "invites", "create", "--note", "--days"].as_slice(),
            ["admin", "invites", "create", "--nope"].as_slice(),
            ["admin", "invites", "list", "extra"].as_slice(),
            ["admin", "invites", "revoke"].as_slice(),
            ["admin", "invites", "revoke", "abc", "extra"].as_slice(),
        ] {
            assert!(parse(words(bad)).is_err(), "{bad:?}");
        }
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
    async fn create_on_a_taken_name_does_not_hash() {
        let tmp = TempDir::new().unwrap();
        let (code, _, err) = cli(
            tmp.path(),
            AdminCommand::Create("matt".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 0, "{err}");

        let passwords = Passwords::new(&AuthConfig::default());
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = crate::db::connect(&url, tmp.path()).await.unwrap();
        let err = admin::create_admin(&pool, &passwords, "MATT", "password1")
            .await
            .unwrap_err();
        match err {
            ApiError::Conflict(message) => assert_eq!(message, "That username is taken."),
            other => panic!("{other:?}"),
        }
        assert_eq!(passwords.stats().hashes, 0);

        let (code, _, err) = cli(
            tmp.path(),
            AdminCommand::Create("Matt".into()),
            "password1\n",
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(err, "That username is taken.\n");
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

    #[tokio::test]
    async fn sessions_revoke_counts_one_user_or_everyone() {
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
        let matt = body["token"].as_str().unwrap().to_string();
        let (status, body) = post_json(
            &app,
            "/v1/auth/login",
            json!({ "username": "matt", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let matt_again = body["token"].as_str().unwrap().to_string();
        let (status, body) = post_json(
            &app,
            "/v1/auth/register",
            json!({ "username": "ada", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let ada = body["token"].as_str().unwrap().to_string();

        let (code, out, err) = cli(
            tmp.path(),
            AdminCommand::RevokeSessions(Some("matt".into())),
            "",
        )
        .await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "revoked 2.\n");
        assert_eq!(me_status(&app, &matt).await, StatusCode::UNAUTHORIZED);
        assert_eq!(me_status(&app, &matt_again).await, StatusCode::UNAUTHORIZED);
        assert_eq!(me_status(&app, &ada).await, StatusCode::OK);

        let (code, out, err) = cli(
            tmp.path(),
            AdminCommand::RevokeSessions(Some("nobody".into())),
            "",
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert_eq!(err, "No account named nobody.\n");

        let (code, out, err) = cli(tmp.path(), AdminCommand::RevokeSessions(None), "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "revoked 1.\n");
        assert_eq!(me_status(&app, &ada).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sessions_revoke_all_hashes_legacy_rows_before_deleting() {
        let tmp = TempDir::new().unwrap();
        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let migrator = sqlx::migrate!("./migrations");
        let mut conn = pool.acquire().await.unwrap();
        sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
            .await
            .unwrap();
        for migration in migrator.iter() {
            if migration.version < 9 {
                sqlx::migrate::Migrate::apply(&mut *conn, migration)
                    .await
                    .unwrap();
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
            "INSERT INTO sessions (token, user_id, created_at) VALUES ('legacy-token', 'user', 50)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        drop(pool);

        let (code, out, err) = cli(tmp.path(), AdminCommand::RevokeSessions(None), "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(out, "revoked 1.\n");

        let pool = crate::db::connect(&url, tmp.path()).await.unwrap();
        let upgraded = crate::auth::session::upgrade_legacy(&pool, crate::db::now_ms())
            .await
            .unwrap();
        assert_eq!(upgraded, 0);
        let found = crate::auth::session::resolve(&pool, "legacy-token", crate::db::now_ms())
            .await
            .unwrap();
        assert!(found.is_none());
        let legacy: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'sessions_legacy'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(legacy, 0);
    }

    #[tokio::test]
    async fn invites_create_list_revoke_and_a_link_can_sign_up() {
        let tmp = TempDir::new().unwrap();
        let (code, out, err) = cli(
            tmp.path(),
            AdminCommand::Invites(InviteCommand::Create {
                days: 7,
                note: Some("a\nb\tc".into()),
            }),
            "",
        )
        .await;
        assert_eq!(code, 0, "{err}");
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out}");
        let id = lines[0].strip_prefix("id ").unwrap();
        assert_eq!(id.len(), 36);
        let expires = lines[1].strip_prefix("expires_at ").unwrap();
        assert!(expires.chars().all(|ch| ch.is_ascii_digit()));
        let link = lines[2];
        let token = link
            .strip_prefix("Link: <your server>/signup?invite=")
            .unwrap();
        assert_eq!(token.len(), 64);
        assert!(token
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
        assert_eq!(out.matches(token).count(), 1);
        assert!(!err.contains(token));

        let (code, listed, err) =
            cli(tmp.path(), AdminCommand::Invites(InviteCommand::List), "").await;
        assert_eq!(code, 0, "{err}");
        assert_eq!(listed.lines().count(), 1, "{listed}");
        assert!(listed.contains(id));
        assert!(listed.contains("active"));
        assert!(listed.contains("a\\nb\\tc"));
        assert!(!listed.contains(token));

        let (code, out, err) = cli(
            tmp.path(),
            AdminCommand::Invites(InviteCommand::Revoke("missing".into())),
            "",
        )
        .await;
        assert_eq!(code, 1, "{out}");
        assert_eq!(err, "No invite with id missing.\n");

        let url = format!("sqlite://{}?mode=rwc", tmp.path().join("mewo.db").display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let created_by: Option<String> =
            sqlx::query_scalar("SELECT created_by FROM invites WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(created_by.is_none());
        drop(pool);

        let app = build(config_at(tmp.path())).await.unwrap();
        let (status, body) = post_json(
            &app,
            "/v1/auth/register",
            json!({ "username": "ada", "password": "password1", "invite": token }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let session = body["token"].as_str().unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/me")
                    .header(header::AUTHORIZATION, format!("Bearer {session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let me: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(me["isAdmin"], false);
        assert_eq!(me["username"], "ada");
    }

    #[tokio::test]
    async fn password_prints_a_temporary_password_that_signs_in_once() {
        let tmp = TempDir::new().unwrap();
        let mut open = config_at(tmp.path());
        open.allow_signup = true;
        let app = build(open).await.unwrap();
        let (status, body) = post_json(
            &app,
            "/v1/auth/register",
            json!({ "username": "ada", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let ada = body["token"].as_str().unwrap().to_string();

        let (code, out, err) = cli(tmp.path(), AdminCommand::Password("Ada".into()), "").await;
        assert_eq!(code, 0, "{err}");
        let password = out
            .strip_prefix("Temporary password: ")
            .and_then(|rest| rest.strip_suffix('\n'))
            .unwrap_or_else(|| panic!("{out}"));
        assert_eq!(password.len(), 19, "{out}");
        assert_eq!(me_status(&app, &ada).await, StatusCode::UNAUTHORIZED);
        let (status, body) = post_json(
            &app,
            "/v1/auth/login",
            json!({ "username": "ada", "password": password }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, _) = post_json(
            &app,
            "/v1/auth/login",
            json!({ "username": "ada", "password": "password1" }),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (code, out, err) = cli(tmp.path(), AdminCommand::Password("nobody".into()), "").await;
        assert_eq!(code, 1, "{out}");
        assert_eq!(err, "No account named nobody.\n");
    }

    async fn me_status(app: &axum::Router, token: &str) -> StatusCode {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/me")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }
}
