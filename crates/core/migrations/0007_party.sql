-- The other side of a transaction: the merchant paid, or whoever paid you.
ALTER TABLE transactions ADD COLUMN party TEXT NOT NULL DEFAULT '';
