//! One Open Library work, even after a merge redirect.
//!
//! The asked key is resolved before the write transaction: the alias table,
//! then the in-memory cache, then one paced catalog hop. A hop that fails
//! leaves the asked key in place. Inside the transaction the whole group is
//! one lookup, and a new leader is stored under the canonical key.

use crate::anchors::{self, PostKey};
use crate::db::now_ms;
use crate::error::ApiError;
use crate::feed::PostKind;
use crate::library;
use crate::AppState;
use sqlx::{Acquire, Connection, SqlitePool};
use std::collections::HashSet;
use std::io::Write;

/// How far a chain or a cycle is followed. Longer than this, the walk stops.
const HOP_CAP: i64 = 8;

pub(crate) struct WorkKeys {
    pub canonical: String,
    /// `all[0]` is [`Self::canonical`]. The rest are aliases, oldest hop first.
    pub all: Vec<String>,
}

/// Canonical key for `asked`, before the caller opens its write transaction.
/// A table hit does not call Open Library. A hop that errors returns `asked`.
pub(crate) async fn canonical(state: &AppState, asked: &str) -> String {
    if let Some(found) = stored_canonical(&state.pool, asked).await {
        return found;
    }
    if let Some(cached) = state.work_cache.cached_canonical(asked) {
        if cached == asked {
            return cached;
        }
        if super::clean_work_key(&cached).is_ok() {
            let _ = record(&state.pool, asked, &cached, now_ms()).await;
            return cached;
        }
    }
    match one_hop(state, asked).await {
        Some(next) if next != asked && super::clean_work_key(&next).is_ok() => {
            let _ = record(&state.pool, asked, &next, now_ms()).await;
            next
        }
        _ => asked.to_string(),
    }
}

async fn stored_canonical(pool: &SqlitePool, asked: &str) -> Option<String> {
    sqlx::query_scalar("SELECT canonical_key FROM work_aliases WHERE alias_key = ?")
        .bind(asked)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

/// One paced `GET /works/{id}.json`. `None` is "not a redirect" or "upstream failed".
async fn one_hop(state: &AppState, asked: &str) -> Option<String> {
    state.catalog_limit.pace().await;
    let base = crate::requests::http::base_url(&state.openlibrary_url).ok()?;
    let doc = state.upstream.work_document(&base, asked).await.ok()?;
    match doc {
        crate::requests::http::RawWork::Redirect(Some(next)) if next != asked => Some(next),
        crate::requests::http::RawWork::Body(value) => {
            super::openlibrary::json_redirect(&value).filter(|next| next != asked)
        }
        _ => None,
    }
}

/// Every key that names this work. Walks alias chains forward to the canonical
/// key, then back out to the aliases, each direction at most [`HOP_CAP`] steps.
pub(crate) async fn group<'a, E>(executor: E, start: &str) -> Result<WorkKeys, ApiError>
where
    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
{
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "WITH RECURSIVE
         forward(key, hop) AS (
             SELECT ?, 0
             UNION ALL
             SELECT wa.canonical_key, f.hop + 1
             FROM forward f
             JOIN work_aliases wa ON wa.alias_key = f.key
             WHERE f.hop < ?
         ),
         canon AS (
             SELECT key FROM forward ORDER BY hop DESC, key ASC LIMIT 1
         ),
         reverse(key, hop) AS (
             SELECT key, 0 FROM canon
             UNION ALL
             SELECT wa.alias_key, r.hop + 1
             FROM reverse r
             JOIN work_aliases wa ON wa.canonical_key = r.key
             WHERE r.hop < ?
         )
         SELECT key, hop FROM reverse ORDER BY hop ASC, key ASC",
    )
    .bind(start)
    .bind(HOP_CAP)
    .bind(HOP_CAP)
    .fetch_all(executor)
    .await?;
    let mut all = Vec::new();
    for (key, _) in rows {
        if !all.contains(&key) {
            all.push(key);
        }
    }
    if all.is_empty() {
        all.push(start.to_string());
    }
    let canonical = all[0].clone();
    Ok(WorkKeys { canonical, all })
}

