CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL,
    username_norm TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE sessions (
    token TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE TABLE books (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    author TEXT NOT NULL,
    handle TEXT NOT NULL,
    imported_at INTEGER NOT NULL,
    post_count INTEGER NOT NULL DEFAULT 0,
    progress_index INTEGER NOT NULL DEFAULT 0,
    is_sample INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX books_one_sample ON books(user_id) WHERE is_sample = 1;

CREATE TABLE likes (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    post_id TEXT NOT NULL,
    PRIMARY KEY (user_id, book_id, post_id)
);
