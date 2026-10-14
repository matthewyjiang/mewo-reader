-- Books start private. Rows already on the shelf stay readable by every account.
ALTER TABLE books ADD COLUMN shared INTEGER NOT NULL DEFAULT 0;

UPDATE books SET shared = 1;

CREATE INDEX books_shared_owner ON books (shared, user_id);
