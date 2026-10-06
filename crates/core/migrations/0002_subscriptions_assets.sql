-- A subscription is a standing charge on one of the owner's accounts. When its renewal date arrives a
-- transaction is added and the date moves on by a month or a year. A paused one keeps its details.
CREATE TABLE subscriptions (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    amount     BIGINT NOT NULL CHECK (amount > 0),
    cycle      TEXT NOT NULL CHECK (cycle IN ('monthly','yearly')),
    next_on    TEXT,                                   -- YYYY-MM-DD, empty for one that was never scheduled
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL DEFAULT '',
    active     BIGINT NOT NULL DEFAULT 1,
    last_on    TEXT,                                   -- the last renewal that was added to transactions
    created_at TEXT NOT NULL DEFAULT utc_now()
);
CREATE INDEX idx_subs_user ON subscriptions(user_id);

-- Things owned outside any account: a home, a vehicle, gold. Their value is whatever the owner says it is
-- now, and it counts as an asset in insights.
CREATE TABLE assets (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('property','vehicle','gold','electronics','other')),
    bought     TEXT NOT NULL DEFAULT '',               -- YYYY-MM
    cost       BIGINT NOT NULL DEFAULT 0,
    value      BIGINT NOT NULL CHECK (value >= 0),
    note       TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT utc_now()
);
CREATE INDEX idx_assets_user ON assets(user_id);
