-- Timestamps are kept as utc text ('2026-10-05 12:30:00') so every client reads them the same way.
CREATE FUNCTION utc_text(t TIMESTAMPTZ) RETURNS TEXT LANGUAGE sql IMMUTABLE AS
$$ SELECT to_char(t AT TIME ZONE 'utc', 'YYYY-MM-DD HH24:MI:SS') $$;
CREATE FUNCTION utc_now() RETURNS TEXT LANGUAGE sql STABLE AS $$ SELECT utc_text(now()) $$;

-- Flags are 0/1 in BIGINT columns, and every integer is BIGINT, so one integer type crosses the wire.
CREATE TABLE users (
    id            BIGSERIAL PRIMARY KEY,
    name          TEXT NOT NULL,
    email         TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    initials      TEXT NOT NULL,
    phone         TEXT NOT NULL DEFAULT '',
    currency      TEXT NOT NULL DEFAULT 'inr' CHECK (currency IN ('inr','usd','eur')),
    picture       TEXT,                                -- a small image as a data: url
    notify_card   BIGINT NOT NULL DEFAULT 1,
    notify_emi    BIGINT NOT NULL DEFAULT 1,
    notify_joint  BIGINT NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT utc_now()
);
-- one account per address, whatever its case
CREATE UNIQUE INDEX idx_users_email ON users (lower(email));

-- A sign-in. Tokens are stored hashed. `kind` is 'session' (the web app, all rights) or 'connector'
-- (an api token for Claude, a script or the CLI, limited by `scopes`).
CREATE TABLE tokens (
    id           BIGSERIAL PRIMARY KEY,
    user_id      BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('session','connector')),
    name         TEXT NOT NULL DEFAULT '',
    token_hash   TEXT NOT NULL UNIQUE,
    tail         TEXT NOT NULL DEFAULT '',
    scopes       TEXT NOT NULL DEFAULT 'read,transactions,add,edit',
    created_at   TEXT NOT NULL DEFAULT utc_now(),
    last_used_at TEXT
);
CREATE INDEX idx_tokens_user ON tokens(user_id);

CREATE TABLE families (
    id          BIGSERIAL PRIMARY KEY,
    name        TEXT NOT NULL,
    owner_id    BIGINT NOT NULL REFERENCES users(id),
    invite_code TEXT UNIQUE,                           -- one-time: cleared when someone joins with it
    created_at  TEXT NOT NULL DEFAULT utc_now()
);
-- one family per person
CREATE TABLE family_members (
    family_id BIGINT NOT NULL REFERENCES families(id) ON DELETE CASCADE,
    user_id   BIGINT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (family_id, user_id)
);

-- Balances are never stored: balance = opening + SUM(transactions.amount). Amounts are signed minor units
-- from the account's own side. For a credit card `opening` is minus what was owed on day one.
CREATE TABLE accounts (
    id             BIGSERIAL PRIMARY KEY,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('bank','credit','loan','investment')),
    visibility     TEXT NOT NULL DEFAULT 'private' CHECK (visibility IN ('private','shared')),
    opening        BIGINT NOT NULL DEFAULT 0,
    institution    TEXT NOT NULL DEFAULT '',
    last4          TEXT NOT NULL DEFAULT '',
    credit_limit   BIGINT,
    statement_day  BIGINT,
    due_day        BIGINT,
    loan_total     BIGINT,
    rate           DOUBLE PRECISION,
    tenure         BIGINT,
    start          TEXT,
    emi            BIGINT,
    emi_day        BIGINT,
    invest_kind    TEXT NOT NULL DEFAULT '',
    invested       BIGINT,
    sip            BIGINT,
    sip_day        BIGINT,
    archived       BIGINT NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL DEFAULT utc_now()
);
CREATE TABLE account_owners (
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, user_id)
);
CREATE INDEX idx_owners_user ON account_owners(user_id);

CREATE TABLE transactions (
    id          BIGSERIAL PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('debit','credit','transfer')),
    amount      BIGINT NOT NULL CHECK (amount <> 0),
    date        TEXT NOT NULL,                       -- YYYY-MM-DD
    description TEXT NOT NULL DEFAULT '',
    note        TEXT NOT NULL DEFAULT '',
    transfer_id BIGINT,                              -- shared by both legs of a transfer
    created_by  BIGINT NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL DEFAULT utc_now(),
    updated_at  TEXT NOT NULL DEFAULT utc_now()
);
CREATE INDEX idx_tx_account_date ON transactions(account_id, date);
CREATE INDEX idx_tx_transfer ON transactions(transfer_id);

-- `id` keeps the order tags were given in: the first one groups the transaction in insights.
CREATE TABLE tx_tags (
    id    BIGSERIAL,
    tx_id BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    tag   TEXT NOT NULL,
    PRIMARY KEY (tx_id, tag)
);
CREATE INDEX idx_tags_tag ON tx_tags(tag);

CREATE TABLE attachments (
    id         BIGSERIAL PRIMARY KEY,
    tx_id      BIGINT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    size       BIGINT NOT NULL,
    mime       TEXT NOT NULL DEFAULT 'application/octet-stream',
    data       BYTEA NOT NULL,
    created_at TEXT NOT NULL DEFAULT utc_now()
);

CREATE TABLE notifications (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL DEFAULT '',
    link       TEXT NOT NULL DEFAULT '',               -- where it leads in the app: 'transactions', 'accounts/3', 'settings/family'
    key        TEXT,                                   -- set for reminders so each is made once
    read       BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT utc_now(),
    UNIQUE (user_id, key)
);
CREATE INDEX idx_notes_user ON notifications(user_id, read);
