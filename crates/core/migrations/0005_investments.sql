-- An investment can be one of the owner's assets (gold, a flat): its value then is the asset's value. One
-- asset backs at most one account.
ALTER TABLE accounts ADD COLUMN asset_id BIGINT REFERENCES assets(id) ON DELETE SET NULL;
CREATE UNIQUE INDEX idx_accounts_asset ON accounts(asset_id) WHERE asset_id IS NOT NULL;

-- Investment types used to be free text. They are a fixed list now (see INVEST_TYPES in the api crate): keep
-- what can be recognised, call the rest `other`, and leave non-investments blank.
UPDATE accounts SET invest_kind = CASE
    WHEN invest_kind ~ 'ppf' THEN 'ppf'
    WHEN invest_kind ~ 'epf' THEN 'epf'
    WHEN invest_kind ~ '(nps|pension)' THEN 'nps'
    WHEN invest_kind ~ '(^fd$|fixed)' THEN 'fixed deposit'
    WHEN invest_kind ~ '(^rd$|recurring)' THEN 'recurring deposit'
    WHEN invest_kind ~ 'gold' THEN 'gold'
    WHEN invest_kind ~ '(real estate|property|land|plot)' THEN 'real estate'
    WHEN invest_kind ~ 'etf' THEN 'etf'
    WHEN invest_kind ~ 'bond' THEN 'bonds'
    WHEN invest_kind ~ '(crypto|bitcoin|btc)' THEN 'crypto'
    WHEN invest_kind ~ '(stock|share|equity)' THEN 'stocks'
    WHEN invest_kind ~ '(mutual|index fund|sip|fund)' THEN 'mutual fund'
    ELSE 'other' END
WHERE kind = 'investment';
UPDATE accounts SET invest_kind = '' WHERE kind <> 'investment';
