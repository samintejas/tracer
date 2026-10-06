-- Who someone is at another provider ('google', 'github'). `subject` is the provider's own stable id for
-- them: never the email, which can change hands.
CREATE TABLE identities (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider   TEXT NOT NULL,
    subject    TEXT NOT NULL,
    email      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT utc_now(),
    UNIQUE (provider, subject)
);
CREATE INDEX idx_identities_user ON identities(user_id);

-- A sign-in with a provider that has started and not come back yet. Single use, ten minutes.
CREATE TABLE oauth_flows (
    state      TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,
    verifier   TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT utc_now()
);

-- What the browser is handed after a provider sign-in, in place of the session itself: it goes in a url, so
-- it works once and only for two minutes. The app swaps it for a session.
CREATE TABLE login_codes (
    code_hash  TEXT PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT utc_now()
);
