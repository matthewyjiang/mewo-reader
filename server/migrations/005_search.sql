-- Posts are the searchable copy of posts.json. The file stays what
-- GET /books/{id}/posts returns. post_fts is an external-content index of
-- that table. Deleting a book cascades to posts, and the delete trigger
-- drops the FTS row. The cascade only fires that trigger when foreign keys
-- are on (they are, on every pooled connection).

CREATE TABLE posts (
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    kind TEXT NOT NULL,
    chapter TEXT NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (book_id, idx)
);

CREATE VIRTUAL TABLE post_fts USING fts5(
    text,
    chapter,
    content='posts',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);

-- Rows are replaced by deleting and inserting, so there is no update trigger.
CREATE TRIGGER posts_ai AFTER INSERT ON posts BEGIN
    INSERT INTO post_fts(rowid, text, chapter)
    VALUES (new.rowid, new.text, new.chapter);
END;

CREATE TRIGGER posts_ad AFTER DELETE ON posts BEGIN
    INSERT INTO post_fts(post_fts, rowid, text, chapter)
    VALUES ('delete', old.rowid, old.text, old.chapter);
END;

-- 0 until store_feed has written posts.json and the posts rows together.
-- Startup backfill selects this.
ALTER TABLE books ADD COLUMN posts_indexed INTEGER NOT NULL DEFAULT 0;
