-- An invite code is good for a week.
ALTER TABLE families ADD COLUMN invite_expires TEXT;

-- Foreign keys that are looked up by their child side
CREATE INDEX idx_tx_created_by ON transactions(created_by);
CREATE INDEX idx_subs_account ON subscriptions(account_id);
CREATE INDEX idx_tokens_session_use ON tokens(kind, created_at);
CREATE INDEX idx_notes_created ON notifications(created_at);
CREATE INDEX idx_attach_tx ON attachments(tx_id);
