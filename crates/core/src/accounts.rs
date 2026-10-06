use std::collections::HashMap;

use chrono::Datelike;
use sqlx::{AssertSqlSafe, PgConnection, Row};

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean, visible};

fn opt_i(r: &sqlx::postgres::PgRow, col: &str) -> Option<i64> {
    r.get::<Option<i64>, _>(col)
}

fn details_from(r: &sqlx::postgres::PgRow) -> AccountDetails {
    AccountDetails {
        institution: r.get("institution"),
        last4: r.get("last4"),
        limit: opt_i(r, "credit_limit"),
        statement_day: opt_i(r, "statement_day").map(|v| v as u32),
        due_day: opt_i(r, "due_day").map(|v| v as u32),
        loan_total: opt_i(r, "loan_total"),
        rate: r.get("rate"),
        tenure: opt_i(r, "tenure").map(|v| v as u32),
        start: r.get("start"),
        emi: opt_i(r, "emi"),
        emi_day: opt_i(r, "emi_day").map(|v| v as u32),
        invest_kind: r.get("invest_kind"),
        invested: opt_i(r, "invested"),
        sip: opt_i(r, "sip"),
        sip_day: opt_i(r, "sip_day").map(|v| v as u32),
    }
}

fn check_day(d: Option<u32>, what: &str) -> Result<()> {
    match d {
        Some(d) if !(1..=31).contains(&d) => Err(Error::bad(format!("{what} must be a day of the month, 1 to 31"))),
        _ => Ok(()),
    }
}

fn validate(kind: AccountKind, d: &AccountDetails) -> Result<()> {
    check_day(d.statement_day, "statement day")?;
    check_day(d.due_day, "due day")?;
    check_day(d.emi_day, "emi day")?;
    check_day(d.sip_day, "sip day")?;
    if kind == AccountKind::Loan {
        if d.loan_total.is_none_or(|t| t <= 0) {
            return Err(Error::bad("a loan needs the amount borrowed (loan_total)"));
        }
        if d.tenure.is_none_or(|t| t == 0) {
            return Err(Error::bad("a loan needs a tenure in months"));
        }
        match &d.start {
            Some(s) if pebblelab_api::loan::parse_ym(s).is_some() => {}
            _ => return Err(Error::bad("a loan needs a start month as YYYY-MM")),
        }
    }
    if kind == AccountKind::Investment {
        let t = d.invest_kind.trim();
        if !t.is_empty() && !is_invest_type(t) {
            let all: Vec<&str> = INVEST_TYPES.iter().map(|(k, _)| *k).chain(["other"]).collect();
            return Err(Error::bad(format!("investment type must be one of: {}", all.join(", "))));
        }
        if invest_group(t) == InvestGroup::Deposit {
            let lump = t == "fixed deposit";
            if lump && d.invested.is_none_or(|v| v <= 0) {
                return Err(Error::bad("a fixed deposit needs the amount deposited (invested)"));
            }
            if !lump && d.sip.is_none_or(|v| v <= 0) {
                return Err(Error::bad("a recurring deposit needs its monthly instalment (sip)"));
            }
            if d.rate.is_none_or(|r| !(0.0..=100.0).contains(&r)) {
                return Err(Error::bad("a deposit needs its interest rate, as a percent a year"));
            }
            if d.tenure.is_none_or(|t| t == 0) {
                return Err(Error::bad("a deposit needs its term in months (tenure)"));
            }
            match &d.start {
                Some(s) if pebblelab_api::loan::parse_ym(s).is_some() => {}
                _ => return Err(Error::bad("a deposit needs the month it was opened as YYYY-MM (start)")),
            }
        }
    }
    Ok(())
}

/// Only these investments can stand for one of your assets.
fn can_link(invest_kind: &str) -> bool {
    matches!(invest_group(invest_kind.trim()), InvestGroup::Physical | InvestGroup::Other)
}

