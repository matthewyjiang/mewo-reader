-- Parser output revision this book's posts were built with. 0 = built before revisions existed.
ALTER TABLE books ADD COLUMN parser_rev INTEGER NOT NULL DEFAULT 0;
-- Bumped whenever a reparse or replacement renumbers posts. Clients drop per-book caches when it changes.
ALTER TABLE books ADD COLUMN feed_rev INTEGER NOT NULL DEFAULT 0;
