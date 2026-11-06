-- One-time signup links. Only sha256(token) is stored; the link is shown once.
-- A redeemed invite keeps its row so the admin list shows who used it.
CREATE TABLE invites (
    id              TEXT PRIMARY KEY NOT NULL,
    token_hash      BLOB NOT NULL UNIQUE,
    note            TEXT,
    created_by      TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at      INTEGER NOT NULL,
    expires_at      INTEGER NOT NULL,
    used_at         INTEGER,
    used_by_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    revoked_at      INTEGER,
    CHECK (used_at IS NULL OR revoked_at IS NULL)
);
CREATE INDEX invites_created ON invites(created_at);
