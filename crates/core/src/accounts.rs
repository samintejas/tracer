use std::collections::HashMap;

use chrono::Datelike;
use sqlx::{AssertSqlSafe, Row};

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean, today, visible};

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
            Some(s) if tracer_api::loan::parse_ym(s).is_some() => {}
            _ => return Err(Error::bad("a loan needs a start month as YYYY-MM")),
        }
    }
    Ok(())
}

impl Store {
    /// Raw sum of transactions per account.
    async fn tx_sums(&self) -> Result<HashMap<i64, i64>> {
        let rows = sqlx::query("SELECT account_id, SUM(amount)::BIGINT AS s FROM transactions GROUP BY account_id").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    async fn owners_of(&self) -> Result<HashMap<i64, Vec<Member>>> {
        let rows = sqlx::query("SELECT o.account_id, u.id, u.name, u.initials FROM account_owners o JOIN users u ON u.id = o.user_id ORDER BY u.id")
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
        let sums = self.tx_sums().await?;
        let owners = self.owners_of().await?;
        let now = today();
        let mut out = Vec::new();
        for r in rows {
            let id: i64 = r.get("id");
            let kind = AccountKind::parse(&r.get::<String, _>("kind")).ok_or_else(|| Error::Internal("bad account kind".into()))?;
            let details = details_from(&r);
            let raw = r.get::<i64, _>("opening") + sums.get(&id).copied().unwrap_or(0);
            let loan = if kind == AccountKind::Loan {
                tracer_api::loan::compute(
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
            let balance = match (kind, &loan) {
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

    /// Only a bank account can be joint, and its co-owners must be in the caller's family.
    async fn check_owners(&self, c: &Caller, kind: AccountKind, owner_ids: &[i64]) -> Result<Vec<i64>> {
        let mut all = vec![c.user_id];
        for id in owner_ids {
            if !all.contains(id) {
                all.push(*id);
            }
        }
        if all.len() > 1 {
            if kind != AccountKind::Bank {
                return Err(Error::bad("only a bank account can be joint. share this one with the family instead"));
            }
            let fam = self.family(c.user_id).await?.ok_or_else(|| Error::bad("create or join a family to share an account with someone"))?;
            for id in &all {
                if !fam.members.iter().any(|m| m.id == *id) {
                    return Err(Error::bad("co-owners must be members of your family"));
                }
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
        validate(b.kind, &b.details)?;
        let owners = self.check_owners(c, b.kind, &b.owner_ids).await?;
        let shown = b.balance.unwrap_or(0);
        let opening = match b.kind {
            AccountKind::Credit => -shown,
            AccountKind::Loan => 0,
            _ => shown,
        };
        let d = &b.details;
        let id = sqlx::query(
            "INSERT INTO accounts (name, kind, visibility, opening, institution, last4, credit_limit, statement_day, due_day, \
             loan_total, rate, tenure, start, emi, emi_day, invest_kind, invested, sip, sip_day) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19) RETURNING id",
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
        .fetch_one(&self.pool)
        .await?
        .get::<i64, _>(0);
        for o in &owners {
            sqlx::query("INSERT INTO account_owners (account_id, user_id) VALUES ($1, $2)").bind(id).bind(o).execute(&self.pool).await?;
        }
        self.account_unchecked(c.user_id, id).await
    }

    /// Delete an account and its transactions. The other side of a transfer stays, as plain money in or
    /// out. Owner only. Cannot be undone.
    pub async fn delete_account(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        self.owned_account(c, id).await?;
        self.remove_account(id).await
    }

    pub(crate) async fn remove_account(&self, id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE transactions SET kind = CASE WHEN amount < 0 THEN 'debit' ELSE 'credit' END, transfer_id = NULL \
             WHERE account_id <> $1 AND transfer_id IN (SELECT transfer_id FROM transactions WHERE account_id = $1 AND transfer_id IS NOT NULL)",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM accounts WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn update_account(&self, c: &Caller, id: i64, b: UpdateAccount) -> Result<Account> {
        c.need("edit")?;
        let a = self.owned_account(c, id).await?;
        if let Some(n) = &b.name {
            if clean(n).is_empty() {
                return Err(Error::bad("name the account"));
            }
            sqlx::query("UPDATE accounts SET name = $1 WHERE id = $2").bind(clean(n).to_lowercase()).bind(id).execute(&self.pool).await?;
        }
        if let Some(v) = b.visibility {
            sqlx::query("UPDATE accounts SET visibility = $1 WHERE id = $2").bind(v.as_str()).bind(id).execute(&self.pool).await?;
        }
        if let Some(arch) = b.archived {
            sqlx::query("UPDATE accounts SET archived = $1 WHERE id = $2").bind(arch as i64).bind(id).execute(&self.pool).await?;
        }
        if let Some(ids) = &b.owner_ids {
            let owners = self.check_owners(c, a.kind, ids).await?;
            sqlx::query("DELETE FROM account_owners WHERE account_id = $1").bind(id).execute(&self.pool).await?;
            for o in owners {
                sqlx::query("INSERT INTO account_owners (account_id, user_id) VALUES ($1, $2)").bind(id).bind(o).execute(&self.pool).await?;
            }
        }
        if let Some(target) = b.balance {
            // set the shown balance by moving the opening figure, so history stays as recorded
            let sum: i64 = self.tx_sums().await?.get(&id).copied().unwrap_or(0);
            let opening = match a.kind {
                AccountKind::Credit => -target - sum,
                AccountKind::Loan => return Err(Error::bad("a loan's balance comes from its schedule: change the details instead")),
                _ => target - sum,
            };
            sqlx::query("UPDATE accounts SET opening = $1 WHERE id = $2").bind(opening).bind(id).execute(&self.pool).await?;
        }
        if let Some(d) = &b.details {
            let merged = merge_details(&a.details, d);
            validate(a.kind, &merged)?;
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
            .execute(&self.pool)
            .await?;
        }
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