/// Remember that `alias` redirects to `canonical`. A repeat of the same pair
/// does not touch the row. The check constraint rejects an alias of itself.
pub(crate) async fn record(
    pool: &SqlitePool,
    alias: &str,
    canonical: &str,
    now: i64,
) -> Result<(), ApiError> {
    if alias == canonical || alias.is_empty() || canonical.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO work_aliases (alias_key, canonical_key, resolved_at)
         VALUES (?, ?, ?)
         ON CONFLICT(alias_key) DO UPDATE SET
             canonical_key = excluded.canonical_key,
             resolved_at = excluded.resolved_at
         WHERE canonical_key <> excluded.canonical_key",
    )
    .bind(alias)
    .bind(canonical)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// `admin works aliases`. Records redirects, then prints each group that has
/// two downloaded books. `apply` moves holders onto the survivor and deletes
/// the other copies. Dry-run prints the same plan and writes nothing but aliases.
pub(crate) async fn consolidate(
    state: &AppState,
    apply: bool,
    out: &mut dyn Write,
) -> Result<(), ApiError> {
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT r.work_key FROM book_requests r
         JOIN books b ON b.id = r.book_id
         WHERE r.state = 'imported' AND r.book_id IS NOT NULL",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    for key in keys {
        let resolved = canonical(state, &key).await;
        let found = group(&state.pool, &resolved).await?;
        if seen.insert(found.canonical.clone()) {
            groups.push(found);
        }
    }
    groups.sort_by(|left, right| left.canonical.cmp(&right.canonical));
    for group in groups {
        let copies = held_copies(&state.pool, &group.all).await?;
        if copies.len() < 2 {
            continue;
        }
        if apply {
            let mut conn = state.pool.acquire().await?;
            let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
            let plan = plan_group(&mut tx, &group.canonical, &copies).await?;
            apply_plan(&mut tx, &plan).await?;
            tx.commit().await?;
            for dead in &plan.dropped {
                let dir = library::book_dir(&state.data_dir, &dead.files_owner, &dead.id);
                let _ = tokio::fs::remove_dir_all(dir).await;
            }
            write_plan(out, "merged", &plan)?;
        } else {
            let mut conn = state.pool.acquire().await?;
            let mut tx = conn.begin().await?;
            let plan = plan_group(&mut tx, &group.canonical, &copies).await?;
            tx.rollback().await?;
            write_plan(out, "dry-run", &plan)?;
        }
    }
    Ok(())
}

struct Copy {
    id: String,
    imported_at: i64,
    files_owner: String,
    holders: i64,
}

async fn held_copies(pool: &SqlitePool, keys: &[String]) -> Result<Vec<Copy>, ApiError> {
    let listed = serde_json::to_string(keys).unwrap_or_else(|_| "[]".into());
    let rows: Vec<(String, i64, String, i64)> = sqlx::query_as(
        "SELECT b.id, b.imported_at, COALESCE(b.files_user_id, b.user_id),
                (SELECT COUNT(DISTINCT h.user_id) FROM book_requests h
                 WHERE h.book_id = b.id AND h.state = 'imported')
         FROM books b
         WHERE b.id IN (
             SELECT r.book_id FROM book_requests r
             WHERE r.state = 'imported' AND r.book_id IS NOT NULL
               AND r.work_key IN (SELECT value FROM json_each(?))
         )
         ORDER BY b.imported_at ASC, b.id ASC",
    )
    .bind(&listed)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, imported_at, files_owner, holders)| Copy {
            id,
            imported_at,
            files_owner,
            holders,
        })
        .collect())
}

struct Dropped {
    id: String,
    files_owner: String,
    holders: i64,
}

struct Shift {
    kind: &'static str,
    who: String,
    post_id: String,
    user_id: String,
    book_id: String,
    dest_post: Option<i64>,
    reason: String,
}

struct Plan {
    canonical: String,
    keep_id: String,
    keep_holders: i64,
    dropped: Vec<Dropped>,
    shifts: Vec<Shift>,
}

