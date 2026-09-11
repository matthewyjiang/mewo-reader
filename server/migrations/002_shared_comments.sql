-- Hosted books are one shelf. Reading place is per person so two
-- accounts do not share a bookmark. Comments sit on the shared book.

CREATE TABLE book_progress (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    progress_index INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, book_id)
);

INSERT INTO book_progress (user_id, book_id, progress_index)
SELECT user_id, id, progress_index FROM books;

CREATE TABLE comments (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id TEXT NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    post_id TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX comments_book_post ON comments (book_id, post_id, created_at);

-- One sample for the server. Drop the per-user unique index.
DROP INDEX IF EXISTS books_one_sample;
