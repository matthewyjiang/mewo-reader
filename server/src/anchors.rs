//! Align two parses of one book and rewrite every anchor onto the new indexes.
//!
//! A post id is its place in the feed. Removing a duplicate heading shifts
//! every later id, so comments, likes, reposts, and reading progress have to
//! move in the same transaction as the new posts.

use crate::error::ApiError;
use crate::feed::{Post, PostKind};
use similar::{capture_diff_slices, Algorithm, DiffOp};
use std::ops::DerefMut;

/// Key used to align two parses of the same text. `chapter` is excluded: a
/// relabelled heading must still align.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub(crate) struct PostKey {
    pub kind: PostKind,
    pub text: String,
}

impl From<&Post> for PostKey {
    fn from(post: &Post) -> Self {
        Self {
            kind: post.kind.clone(),
            text: post.text.clone(),
        }
    }
}

pub(crate) struct Remap {
    /// `new_of[old_idx]` for every old index. Identity means nothing moved.
    pub new_of: Vec<u32>,
    pub new_len: u32,
    /// Old posts that aligned exactly.
    pub matched: u32,
    /// Old posts with no counterpart, snapped to a neighbour.
    pub snapped: u32,
    exact: Vec<bool>,
}

impl Remap {
    #[cfg(test)]
    pub(crate) fn is_identity(&self) -> bool {
        self.new_len as usize == self.new_of.len()
            && self
                .new_of
                .iter()
                .enumerate()
                .all(|(index, new_index)| *new_index as usize == index)
    }

    #[cfg(test)]
    pub(crate) fn matched_ratio(&self) -> f32 {
        if self.new_of.is_empty() {
            1.0
        } else {
            self.matched as f32 / self.new_of.len() as f32
        }
    }

    /// `(removed, added)` heading posts. An exact match is neither. A duplicate
    /// that snapped onto its twin counts as removed.
    pub(crate) fn heading_delta(&self, old: &[PostKey], new: &[PostKey]) -> (u32, u32) {
        let mut covered = vec![false; new.len()];
        let mut removed = 0u32;
        for (index, post) in old.iter().enumerate() {
            if post.kind != PostKind::Heading {
                continue;
            }
            if self.exact.get(index).copied().unwrap_or(false) {
                if let Some(slot) = covered.get_mut(self.new_of[index] as usize) {
                    *slot = true;
                }
            } else {
                removed += 1;
            }
        }
        let added = new
            .iter()
            .enumerate()
            .filter(|(index, post)| {
                post.kind == PostKind::Heading && !covered.get(*index).copied().unwrap_or(false)
            })
            .count() as u32;
        (removed, added)
    }
}

/// Myers diff over [`PostKey`] slices.
///
/// Equal runs map 1:1. In a `Replace` hunk with equal-length sides, `old[i]`
/// maps to `new[i]` (a relabelled heading). Anything still unmatched snaps to
/// the surviving post with the same key when one exists — a diff may keep
/// either copy of a repeated heading — and otherwise to the new index of the
/// nearest preceding matched post, or 0 when the post is the first.
pub(crate) fn align(old: &[PostKey], new: &[PostKey]) -> Remap {
    let ops = capture_diff_slices(Algorithm::Myers, old, new);
    let mut assigned: Vec<Option<u32>> = vec![None; old.len()];
    let mut exact = vec![false; old.len()];
    let mut matched = 0u32;
    for op in ops {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                matched += len as u32;
                for offset in 0..len {
                    assigned[old_index + offset] = Some((new_index + offset) as u32);
                    exact[old_index + offset] = true;
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } if old_len == new_len => {
                for offset in 0..old_len {
                    assigned[old_index + offset] = Some((new_index + offset) as u32);
                }
            }
            _ => {}
        }
    }

    let last = new.len().saturating_sub(1) as u32;
    let mut new_of = Vec::with_capacity(old.len());
    let mut snapped = 0u32;
    let mut previous: Option<u32> = None;
    for index in 0..old.len() {
        if let Some(new_index) = assigned[index] {
            new_of.push(new_index);
            previous = Some(new_index);
            continue;
        }
        snapped += 1;
        let twin = if old[index].kind == PostKind::Heading {
            nearest_exact_same_key(old, &assigned, &exact, index)
        } else {
            None
        };
        let snap = twin.unwrap_or_else(|| previous.unwrap_or(0));
        let snap = if new.is_empty() { 0 } else { snap.min(last) };
        new_of.push(snap);
    }

    Remap {
        new_of,
        new_len: new.len() as u32,
        matched,
        snapped,
        exact,
    }
}