fn survivor_index(copies: &[Copy]) -> usize {
    let mut best = 0;
    for (index, copy) in copies.iter().enumerate().skip(1) {
        let have = &copies[best];
        let better = copy.holders > have.holders
            || (copy.holders == have.holders && copy.imported_at < have.imported_at)
            || (copy.holders == have.holders
                && copy.imported_at == have.imported_at
                && copy.id < have.id);
        if better {
            best = index;
        }
    }
    best
}

async fn plan_group(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    canonical: &str,
    copies: &[Copy],
) -> Result<Plan, ApiError> {
    let keep_at = survivor_index(copies);
    let keep = &copies[keep_at];
    let survivor_posts = load_posts(tx, &keep.id).await?;
    let survivor_keys = post_keys(&survivor_posts);
    let mut shifts = Vec::new();
    let mut dropped = Vec::new();
    for (index, copy) in copies.iter().enumerate() {
        if index == keep_at {
            continue;
        }
        dropped.push(Dropped {
            id: copy.id.clone(),
            files_owner: copy.files_owner.clone(),
            holders: copy.holders,
        });
        let old_posts = load_posts(tx, &copy.id).await?;
        let remap = anchors::align(&post_keys(&old_posts), &survivor_keys);
        push_comments(tx, &mut shifts, copy, &old_posts, &survivor_posts, &remap).await?;
        let aligned = Aligned {
            old_posts: &old_posts,
            new_posts: &survivor_posts,
            remap: &remap,
        };
        push_keyed(
            tx,
            &mut shifts,
            "repost",
            "reposts",
            copy,
            &aligned,
            &keep.id,
        )
        .await?;
        push_keyed(tx, &mut shifts, "like", "likes", copy, &aligned, &keep.id).await?;
        push_progress(
            tx,
            &mut shifts,
            copy,
            &old_posts,
            &survivor_posts,
            &remap,
            &keep.id,
        )
        .await?;
    }
    dropped.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(Plan {
        canonical: canonical.to_string(),
        keep_id: keep.id.clone(),
        keep_holders: keep.holders,
        dropped,
        shifts,
    })
}

#[derive(sqlx::FromRow)]
struct StoredPost {
    idx: i64,
    kind: String,
    text: String,
}

async fn load_posts(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
) -> Result<Vec<StoredPost>, ApiError> {
    let rows = sqlx::query_as::<_, StoredPost>(
        "SELECT idx, kind, text FROM posts WHERE book_id = ? ORDER BY idx ASC",
    )
    .bind(book_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

fn post_keys(rows: &[StoredPost]) -> Vec<PostKey> {
    let Some(max) = rows.iter().map(|row| row.idx).max() else {
        return Vec::new();
    };
    if max < 0 {
        return Vec::new();
    }
    let mut keys = Vec::with_capacity(max as usize + 1);
    for index in 0..=max {
        let real = rows.iter().find(|row| row.idx == index);
        keys.push(match real {
            Some(row) => PostKey {
                kind: if row.kind == "Heading" {
                    PostKind::Heading
                } else {
                    PostKind::Body
                },
                text: row.text.clone(),
            },
            None => PostKey {
                kind: PostKind::Body,
                text: format!("\u{0}missing-{index}"),
            },
        });
    }
    keys
}

struct Aligned<'a> {
    old_posts: &'a [StoredPost],
    new_posts: &'a [StoredPost],
    remap: &'a anchors::Remap,
}

fn has_post(rows: &[StoredPost], idx: i64) -> bool {
    rows.iter().any(|row| row.idx == idx)
}

fn destination(
    remap: &anchors::Remap,
    old_posts: &[StoredPost],
    new_posts: &[StoredPost],
    post_id: &str,
) -> Option<i64> {
    let idx: usize = post_id.parse().ok()?;
    let dest = remap.anchored(idx)?;
    if has_post(old_posts, idx as i64) && has_post(new_posts, i64::from(dest)) {
        Some(i64::from(dest))
    } else {
        None
    }
}

async fn push_comments(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    shifts: &mut Vec<Shift>,
    copy: &Copy,
    old_posts: &[StoredPost],
    new_posts: &[StoredPost],
    remap: &anchors::Remap,
) -> Result<(), ApiError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, user_id, post_id FROM comments WHERE book_id = ? ORDER BY id ASC -- own rows only",
    )
    .bind(&copy.id)
    .fetch_all(&mut **tx)
    .await?;
    for (id, user_id, post_id) in rows {
        let dest = destination(remap, old_posts, new_posts, &post_id);
        let reason = if dest.is_some() {
            String::new()
        } else {
            "no anchor".into()
        };
        shifts.push(Shift {
            kind: "reply",
            who: id,
            post_id,
            user_id,
            book_id: copy.id.clone(),
            dest_post: dest,
            reason,
        });
    }
    Ok(())
}

