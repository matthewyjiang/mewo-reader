-- Profile lists every reply by one account. Newest first.
CREATE INDEX comments_user ON comments (user_id, created_at DESC);