fn nearest_exact_same_key(
    old: &[PostKey],
    assigned: &[Option<u32>],
    exact: &[bool],
    index: usize,
) -> Option<u32> {
    let key = &old[index];
    let mut best: Option<(usize, u32)> = None;
    for (other, is_exact) in exact.iter().enumerate() {
        if !*is_exact || &old[other] != key {
            continue;
        }
        let Some(new_index) = assigned[other] else {
            continue;
        };
        let distance = other.abs_diff(index);
        match best {
            Some((best_distance, _)) if distance >= best_distance => {}
            _ => best = Some((distance, new_index)),
        }
    }
    best.map(|(_, new_index)| new_index)
}

/// The caller's transaction must already be `BEGIN IMMEDIATE`.
pub(crate) async fn apply(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
    remap: &Remap,
) -> Result<(), ApiError> {
    // A pooled connection can outlive a rolled-back attempt. Drop a leftover
    // map before creating one, and again before commit.
    sqlx::query("DROP TABLE IF EXISTS anchor_map")
        .execute(tx.deref_mut())
        .await?;
    sqlx::query("CREATE TEMP TABLE anchor_map(old INTEGER PRIMARY KEY, new INTEGER NOT NULL)")
        .execute(tx.deref_mut())
        .await?;
    let mapped: Vec<(i64, i64)> = remap
        .new_of
        .iter()
        .enumerate()
        .filter(|(old, new_index)| **new_index as usize != *old)
        .map(|(old, new_index)| (old as i64, *new_index as i64))
        .collect();
    insert_map(tx, &mapped).await?;
    remap_comments(tx, book_id).await?;
    // UPDATE, not delete-and-insert. `reposts_private` is AFTER INSERT and
    // would recompute `private` from today's sharing.
    remap_keyed(tx, "likes", book_id).await?;
    remap_keyed(tx, "reposts", book_id).await?;
    remap_progress(tx, book_id, remap.new_len).await?;
    sqlx::query("DROP TABLE IF EXISTS anchor_map")
        .execute(tx.deref_mut())
        .await?;
    Ok(())
}

async fn insert_map(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    rows: &[(i64, i64)],
) -> Result<(), ApiError> {
    const BATCH: usize = 500;
    for chunk in rows.chunks(BATCH) {
        let mut sql = String::from("INSERT INTO anchor_map (old, new) VALUES ");
        for (offset, _) in chunk.iter().enumerate() {
            if offset > 0 {
                sql.push(',');
            }
            sql.push_str("(?,?)");
        }
        let mut query = sqlx::query(&sql);
        for (old, new_index) in chunk {
            query = query.bind(old).bind(new_index);
        }
        query.execute(tx.deref_mut()).await?;
    }
    Ok(())
}

async fn remap_comments(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE comments
         SET post_id = CAST(m.new AS TEXT)
         FROM anchor_map m
         WHERE comments.book_id = ?
           AND comments.post_id GLOB '[0-9]*'
           AND comments.post_id NOT GLOB '*[^0-9]*'
           AND CAST(comments.post_id AS INTEGER) = m.old",
    )
    .bind(book_id)
    .execute(tx.deref_mut())
    .await?;
    Ok(())
}

/// `likes` and `reposts` are keyed `(user_id, book_id, post_id)`. A renumber
/// can collide mid-statement (6 -> 5 while 5 stays) and two old anchors can
/// land on one post.
///
/// 1. Move rows whose id is in the map to `~<new>`. `UPDATE OR IGNORE` leaves
///    a row that would collide on the tilde form sitting on its old id.
/// 2. Delete those leftovers. An equal row already exists at the destination.
/// 3. Strip the tilde. `OR IGNORE` again: a row that never moved already
///    occupies the plain id (the duplicate heading's twin). The tilde row is
///    the one that lost, so it is deleted. The survivor was updated, not
///    inserted, and keeps its `private` flag.
async fn remap_keyed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    book_id: &str,
) -> Result<(), ApiError> {
    let stage = format!(
        "UPDATE OR IGNORE {table}
         SET post_id = '~' || m.new
         FROM anchor_map m
         WHERE {table}.book_id = ?
           AND {table}.post_id GLOB '[0-9]*'
           AND {table}.post_id NOT GLOB '*[^0-9]*'
           AND CAST({table}.post_id AS INTEGER) = m.old"
    );
    sqlx::query(&stage)
        .bind(book_id)
        .execute(tx.deref_mut())
        .await?;
    let drop_losers = format!(
        "DELETE FROM {table}
         WHERE book_id = ?
           AND post_id NOT LIKE '~%'
           AND post_id GLOB '[0-9]*'
           AND post_id NOT GLOB '*[^0-9]*'
           AND CAST(post_id AS INTEGER) IN (SELECT old FROM anchor_map)"
    );
    sqlx::query(&drop_losers)
        .bind(book_id)
        .execute(tx.deref_mut())
        .await?;
    let reveal = format!(
        "UPDATE OR IGNORE {table}
         SET post_id = substr(post_id, 2)
         WHERE book_id = ? AND post_id LIKE '~%'"
    );
    sqlx::query(&reveal)
        .bind(book_id)
        .execute(tx.deref_mut())
        .await?;
    let drop_tilde = format!("DELETE FROM {table} WHERE book_id = ? AND post_id LIKE '~%'");
    sqlx::query(&drop_tilde)
        .bind(book_id)
        .execute(tx.deref_mut())
        .await?;
    Ok(())
}

