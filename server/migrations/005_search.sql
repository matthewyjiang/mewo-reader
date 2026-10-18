-- Posts are the searchable copy of posts.json. The file stays what
-- GET /books/{id}/posts returns. post_fts is an external-content index of
-- that table. Deleting a book cascades to posts, and the delete trigger
-- drops the FTS row. The cascade only fires that trigger when foreign keys
-- are on (they are, on every pooled connection).
--
-- `id` is the rowid. VACUUM does not renumber an INTEGER PRIMARY KEY, so the
-- FTS content_rowid stays pointed at the same post after a dump or restore.

CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    kind TEXT NOT NULL,
    chapter TEXT NOT NULL,
    text TEXT NOT NULL,
    UNIQUE (book_id, idx)
);

CREATE VIRTUAL TABLE post_fts USING fts5(
    text,
    chapter,
    content='posts',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

-- Rows are replaced by deleting and inserting, so there is no update trigger.
CREATE TRIGGER posts_ai AFTER INSERT ON posts BEGIN
    INSERT INTO post_fts(rowid, text, chapter)
    VALUES (new.id, new.text, new.chapter);
END;

CREATE TRIGGER posts_ad AFTER DELETE ON posts BEGIN
    INSERT INTO post_fts(post_fts, rowid, text, chapter)
    VALUES ('delete', old.id, old.text, old.chapter);
END;

-- 0 until store_feed has written posts.json and the posts rows together.
-- -1 when the EPUB could not be read and there is no posts.json.
-- Startup backfill selects 0.
ALTER TABLE books ADD COLUMN posts_indexed INTEGER NOT NULL DEFAULT 0;

-- Home feed walks comments newest first. The per-user index does not cover that.
CREATE INDEX comments_created ON comments (created_at DESC);

-- One row per account per post. Deleting the account or the book drops it.
CREATE TABLE reposts (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    post_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, book_id, post_id)
);

CREATE INDEX reposts_user ON reposts (user_id, created_at DESC);
CREATE INDEX reposts_created ON reposts (created_at DESC);