impl Store {
    /// The asset an investment will stand for: the caller's own, and not already behind another account.
    /// Returns its value.
    async fn check_link(&self, db: &mut PgConnection, c: &Caller, asset_id: i64, this_account: Option<i64>) -> Result<i64> {
        let value: i64 = sqlx::query("SELECT value FROM assets WHERE id = $1 AND user_id = $2 FOR UPDATE")
            .bind(asset_id)
            .bind(c.user_id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or(Error::NotFound("asset"))?
            .get(0);
        let other = sqlx::query("SELECT name FROM accounts WHERE asset_id = $1 AND id <> $2").bind(asset_id).bind(this_account.unwrap_or(0)).fetch_optional(&mut *db).await?;
        if let Some(r) = other {
            return Err(Error::Conflict(format!("that asset is already the value of {}", r.get::<String, _>(0))));
        }
        Ok(value)
    }

    /// Raw sum of transactions for each of these accounts.
    async fn tx_sums(&self, ids: &[i64]) -> Result<HashMap<i64, i64>> {
        let rows = sqlx::query("SELECT account_id, SUM(amount)::BIGINT AS s FROM transactions WHERE account_id = ANY($1) GROUP BY account_id")
            .bind(ids)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    async fn owners_of(&self, ids: &[i64]) -> Result<HashMap<i64, Vec<Member>>> {
        let rows = sqlx::query("SELECT o.account_id, u.id, u.name, u.initials FROM account_owners o JOIN users u ON u.id = o.user_id WHERE o.account_id = ANY($1) ORDER BY u.id")
            .bind(ids)
            .fetch_all(&self.pool)
            .await?;
        let mut m: HashMap<i64, Vec<Member>> = HashMap::new();
        for r in rows {
            m.entry(r.get(0)).or_default().push(Member { id: r.get(1), name: r.get(2), initials: r.get(3), email: String::new() });
        }
        Ok(m)
    }

    async fn load_accounts(&self, user_id: i64, only: Option<i64>, include_archived: bool) -> Result<Vec<Account>> {
        let sql = format!(
            "SELECT a.* FROM accounts a WHERE {} AND ($1 IS NULL OR a.id = $1) AND ($2 = 1 OR a.archived = 0) ORDER BY a.id",
            visible(user_id)
        );
        let rows = sqlx::query(AssertSqlSafe(sql)).bind(only).bind(include_archived as i64).fetch_all(&self.pool).await?;
        let ids: Vec<i64> = rows.iter().map(|r| r.get::<i64, _>("id")).collect();
        let sums = self.tx_sums(&ids).await?;
        let owners = self.owners_of(&ids).await?;
        let asset_ids: Vec<i64> = rows.iter().filter_map(|r| r.get::<Option<i64>, _>("asset_id")).collect();
        let linked: HashMap<i64, (String, String, i64)> = if asset_ids.is_empty() {
            HashMap::new()
        } else {
            sqlx::query("SELECT id, name, kind, value FROM assets WHERE id = ANY($1)")
                .bind(&asset_ids)
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(|r| (r.get(0), (r.get(1), r.get(2), r.get(3))))
                .collect()
        };
        let now = self.today();
        let mut out = Vec::new();
        for r in rows {
            let id: i64 = r.get("id");
            let kind = AccountKind::parse(&r.get::<String, _>("kind")).ok_or_else(|| Error::Internal("bad account kind".into()))?;
            let details = details_from(&r);
            let raw = r.get::<i64, _>("opening") + sums.get(&id).copied().unwrap_or(0);
            let loan = if kind == AccountKind::Loan {
                pebblelab_api::loan::compute(
                    details.loan_total.unwrap_or(0),
                    details.rate.unwrap_or(0.0),
                    details.tenure.unwrap_or(0),
                    details.start.as_deref().unwrap_or(""),
                    details.emi,
                    (now.year(), now.month()),
                )
            } else {
                None
            };
            let asset = r.get::<Option<i64>, _>("asset_id").and_then(|aid| linked.get(&aid).map(|(name, akind, value)| (LinkedAsset { id: aid, name: name.clone(), kind: akind.clone() }, *value)));
            let deposit = (kind == AccountKind::Investment && invest_group(&details.invest_kind) == InvestGroup::Deposit)
                .then(|| {
                    let lump = if details.invest_kind == "fixed deposit" { details.invested.unwrap_or(0) } else { 0 };
                    let monthly = if details.invest_kind == "recurring deposit" { details.sip.unwrap_or(0) } else { 0 };
                    pebblelab_api::deposit::compute(lump, monthly, details.rate.unwrap_or(0.0), details.tenure.unwrap_or(0), details.start.as_deref().unwrap_or(""), (now.year(), now.month()))
                })
                .flatten();
            let balance = match (kind, &loan) {
                (AccountKind::Investment, _) if asset.is_some() => asset.as_ref().map(|(_, v)| *v).unwrap_or(0),
                (AccountKind::Loan, Some(l)) => l.balance,
                (AccountKind::Loan, None) => details.loan_total.unwrap_or(0),
                (AccountKind::Credit, _) => -raw,
                _ => raw,
            };
            let owners = owners.get(&id).cloned().unwrap_or_default();
            out.push(Account {
                id,
                name: r.get("name"),
                kind,
                joint: owners.len() > 1,
                owners,
                visibility: if r.get::<String, _>("visibility") == "shared" { Visibility::Shared } else { Visibility::Private },
                balance,
                details,
                loan,
                deposit,
                asset: asset.map(|(a, _)| a),
                archived: r.get::<i64, _>("archived") != 0,
            });
        }
        Ok(out)
    }

    pub async fn accounts(&self, c: &Caller, include_archived: bool) -> Result<Vec<Account>> {
        c.need("read")?;
        self.load_accounts(c.user_id, None, include_archived).await
    }

    pub async fn account(&self, c: &Caller, id: i64) -> Result<Account> {
        c.need("read")?;
        self.load_accounts(c.user_id, Some(id), true).await?.into_iter().next().ok_or(Error::NotFound("account"))
    }

    /// The account, only if the caller owns it (may change it).
    pub(crate) async fn owned_account(&self, c: &Caller, id: i64) -> Result<Account> {
        let a = self.account_unchecked(c.user_id, id).await?;
        if a.owners.iter().any(|o| o.id == c.user_id) {
            Ok(a)
        } else {
            Err(Error::Forbidden("only an owner can change this account".into()))
        }
    }

    pub(crate) async fn account_unchecked(&self, user_id: i64, id: i64) -> Result<Account> {
        self.load_accounts(user_id, Some(id), true).await?.into_iter().next().ok_or(Error::NotFound("account"))
    }

    /// Who the owners will be. You are always one. Only a bank account can be joint. People can be added,
    /// when they are in your family, but only they can take themselves off (see `leave_account`).
    fn check_owners(&self, c: &Caller, kind: AccountKind, wanted: &[i64], current: &[i64], family: Option<&Family>) -> Result<Vec<i64>> {
        let mut all = vec![c.user_id];
        for id in wanted {
            if !all.contains(id) {
                all.push(*id);
            }
        }
        if current.iter().any(|o| !all.contains(o)) {
            return Err(Error::Forbidden("you can only take yourself off a joint account; the others leave it themselves".into()));
        }
        if all.len() > 1 && kind != AccountKind::Bank {
            return Err(Error::bad("only a bank account can be joint. share this one with the family instead"));
        }
        let added: Vec<i64> = all.iter().copied().filter(|id| !current.contains(id) && *id != c.user_id).collect();
        if !added.is_empty() {
            let fam = family.ok_or_else(|| Error::bad("create or join a family to share an account with someone"))?;
            if added.iter().any(|id| !fam.members.iter().any(|m| m.id == *id)) {
                return Err(Error::bad("co-owners must be members of your family"));
            }
        }
        Ok(all)
    }

    pub async fn create_account(&self, c: &Caller, b: NewAccount) -> Result<Account> {
        c.need("edit")?;
        let name = clean(&b.name).to_lowercase();
        if name.is_empty() {
            return Err(Error::bad("name the account"));
        }
        let mut b = b;
        if b.kind == AccountKind::Investment {
            b.details.invest_kind = clean(&b.details.invest_kind).to_lowercase();
            if b.details.invest_kind.is_empty() {
                b.details.invest_kind = "other".into();
            }
        }
        validate(b.kind, &b.details)?;
        if b.asset_id.is_some() {
            if b.kind != AccountKind::Investment || !can_link(&b.details.invest_kind) {
                return Err(Error::bad("only gold, real estate and other investments can be linked to an asset"));
            }
            if b.balance.is_some() {
                return Err(Error::bad("a linked investment is worth what its asset is worth: leave the balance out"));
            }
        }
        let family = self.family(c.user_id).await?;
        let owners = self.check_owners(c, b.kind, &b.owner_ids, &[], family.as_ref())?;
        // a deposit with no balance starts at what its terms say it is worth today
        let estimate = (b.kind == AccountKind::Investment && invest_group(&b.details.invest_kind) == InvestGroup::Deposit)
            .then(|| {
                let d = &b.details;
                let lump = if d.invest_kind == "fixed deposit" { d.invested.unwrap_or(0) } else { 0 };
                let monthly = if d.invest_kind == "recurring deposit" { d.sip.unwrap_or(0) } else { 0 };
                let now = self.today();
                pebblelab_api::deposit::compute(lump, monthly, d.rate.unwrap_or(0.0), d.tenure.unwrap_or(0), d.start.as_deref().unwrap_or(""), (now.year(), now.month())).map(|c| c.value)
            })
            .flatten();
        let shown = b.balance.or(estimate).unwrap_or(0);
        let opening = match b.kind {
            AccountKind::Credit => -shown,
            AccountKind::Loan => 0,
            _ => shown,
        };
        let d = &b.details;
        let mut db = self.pool.begin().await?;
        if let Some(aid) = b.asset_id {
            self.check_link(&mut db, c, aid, None).await?;
        }
        let id = sqlx::query(
            "INSERT INTO accounts (name, kind, visibility, opening, institution, last4, credit_limit, statement_day, due_day, \
             loan_total, rate, tenure, start, emi, emi_day, invest_kind, invested, sip, sip_day, asset_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20) RETURNING id",
        )
        .bind(&name)
        .bind(b.kind.as_str())
        .bind(b.visibility.as_str())
        .bind(opening)
        .bind(clean(&d.institution).to_lowercase())
        .bind(clean(&d.last4))
        .bind(d.limit)
        .bind(d.statement_day.map(i64::from))
        .bind(d.due_day.map(i64::from))
        .bind(d.loan_total)
        .bind(d.rate)
        .bind(d.tenure.map(i64::from))
        .bind(d.start.as_deref())
        .bind(d.emi)
        .bind(d.emi_day.map(i64::from))
        .bind(clean(&d.invest_kind).to_lowercase())
        .bind(d.invested)
        .bind(d.sip)
        .bind(d.sip_day.map(i64::from))
        .bind(b.asset_id)
        .fetch_one(&mut *db)
        .await?
        .get::<i64, _>(0);
        for o in &owners {
            sqlx::query("INSERT INTO account_owners (account_id, user_id) VALUES ($1, $2)").bind(id).bind(o).execute(&mut *db).await?;
        }
        db.commit().await?;
        self.account_unchecked(c.user_id, id).await
    }

    /// Delete an account and its transactions. The other side of a transfer stays, as plain money in or
    /// out. Only an account with a single owner can be deleted: on a joint one, the others leave first.
    /// Cannot be undone.
    pub async fn delete_account(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        let mut db = self.pool.begin().await?;
        self.lock_account(&mut db, id).await?;
        let a = self.owned_account(c, id).await?;
        if a.owners.len() > 1 {
            return Err(Error::Conflict("this account is joint: the other owners have to leave it before it can be deleted".into()));
        }
        self.remove_account(&mut db, id).await?;
        db.commit().await?;
        Ok(())
    }

    /// Take the caller off an account they co-own. The others keep it, with its history.
    pub async fn leave_account(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        let mut db = self.pool.begin().await?;
        self.lock_account(&mut db, id).await?;
        let a = self.owned_account(c, id).await?;
        if a.owners.len() < 2 {
            return Err(Error::Conflict("you are the only owner: delete the account instead".into()));
        }
        sqlx::query("DELETE FROM account_owners WHERE account_id = $1 AND user_id = $2").bind(id).bind(c.user_id).execute(&mut *db).await?;
        sqlx::query("DELETE FROM subscriptions WHERE user_id = $1 AND account_id = $2").bind(c.user_id).bind(id).execute(&mut *db).await?;
        db.commit().await?;
        Ok(())
    }

    /// Serialise changes to one account.
    async fn lock_account(&self, db: &mut PgConnection, id: i64) -> Result<()> {
        sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE").bind(id).fetch_optional(&mut *db).await?.ok_or(Error::NotFound("account"))?;
        Ok(())
    }

    pub(crate) async fn remove_account(&self, db: &mut PgConnection, id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE transactions SET kind = CASE WHEN amount < 0 THEN 'debit' ELSE 'credit' END, transfer_id = NULL \
             WHERE account_id <> $1 AND transfer_id IN (SELECT transfer_id FROM transactions WHERE account_id = $1 AND transfer_id IS NOT NULL)",
        )
        .bind(id)
        .execute(&mut *db)
        .await?;
        sqlx::query("DELETE FROM accounts WHERE id = $1").bind(id).execute(&mut *db).await?;
        Ok(())
    }

    pub async fn update_account(&self, c: &Caller, id: i64, b: UpdateAccount) -> Result<Account> {
        c.need("edit")?;
        let mut db = self.pool.begin().await?;
        self.lock_account(&mut db, id).await?;
        let a = self.owned_account(c, id).await?;
        // check everything that can be refused before changing anything
        let name = match &b.name {
            Some(n) if clean(n).is_empty() => return Err(Error::bad("name the account")),
            Some(n) => Some(clean(n).to_lowercase()),
            None => None,
        };
        let owners = match &b.owner_ids {
            Some(ids) => {
                let family = self.family_on(&mut db, c.user_id).await?;
                let current: Vec<i64> = a.owners.iter().map(|o| o.id).collect();
                Some(self.check_owners(c, a.kind, ids, &current, family.as_ref())?)
            }
            None => None,
        };
        let merged = match &b.details {
            Some(d) => {
                let mut m = merge_details(&a.details, d);
                if a.kind == AccountKind::Investment && m.invest_kind.is_empty() {
                    m.invest_kind = "other".into();
                }
                validate(a.kind, &m)?;
                Some(m)
            }
            None => None,
        };
        // the asset behind it after this change
        let link_after: Option<i64> = match b.asset_id {
            Some(wanted) => wanted,
            None => a.asset.as_ref().map(|l| l.id),
        };
        let kind_after = merged.as_ref().map(|m| m.invest_kind.clone()).unwrap_or_else(|| a.details.invest_kind.clone());
        if let Some(aid) = link_after {
            if a.kind != AccountKind::Investment || !can_link(&kind_after) {
                return Err(Error::bad("only gold, real estate and other investments can be linked to an asset: unlink it to change the type"));
            }
            if a.asset.as_ref().map(|l| l.id) != Some(aid) {
                self.check_link(&mut db, c, aid, Some(id)).await?;
            }
            if b.balance.is_some() {
                return Err(Error::bad("a linked investment is worth what its asset is worth: change the asset's value, or unlink it first"));
            }
        }
        let unlinking = a.asset.is_some() && link_after.is_none();
        let opening = match (b.balance, unlinking) {
            (None, false) => None,
            (target, _) => {
                let sum: i64 = sqlx::query("SELECT COALESCE(SUM(amount), 0)::BIGINT FROM transactions WHERE account_id = $1").bind(id).fetch_one(&mut *db).await?.get(0);
                // set the shown balance by moving the opening figure, so history stays as recorded. Taking
                // off a link keeps the value the asset gave it, so nothing jumps.
                Some(match (a.kind, target) {
                    (AccountKind::Credit, Some(t)) => -t - sum,
                    (AccountKind::Loan, Some(_)) => return Err(Error::bad("a loan's balance comes from its schedule: change the details instead")),
                    (_, Some(t)) => t - sum,
                    (_, None) => a.balance - sum,
                })
            }
        };
        if let Some(n) = name {
            sqlx::query("UPDATE accounts SET name = $1 WHERE id = $2").bind(n).bind(id).execute(&mut *db).await?;
        }
        if let Some(v) = b.visibility {
            sqlx::query("UPDATE accounts SET visibility = $1 WHERE id = $2").bind(v.as_str()).bind(id).execute(&mut *db).await?;
        }
        if let Some(arch) = b.archived {
            sqlx::query("UPDATE accounts SET archived = $1 WHERE id = $2").bind(arch as i64).bind(id).execute(&mut *db).await?;
        }
        if let Some(owners) = owners {
            for o in owners {
                sqlx::query("INSERT INTO account_owners (account_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(id).bind(o).execute(&mut *db).await?;
            }
        }
        if let Some(opening) = opening {
            sqlx::query("UPDATE accounts SET opening = $1 WHERE id = $2").bind(opening).bind(id).execute(&mut *db).await?;
        }
        if b.asset_id.is_some() {
            sqlx::query("UPDATE accounts SET asset_id = $1 WHERE id = $2").bind(link_after).bind(id).execute(&mut *db).await?;
        }
        if let Some(merged) = merged {
            sqlx::query(
                "UPDATE accounts SET institution=$1, last4=$2, credit_limit=$3, statement_day=$4, due_day=$5, loan_total=$6, rate=$7, tenure=$8, \
                 start=$9, emi=$10, emi_day=$11, invest_kind=$12, invested=$13, sip=$14, sip_day=$15 WHERE id = $16",
            )
            .bind(&merged.institution)
            .bind(&merged.last4)
            .bind(merged.limit)
            .bind(merged.statement_day.map(i64::from))
            .bind(merged.due_day.map(i64::from))
            .bind(merged.loan_total)
            .bind(merged.rate)
            .bind(merged.tenure.map(i64::from))
            .bind(merged.start.as_deref())
            .bind(merged.emi)
            .bind(merged.emi_day.map(i64::from))
            .bind(&merged.invest_kind)
            .bind(merged.invested)
            .bind(merged.sip)
            .bind(merged.sip_day.map(i64::from))
            .bind(id)
            .execute(&mut *db)
            .await?;
        }
        db.commit().await?;
        self.account_unchecked(c.user_id, id).await
    }
}

/// A partial update: text fields replace when non-empty, numbers when present.
fn merge_details(old: &AccountDetails, new: &AccountDetails) -> AccountDetails {
    let text = |o: &str, n: &str| if n.trim().is_empty() { o.to_string() } else { n.trim().to_string() };
    AccountDetails {
        institution: text(&old.institution, &new.institution),
        last4: text(&old.last4, &new.last4),
        limit: new.limit.or(old.limit),
        statement_day: new.statement_day.or(old.statement_day),
        due_day: new.due_day.or(old.due_day),
        loan_total: new.loan_total.or(old.loan_total),
        rate: new.rate.or(old.rate),
        tenure: new.tenure.or(old.tenure),
        start: new.start.clone().or_else(|| old.start.clone()),
        emi: new.emi.or(old.emi),
        emi_day: new.emi_day.or(old.emi_day),
        invest_kind: text(&old.invest_kind, &new.invest_kind),
        invested: new.invested.or(old.invested),
        sip: new.sip.or(old.sip),
        sip_day: new.sip_day.or(old.sip_day),
    }
}