async fn remap_progress(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: &str,
    new_len: u32,
) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE book_progress
         SET progress_index = m.new
         FROM anchor_map m
         WHERE book_progress.book_id = ?
           AND book_progress.progress_index = m.old",
    )
    .bind(book_id)
    .execute(tx.deref_mut())
    .await?;
    let last = new_len.saturating_sub(1) as i64;
    sqlx::query(
        "UPDATE book_progress
         SET progress_index = ?
         WHERE book_id = ? AND progress_index > ?",
    )
    .bind(last)
    .bind(book_id)
    .bind(last)
    .execute(tx.deref_mut())
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(kind: PostKind, text: &str) -> PostKey {
        PostKey {
            kind,
            text: text.to_owned(),
        }
    }

    fn heading(text: &str) -> PostKey {
        key(PostKind::Heading, text)
    }

    fn body(text: &str) -> PostKey {
        key(PostKind::Body, text)
    }

    #[test]
    fn identical_sequences_are_identity() {
        let posts = [heading("Chapter 1"), body("Later")];
        let remap = align(&posts, &posts);
        assert!(remap.is_identity());
        assert_eq!(remap.matched, 2);
        assert_eq!(remap.snapped, 0);
        assert_eq!(remap.matched_ratio(), 1.0);
        assert_eq!(remap.heading_delta(&posts, &posts), (0, 0));
    }

    #[test]
    fn removed_duplicate_heading_shifts_later_ids_down_by_one() {
        let old = [heading("Chapter 1"), heading("Chapter 1"), body("Later")];
        let new = [heading("Chapter 1"), body("Later")];
        let remap = align(&old, &new);
        assert_eq!(remap.new_of, vec![0, 0, 1]);
        assert_eq!(remap.new_len, 2);
        assert_eq!(remap.heading_delta(&old, &new), (1, 0));
        assert_eq!(remap.new_of[2], 1);
    }

    #[test]
    fn reply_on_removed_duplicate_heading_snaps_to_surviving_heading() {
        let old = [
            heading("Chapter 1"),
            heading("Chapter 1"),
            body("Later"),
            heading("Heaven"),
            heading("Heaven"),
        ];
        let new = [heading("Chapter 1"), body("Later"), heading("Heaven")];
        let remap = align(&old, &new);
        assert_eq!(remap.new_of[0], 0);
        assert_eq!(remap.new_of[1], 0);
        assert_eq!(remap.new_of[2], 1);
        assert_eq!(remap.new_of[3], 2);
        assert_eq!(remap.new_of[4], 2);
    }

    #[test]
    fn relabelled_heading_in_equal_replace_hunk_maps_positionally() {
        let old = [body("before"), heading("Chapter 1"), body("after")];
        let new = [body("before"), heading("Chapter One"), body("after")];
        let remap = align(&old, &new);
        assert_eq!(remap.new_of, vec![0, 1, 2]);
        assert_eq!(remap.matched, 2);
        assert_eq!(remap.snapped, 0);
        assert_eq!(remap.heading_delta(&old, &new), (1, 1));
    }

    #[test]
    fn progress_snaps_to_preceding_and_clamps() {
        let old = [heading("Chapter"), body("one"), body("two")];
        let new = [heading("Chapter")];
        let remap = align(&old, &new);
        assert_eq!(remap.new_of, vec![0, 0, 0]);
        assert_eq!(remap.new_len, 1);
        assert_eq!(clamp_index(remap.new_of[2], remap.new_len), 0);
        assert_eq!(clamp_index(5, 1), 0);
        assert_eq!(clamp_index(5, 0), 0);
    }

    fn clamp_index(index: u32, new_len: u32) -> u32 {
        if new_len == 0 {
            0
        } else {
            index.min(new_len - 1)
        }
    }
}
