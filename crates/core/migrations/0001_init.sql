CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    email         TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    initials      TEXT NOT NULL,
    phone         TEXT NOT NULL DEFAULT '',
    currency      TEXT NOT NULL DEFAULT 'inr' CHECK (currency IN ('inr','usd','eur')),
    picture       TEXT,                                -- a small image as a data: url
    notify_card   INTEGER NOT NULL DEFAULT 1,
    notify_emi    INTEGER NOT NULL DEFAULT 1,
    notify_joint  INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- A sign-in. Tokens are stored hashed. `kind` is 'session' (the web app, all rights) or 'connector'
-- (an api token for Claude, a script or the CLI, limited by `scopes`).
CREATE TABLE tokens (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('session','connector')),
    name         TEXT NOT NULL DEFAULT '',
    token_hash   TEXT NOT NULL UNIQUE,
    tail         TEXT NOT NULL DEFAULT '',
    scopes       TEXT NOT NULL DEFAULT 'read,transactions,add,edit',
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT
);
CREATE INDEX idx_tokens_user ON tokens(user_id);

CREATE TABLE families (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    owner_id    INTEGER NOT NULL REFERENCES users(id),
    invite_code TEXT UNIQUE,                           -- one-time: cleared when someone joins with it
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
-- one family per person
CREATE TABLE family_members (
    family_id INTEGER NOT NULL REFERENCES families(id) ON DELETE CASCADE,
    user_id   INTEGER NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (family_id, user_id)
);

-- Balances are never stored: balance = opening + SUM(transactions.amount). Amounts are signed minor units
-- from the account's own side. For a credit card `opening` is minus what was owed on day one.
CREATE TABLE accounts (
    id             INTEGER PRIMARY KEY,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('bank','credit','loan','investment')),
    visibility     TEXT NOT NULL DEFAULT 'private' CHECK (visibility IN ('private','shared')),
    opening        INTEGER NOT NULL DEFAULT 0,
    institution    TEXT NOT NULL DEFAULT '',
    last4          TEXT NOT NULL DEFAULT '',
    credit_limit   INTEGER,
    statement_day  INTEGER,
    due_day        INTEGER,
    loan_total     INTEGER,
    rate           REAL,
    tenure         INTEGER,
    start          TEXT,
    emi            INTEGER,
    emi_day        INTEGER,
    invest_kind    TEXT NOT NULL DEFAULT '',
    invested       INTEGER,
    sip            INTEGER,
    sip_day        INTEGER,
    archived       INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE account_owners (
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, user_id)
);
CREATE INDEX idx_owners_user ON account_owners(user_id);

CREATE TABLE transactions (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('debit','credit','transfer')),
    amount      INTEGER NOT NULL CHECK (amount <> 0),
    date        TEXT NOT NULL,                       -- YYYY-MM-DD
    description TEXT NOT NULL DEFAULT '',
    note        TEXT NOT NULL DEFAULT '',
    transfer_id INTEGER,                             -- shared by both legs of a transfer
    created_by  INTEGER NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_tx_account_date ON transactions(account_id, date);
CREATE INDEX idx_tx_transfer ON transactions(transfer_id);

CREATE TABLE tx_tags (
    tx_id INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    tag   TEXT NOT NULL,
    PRIMARY KEY (tx_id, tag)
);
CREATE INDEX idx_tags_tag ON tx_tags(tag);

CREATE TABLE attachments (
    id         INTEGER PRIMARY KEY,
    tx_id      INTEGER NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    mime       TEXT NOT NULL DEFAULT 'application/octet-stream',
    data       BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE notifications (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL DEFAULT '',
    link       TEXT NOT NULL DEFAULT '',               -- where it leads in the app: 'transactions', 'accounts/3', 'settings/family'
    key        TEXT,                                   -- set for reminders so each is made once
    read       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (user_id, key)
);
CREATE INDEX idx_notes_user ON notifications(user_id, read);
