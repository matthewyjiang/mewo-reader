//! One predicate for "can this account see this book?".
//!
//! The shelf, search, profiles, and the home feed use these fragments. Do not
//! copy the predicate into a query. The book alias is `b`. `BOOK_SELECT` binds
//! the viewer first (that account's reading place). `BOOK_VISIBLE_TO` binds the
//! viewer next.

/// Owned, shared by its owner, or the built-in sample.
pub(crate) const BOOK_VISIBLE_TO: &str = "(b.user_id = ? OR b.shared = 1 OR b.is_sample = 1)";

pub(crate) const BOOK_SELECT: &str = "
SELECT b.id, b.title, b.author, b.handle, b.imported_at, b.post_count,
       COALESCE(p.progress_index, 0) as progress_index,
       b.is_sample, b.shared, b.user_id as owner_id, owner.username as owner_username
FROM books b
JOIN users owner ON owner.id = b.user_id
LEFT JOIN book_progress p ON p.book_id = b.id AND p.user_id = ?
";
