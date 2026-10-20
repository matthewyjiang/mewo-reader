-- A reader asked the server to find and import a book from the catalog.
-- One row per (user, work) attempt. The worker in requests/worker.rs is the
-- only writer of state, download_id, tried_guids, attempts, error, next_check_at.
CREATE TABLE book_requests (
    id TEXT PRIMARY KEY NOT NULL,                  -- also the imported book's id
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_key TEXT NOT NULL,                        -- "/works/OL893415W"
    title TEXT NOT NULL,
    author TEXT NOT NULL,
    year INTEGER,
    cover_id INTEGER,
    state TEXT NOT NULL CHECK (state IN ('requested','downloading','imported','failed','canceled')),
    release_title TEXT,                            -- the release we chose, for the admin list
    release_size INTEGER,
    indexer TEXT,
    download_id TEXT,                              -- SABnzbd nzo_id
    progress INTEGER NOT NULL DEFAULT 0,           -- 0..100 while downloading
    tried_guids TEXT NOT NULL DEFAULT '[]',        -- JSON array, so a retry picks the next release
    attempts INTEGER NOT NULL DEFAULT 0,           -- grabs made (max 3)
    error TEXT,
    book_id TEXT REFERENCES books(id) ON DELETE SET NULL,
    next_check_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- One live request per reader per work. Imported, failed and canceled rows
-- don't block a new one.
CREATE UNIQUE INDEX book_requests_one_active
    ON book_requests (user_id, work_key) WHERE state IN ('requested','downloading');
CREATE INDEX book_requests_due
    ON book_requests (next_check_at) WHERE state IN ('requested','downloading');

-- Write-only credentials. Never part of the Settings doc, so GET can't leak them.
ALTER TABLE settings ADD COLUMN secrets TEXT NOT NULL DEFAULT '{}';