async fn occupied(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    survivor: &str,
    user_id: &str,
    post_id: &str,
) -> Result<bool, ApiError> {
    let sql =
        format!("SELECT 1 FROM {table} WHERE book_id = ? AND user_id = ? AND post_id = ? LIMIT 1 -- own rows only");
    let hit: Option<i64> = sqlx::query_scalar(&sql)
        .bind(survivor)
        .bind(user_id)
        .bind(post_id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(hit.is_some())
}

async fn push_keyed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    shifts: &mut Vec<Shift>,
    kind: &'static str,
    table: &str,
    copy: &Copy,
    aligned: &Aligned<'_>,
    survivor: &str,
) -> Result<(), ApiError> {
    let sql = format!(
        "SELECT user_id, post_id FROM {table} WHERE book_id = ? ORDER BY user_id ASC, post_id ASC -- own rows only"
    );
    let rows: Vec<(String, String)> = sqlx::query_as(&sql)
        .bind(&copy.id)
        .fetch_all(&mut **tx)
        .await?;
    for (user_id, post_id) in rows {
        let dest = destination(
            aligned.remap,
            aligned.old_posts,
            aligned.new_posts,
            &post_id,
        );
        let (dest_post, reason) = match dest {
            Some(dest) => {
                let landed = dest.to_string();
                if occupied(tx, table, survivor, &user_id, &landed).await? {
                    (None, format!("already on {survivor}"))
                } else {
                    (Some(dest), String::new())
                }
            }
            None => (None, "no anchor".to_string()),
        };
        shifts.push(Shift {
            kind,
            who: user_id.clone(),
            post_id,
            user_id,
            book_id: copy.id.clone(),
            dest_post,
            reason,
        });
    }
    Ok(())
}

async fn push_progress(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    shifts: &mut Vec<Shift>,
    copy: &Copy,
    old_posts: &[StoredPost],
    new_posts: &[StoredPost],
    remap: &anchors::Remap,
    survivor: &str,
) -> Result<(), ApiError> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT user_id, progress_index FROM book_progress WHERE book_id = ? ORDER BY user_id ASC",
    )
    .bind(&copy.id)
    .fetch_all(&mut **tx)
    .await?;
    for (user_id, index) in rows {
        let post_id = index.to_string();
        let dest = destination(remap, old_posts, new_posts, &post_id);
        let (dest_post, reason) = match dest {
            Some(dest) => {
                let taken: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM book_progress WHERE book_id = ? AND user_id = ? LIMIT 1",
                )
                .bind(survivor)
                .bind(&user_id)
                .fetch_optional(&mut **tx)
                .await?;
                if taken.is_some() {
                    (None, format!("already on {survivor}"))
                } else {
                    (Some(dest), String::new())
                }
            }
            None => (None, "no anchor".to_string()),
        };
        shifts.push(Shift {
            kind: "progress",
            who: user_id.clone(),
            post_id,
            user_id,
            book_id: copy.id.clone(),
            dest_post,
            reason,
        });
    }
    Ok(())
}

