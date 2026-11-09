-- A reported release is never grabbed again. guid is the indexer id.
-- title_norm + size catches the same file posted under a new guid.
CREATE TABLE release_blocklist (
    id TEXT PRIMARY KEY NOT NULL,
    guid TEXT NOT NULL,
    indexer TEXT NOT NULL,
    title TEXT NOT NULL,
    title_norm TEXT NOT NULL,
    size INTEGER NOT NULL,
    work_key TEXT,
    book_title TEXT,
    book_author TEXT,
    blocked_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX release_blocklist_guid ON release_blocklist (guid);
CREATE INDEX release_blocklist_shape ON release_blocklist (title_norm, size);

-- Open Library merged X into Y. alias_key is the dead key. Requests and books
-- for either key are one work. A chain (A→B, B→C) is walked at read time.
CREATE TABLE work_aliases (
    alias_key TEXT PRIMARY KEY NOT NULL,
    canonical_key TEXT NOT NULL,
    resolved_at INTEGER NOT NULL,
    CHECK (alias_key <> canonical_key)
);
CREATE INDEX work_aliases_canonical ON work_aliases (canonical_key);

-- Set when the owner replaces book.epub. A replaced file is not the release
-- the request imported, so it cannot be reported as the wrong file.
ALTER TABLE books ADD COLUMN replaced_at INTEGER;

-- Wrong file starts a new download of the same work. The new book keeps the
-- sharing of the copy that was removed. 0 is a normal request.
ALTER TABLE book_requests ADD COLUMN want_shared INTEGER NOT NULL DEFAULT 0;
