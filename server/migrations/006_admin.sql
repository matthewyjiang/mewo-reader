-- Admins configure the server from the web app. The first one is granted with
-- `mewo-server admin grant <username>`.
ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0;

-- One row. `doc` is the JSON of `settings::Settings`. A key missing from doc
-- takes that field's default, so a new setting needs no migration.
CREATE TABLE settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    doc TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL
);
