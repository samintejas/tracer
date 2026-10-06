-- A password reset that has been emailed and not used yet. Only the hash of the token is kept; it works
-- once, for an hour, and a newer request replaces an older one.
CREATE TABLE password_resets (
    token_hash TEXT PRIMARY KEY,
    user_id    BIGINT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT utc_now()
);
