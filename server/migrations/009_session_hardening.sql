-- Sessions store sha256(token), not the bearer token. SQLite has no SHA-256,
-- so auth::session::upgrade_legacy hashes rows left in sessions_legacy.
ALTER TABLE sessions RENAME TO sessions_legacy;

CREATE TABLE sessions (
    id           TEXT PRIMARY KEY NOT NULL,
    token_hash   BLOB NOT NULL UNIQUE,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL
);

CREATE INDEX sessions_user ON sessions(user_id);
CREATE INDEX sessions_expires ON sessions(expires_at);