async fn apply_plan(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &Plan,
) -> Result<(), ApiError> {
    let now = now_ms();
    for shift in &plan.shifts {
        match (shift.kind, shift.dest_post) {
            ("reply", Some(dest)) => {
                sqlx::query(
                    "UPDATE comments SET book_id = ?, post_id = ? WHERE id = ? AND book_id = ?",
                )
                .bind(&plan.keep_id)
                .bind(dest.to_string())
                .bind(&shift.who)
                .bind(&shift.book_id)
                .execute(&mut **tx)
                .await?;
            }
            ("reply", None) => {
                sqlx::query("DELETE FROM comments WHERE id = ? AND book_id = ? -- own rows only")
                    .bind(&shift.who)
                    .bind(&shift.book_id)
                    .execute(&mut **tx)
                    .await?;
            }
            ("repost", Some(dest)) => {
                move_keyed(tx, "reposts", &plan.keep_id, shift, dest).await?;
            }
            ("like", Some(dest)) => {
                move_keyed(tx, "likes", &plan.keep_id, shift, dest).await?;
            }
            ("repost" | "like", None) => {
                let table = if shift.kind == "repost" {
                    "reposts"
                } else {
                    "likes"
                };
                let sql = format!(
                    "DELETE FROM {table} WHERE book_id = ? AND user_id = ? AND post_id = ? -- own rows only"
                );
                sqlx::query(&sql)
                    .bind(&shift.book_id)
                    .bind(&shift.user_id)
                    .bind(&shift.post_id)
                    .execute(&mut **tx)
                    .await?;
            }
            ("progress", Some(dest)) => {
                sqlx::query(
                    "UPDATE book_progress SET book_id = ?, progress_index = ?
                     WHERE book_id = ? AND user_id = ?",
                )
                .bind(&plan.keep_id)
                .bind(dest)
                .bind(&shift.book_id)
                .bind(&shift.user_id)
                .execute(&mut **tx)
                .await?;
            }
            ("progress", None) => {
                sqlx::query("DELETE FROM book_progress WHERE book_id = ? AND user_id = ?")
                    .bind(&shift.book_id)
                    .bind(&shift.user_id)
                    .execute(&mut **tx)
                    .await?;
            }
            _ => {}
        }
    }
    for dead in &plan.dropped {
        let holders: Vec<String> = sqlx::query_scalar(
            "SELECT user_id FROM book_requests
             WHERE book_id = ? AND state = 'imported'",
        )
        .bind(&plan.keep_id)
        .fetch_all(&mut **tx)
        .await?;
        for user_id in holders {
            sqlx::query(
                "UPDATE book_requests
                 SET state = 'canceled', book_id = NULL, error = NULL, download_id = NULL,
                     updated_at = ?, next_check_at = ?
                 WHERE book_id = ? AND user_id = ? AND state = 'imported'",
            )
            .bind(now)
            .bind(now)
            .bind(&dead.id)
            .bind(&user_id)
            .execute(&mut **tx)
            .await?;
        }
        sqlx::query(
            "UPDATE book_requests SET book_id = ? WHERE book_id = ? AND state = 'imported'",
        )
        .bind(&plan.keep_id)
        .bind(&dead.id)
        .execute(&mut **tx)
        .await?;
        sqlx::query("DELETE FROM books WHERE id = ?")
            .bind(&dead.id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Move one like or repost onto the survivor. `UPDATE`, so `private` stays
/// as it was written. A row that cannot land is deleted instead of inserted.
async fn move_keyed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    survivor: &str,
    shift: &Shift,
    dest: i64,
) -> Result<(), ApiError> {
    let sql = format!(
        "UPDATE OR IGNORE {table} SET book_id = ?, post_id = ?
         WHERE book_id = ? AND user_id = ? AND post_id = ?"
    );
    let changed = sqlx::query(&sql)
        .bind(survivor)
        .bind(dest.to_string())
        .bind(&shift.book_id)
        .bind(&shift.user_id)
        .bind(&shift.post_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if changed == 0 {
        let drop_sql =
            format!("DELETE FROM {table} WHERE book_id = ? AND user_id = ? AND post_id = ? -- own rows only");
        sqlx::query(&drop_sql)
            .bind(&shift.book_id)
            .bind(&shift.user_id)
            .bind(&shift.post_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

fn write_plan(out: &mut dyn Write, verb: &str, plan: &Plan) -> Result<(), ApiError> {
    writeln!(out, "{verb} {}", plan.canonical).map_err(io_err)?;
    writeln!(out, "keep {} ({} holders)", plan.keep_id, plan.keep_holders).map_err(io_err)?;
    for dead in &plan.dropped {
        writeln!(out, "drop {} ({} holders)", dead.id, dead.holders).map_err(io_err)?;
    }
    for (kind, label) in [
        ("reply", "replies"),
        ("repost", "reposts"),
        ("like", "likes"),
        ("progress", "progress"),
    ] {
        let moved = plan
            .shifts
            .iter()
            .filter(|shift| shift.kind == kind && shift.dest_post.is_some())
            .count();
        let lost = plan
            .shifts
            .iter()
            .filter(|shift| shift.kind == kind && shift.dest_post.is_none())
            .count();
        writeln!(out, "{label} move {moved} lost {lost}").map_err(io_err)?;
    }
    let mut lost: Vec<&Shift> = plan
        .shifts
        .iter()
        .filter(|shift| shift.dest_post.is_none())
        .collect();
    lost.sort_by(|left, right| {
        left.kind
            .cmp(right.kind)
            .then(left.who.cmp(&right.who))
            .then(left.post_id.cmp(&right.post_id))
    });
    for shift in lost {
        writeln!(
            out,
            "lost {} {} post {} ({})",
            shift.kind, shift.who, shift.post_id, shift.reason
        )
        .map_err(io_err)?;
    }
    Ok(())
}

fn io_err(err: std::io::Error) -> ApiError {
    tracing::error!(error = %err, "could not print work aliases");
    ApiError::Internal
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, Config, LimitsConfig};
    use axum::body::Body;
    use axum::extract::Path;
    use axum::http::{header, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn config(dir: &std::path::Path, openlibrary_url: &str) -> Config {
        Config {
            listen: "127.0.0.1:0".into(),
            data_dir: dir.to_path_buf(),
            web_dir: None,
            allow_signup: true,
            max_epub_bytes: 64 * 1024 * 1024,
            downloads_dir: None,
            openlibrary_url: openlibrary_url.to_string(),
            auth: AuthConfig::default(),
            limits: LimitsConfig {
                min_free_bytes: 0,
                ..LimitsConfig::default()
            },
        }
    }

    struct Script {
        hits: u32,
        status: u16,
        body: String,
        redirect: Option<String>,
    }

    async fn serve(script: Arc<Mutex<Script>>) -> String {
        let app = Router::new()
            .route("/works/{id}", get(work))
            .with_state(script);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        origin
    }

    async fn work(
        axum::extract::State(script): axum::extract::State<Arc<Mutex<Script>>>,
        Path(_id): Path<String>,
    ) -> axum::response::Response {
        let mut script = script.lock().await;
        script.hits += 1;
        if let Some(location) = &script.redirect {
            return axum::response::Response::builder()
                .status(StatusCode::FOUND)
                .header(header::LOCATION, location.clone())
                .body(Body::empty())
                .unwrap();
        }
        if script.status != 200 {
            return axum::response::Response::builder()
                .status(StatusCode::from_u16(script.status).unwrap())
                .body(Body::empty())
                .unwrap();
        }
        axum::response::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(script.body.clone()))
            .unwrap()
    }

    async fn alias_of(pool: &SqlitePool, key: &str) -> Option<String> {
        sqlx::query_scalar("SELECT canonical_key FROM work_aliases WHERE alias_key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_redirect_is_recorded_and_a_second_call_does_not_ask_again() {
        let script = Arc::new(Mutex::new(Script {
            hits: 0,
            status: 200,
            body: r#"{"type":{"key":"/type/redirect"},"location":"/works/OL2W"}"#.into(),
            redirect: None,
        }));
        let origin = serve(Arc::clone(&script)).await;
        let dir = tempfile::tempdir().unwrap();
        let state = crate::prepare(config(dir.path(), &origin)).await.unwrap();
        let got = canonical(&state, "/works/OL1W").await;
        assert_eq!(got, "/works/OL2W");
        assert_eq!(
            alias_of(&state.pool, "/works/OL1W").await.as_deref(),
            Some("/works/OL2W")
        );
        let again = canonical(&state, "/works/OL1W").await;
        assert_eq!(again, "/works/OL2W");
        assert_eq!(script.lock().await.hits, 1);

        script.lock().await.redirect = Some("/works/OL4W.json".into());
        script.lock().await.body.clear();
        let http = canonical(&state, "/works/OL3W").await;
        assert_eq!(http, "/works/OL4W");
        assert_eq!(
            alias_of(&state.pool, "/works/OL3W").await.as_deref(),
            Some("/works/OL4W")
        );
    }

    #[tokio::test]
    async fn a_real_work_and_an_upstream_error_leave_the_asked_key() {
        let script = Arc::new(Mutex::new(Script {
            hits: 0,
            status: 200,
            body: r#"{"title":"Dune","type":{"key":"/type/work"}}"#.into(),
            redirect: None,
        }));
        let origin = serve(Arc::clone(&script)).await;
        let dir = tempfile::tempdir().unwrap();
        let state = crate::prepare(config(dir.path(), &origin)).await.unwrap();
        assert_eq!(canonical(&state, "/works/OL1W").await, "/works/OL1W");
        assert!(alias_of(&state.pool, "/works/OL1W").await.is_none());

        script.lock().await.status = 500;
        assert_eq!(canonical(&state, "/works/OL9W").await, "/works/OL9W");
        assert!(alias_of(&state.pool, "/works/OL9W").await.is_none());
    }

    #[tokio::test]
    async fn a_table_hit_does_not_call_upstream() {
        let script = Arc::new(Mutex::new(Script {
            hits: 0,
            status: 500,
            body: String::new(),
            redirect: None,
        }));
        let origin = serve(Arc::clone(&script)).await;
        let dir = tempfile::tempdir().unwrap();
        let state = crate::prepare(config(dir.path(), &origin)).await.unwrap();
        record(&state.pool, "/works/OL1W", "/works/OL2W", 1)
            .await
            .unwrap();
        assert_eq!(canonical(&state, "/works/OL1W").await, "/works/OL2W");
        assert_eq!(script.lock().await.hits, 0);
    }

    async fn link(pool: &SqlitePool, alias: &str, canonical: &str) {
        record(pool, alias, canonical, 1).await.unwrap();
    }

    #[tokio::test]
    async fn group_walks_a_chain_in_both_directions_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::prepare(config(dir.path(), "http://127.0.0.1:9"))
            .await
            .unwrap();
        link(&state.pool, "/works/A", "/works/B").await;
        link(&state.pool, "/works/B", "/works/C").await;
        let from_alias = group(&state.pool, "/works/A").await.unwrap();
        assert_eq!(from_alias.canonical, "/works/C");
        assert_eq!(from_alias.all[0], "/works/C");
        assert!(from_alias.all.contains(&"/works/A".to_string()));
        assert!(from_alias.all.contains(&"/works/B".to_string()));
        let from_canon = group(&state.pool, "/works/C").await.unwrap();
        assert_eq!(from_canon.canonical, "/works/C");
        assert!(from_canon.all.contains(&"/works/A".to_string()));
        assert!(from_canon.all.contains(&"/works/B".to_string()));

        for n in 0..12 {
            let alias = format!("/works/K{n}");
            let next = format!("/works/K{}", n + 1);
            link(&state.pool, &alias, &next).await;
        }
        let capped = group(&state.pool, "/works/K0").await.unwrap();
        assert_eq!(capped.canonical, "/works/K8");
        assert!(capped.all.contains(&"/works/K0".to_string()));
        assert!(!capped.all.iter().any(|key| key == "/works/K10"));
        assert!(!capped.all.iter().any(|key| key == "/works/K12"));

        link(&state.pool, "/works/X", "/works/Y").await;
        link(&state.pool, "/works/Y", "/works/X").await;
        let cycled = group(&state.pool, "/works/X").await.unwrap();
        assert!(cycled.all.len() <= (HOP_CAP as usize) + 1);
        assert!(cycled.all.contains(&"/works/X".to_string()));
        assert!(cycled.all.contains(&"/works/Y".to_string()));
    }
}
