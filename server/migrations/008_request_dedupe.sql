-- One file per Open Library work. A second request reuses the imported
-- book or follows the single in-flight leader. Holders of a private copy
-- do not see each other's replies or reposts.

ALTER TABLE book_requests ADD COLUMN follows TEXT REFERENCES book_requests(id) ON DELETE SET NULL;

-- Two users can both be active leaders today. Keep the oldest; the rest
-- follow it. state becomes requested so the leader's import update, which
-- only touches followers in that state, picks up a row that was downloading.
-- The worker deletes the demoted row's SABnzbd job.
WITH first AS (
    SELECT work_key, MIN(created_at || ':' || id) AS k
    FROM book_requests
    WHERE state IN ('requested', 'downloading')
    GROUP BY work_key
)
UPDATE book_requests SET
    follows = (
        SELECT o.id FROM book_requests o
        JOIN first f ON f.work_key = o.work_key
        WHERE o.work_key = book_requests.work_key
          AND o.created_at || ':' || o.id = f.k
    ),
    state = 'requested'
WHERE state IN ('requested', 'downloading')
  AND created_at || ':' || id <> (
      SELECT k FROM first WHERE first.work_key = book_requests.work_key
  );

CREATE UNIQUE INDEX book_requests_one_leader
    ON book_requests (work_key)
    WHERE state IN ('requested', 'downloading') AND follows IS NULL;

DROP INDEX book_requests_due;
CREATE INDEX book_requests_due
    ON book_requests (next_check_at)
    WHERE state IN ('requested', 'downloading') AND follows IS NULL;

CREATE INDEX book_requests_work ON book_requests (work_key, state);
CREATE INDEX book_requests_follows ON book_requests (follows) WHERE follows IS NOT NULL;
CREATE INDEX book_requests_holder ON book_requests (user_id, book_id) WHERE state = 'imported';

-- Owners plus accounts whose imported request points at the book.
-- BOOK_VISIBLE_TO binds the viewer once against this view.
CREATE VIEW book_holders AS
    SELECT b.id AS book_id, b.user_id FROM books b
    UNION ALL
    SELECT r.book_id, r.user_id FROM book_requests r
    WHERE r.state = 'imported' AND r.book_id IS NOT NULL;

ALTER TABLE comments ADD COLUMN private INTEGER NOT NULL DEFAULT 0;
ALTER TABLE reposts ADD COLUMN private INTEGER NOT NULL DEFAULT 0;

-- The flag is the sharing state at insert. A later share does not clear it,
-- so a reply written while the book was private stays private.
CREATE TRIGGER comments_private AFTER INSERT ON comments BEGIN
    UPDATE comments SET private = (
        SELECT b.user_id <> NEW.user_id AND b.shared = 0 AND b.is_sample = 0
        FROM books b WHERE b.id = NEW.book_id)
    WHERE id = NEW.id;
END;

CREATE TRIGGER reposts_private AFTER INSERT ON reposts BEGIN
    UPDATE reposts SET private = (
        SELECT b.user_id <> NEW.user_id AND b.shared = 0 AND b.is_sample = 0
        FROM books b WHERE b.id = NEW.book_id)
    WHERE user_id = NEW.user_id AND book_id = NEW.book_id AND post_id = NEW.post_id;
END;

-- NULL means the file still lives under user_id. An ownership transfer
-- sets this so the path does not move.
ALTER TABLE books ADD COLUMN files_user_id TEXT;
