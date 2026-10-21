//! One predicate for "can this account see this book?".
//!
//! The shelf, search, profiles, and the home feed use these fragments. Do not
//! copy the predicate into a query. The book alias is `b`. `BOOK_SELECT` binds
//! the viewer first (that account's reading place). `BOOK_VISIBLE_TO` binds the
//! viewer next. `social_visible` binds the viewer once more, on a comment or
//! repost row that is joined to that book.

/// Shared, the sample, or a book this account owns or holds.
/// The subquery is not correlated: SQLite builds the viewer's holder set once.
pub(crate) const BOOK_VISIBLE_TO: &str =
    "(b.shared = 1 OR b.is_sample = 1 OR b.id IN (SELECT book_id FROM book_holders WHERE user_id = ?))";

/// A comment or repost row `alias` on book `b` (which must be joined).
/// Your own rows always show. Others' rows show only while the book is
/// deliberately shared (or the sample) and the row was not written privately.
pub(crate) fn social_visible(alias: &str) -> String {
    format!("({alias}.user_id = ? OR ((b.shared = 1 OR b.is_sample = 1) AND {alias}.private = 0))")
}

pub(crate) const BOOK_SELECT: &str = "
SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
       COALESCE(p.progress_index, 0) as progress_index,
       b.is_sample, b.shared, b.user_id as owner_id, owner.username as owner_username,
       COALESCE(b.files_user_id, b.user_id) AS files_owner
FROM books b
JOIN users owner ON owner.id = b.user_id
LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
";
