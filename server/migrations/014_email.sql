-- Optional account email for password reset. Only verified addresses live in
-- users.email. A pending address waits in its verify row until the link is
-- opened. email_norm is the ASCII-lowercased address.
ALTER TABLE users ADD COLUMN email TEXT;
ALTER TABLE users ADD COLUMN email_norm TEXT;
CREATE UNIQUE INDEX users_email_norm ON users(email_norm) WHERE email_norm IS NOT NULL;

-- One live link per account and purpose. Only sha256(token) is stored.
-- password_fp is sha256(users.password_hash) when a reset link was issued, so
-- any password change kills the link.
CREATE TABLE email_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK (purpose IN ('verify', 'reset')),
    email TEXT NOT NULL,
    email_norm TEXT NOT NULL,
    password_fp BLOB,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    UNIQUE (user_id, purpose)
);
