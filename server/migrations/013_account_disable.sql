-- An admin can turn an account off without deleting it. NULL means active.
-- A disabled account keeps its books, posts, and profile but cannot sign in.
ALTER TABLE users ADD COLUMN disabled_at INTEGER;
