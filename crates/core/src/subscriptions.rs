use chrono::{Datelike, Months, NaiveDate};
use sqlx::Row;

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean, tags as norm_tags};

/// Renewals added in one go when a subscription was left unvisited for a long time; the rest follow on
/// the next read.
const CATCH_UP: usize = 60;

fn sub_from(r: &sqlx::postgres::PgRow) -> Subscription {
    Subscription {
        id: r.get("id"),
        name: r.get("name"),
        amount: r.get("amount"),
        cycle: Cycle::parse(&r.get::<String, _>("cycle")).unwrap_or_default(),
        next: r.get::<Option<String>, _>("next_on").filter(|s| !s.is_empty()),
        last: r.get("last_on"),
        account_id: r.get("account_id"),
        tag: r.get("tag"),
        active: r.get::<i64, _>("active") != 0,
    }
}

/// The renewal after `d`. `anchor` is the day of the month it was first set for, so a subscription on the
/// 31st goes 28 feb, 31 mar rather than drifting to the 28th for good.
fn step(d: NaiveDate, cycle: Cycle, anchor: u32) -> NaiveDate {
    let by = if cycle == Cycle::Yearly { 12 } else { 1 };
    let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap() + Months::new(by);
    let last = (first + Months::new(1)).pred_opt().unwrap().day();
    first.with_day(anchor.min(last)).unwrap()
}

fn date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| Error::bad(format!("invalid date '{s}', expected YYYY-MM-DD")))
}

/// `None` and an empty string both mean no date.
fn next_date(s: Option<&str>) -> Result<Option<String>> {
    match s.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => Ok(Some(date(s)?.to_string())),
    }
}

impl Store {
    pub async fn subscriptions(&self, c: &Caller) -> Result<Vec<Subscription>> {
        c.need("read")?;
        let rows = sqlx::query(
            "SELECT * FROM subscriptions WHERE user_id = $1 \
             ORDER BY active DESC, (next_on IS NULL OR next_on = ''), next_on, id",
        )
        .bind(c.user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(sub_from).collect())
    }

    async fn subscription(&self, c: &Caller, id: i64) -> Result<Subscription> {
        let r = sqlx::query("SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2").bind(id).bind(c.user_id).fetch_optional(&self.pool).await?;
        r.as_ref().map(sub_from).ok_or(Error::NotFound("subscription"))
    }

    pub async fn add_subscription(&self, c: &Caller, b: NewSubscription) -> Result<Subscription> {
        c.need("add")?;
        let name = clean(&b.name).to_lowercase();
        if name.is_empty() {
            return Err(Error::bad("name the subscription"));
        }
        if b.amount <= 0 {
            return Err(Error::bad("amount must be above zero"));
        }
        let next = next_date(b.next.as_deref())?;
        self.owned_account(c, b.account_id).await?;
        let tag = norm_tags(&[b.tag]).into_iter().next().unwrap_or_default();
        let id: i64 = sqlx::query("INSERT INTO subscriptions (user_id, name, amount, cycle, next_on, account_id, tag, active) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id")
            .bind(c.user_id)
            .bind(&name)
            .bind(b.amount)
            .bind(b.cycle.as_str())
            .bind(&next)
            .bind(b.account_id)
            .bind(&tag)
            .bind(b.active as i64)
            .fetch_one(&self.pool)
            .await?
            .get(0);
        // a renewal date that has already passed is added straight away
        self.post_due(c.user_id).await?;
        self.subscription(c, id).await
    }

    pub async fn update_subscription(&self, c: &Caller, id: i64, b: UpdateSubscription) -> Result<Subscription> {
        c.need("edit")?;
        let cur = self.subscription(c, id).await?;
        let name = match &b.name {
            Some(n) if clean(n).is_empty() => return Err(Error::bad("name the subscription")),
            Some(n) => clean(n).to_lowercase(),
            None => cur.name,
        };
        let amount = b.amount.unwrap_or(cur.amount);
        if amount <= 0 {
            return Err(Error::bad("amount must be above zero"));
        }
        let next = match &b.next {
            Some(n) => next_date(Some(n))?,
            None => cur.next,
        };
        let account_id = b.account_id.unwrap_or(cur.account_id);
        if account_id != cur.account_id {
            self.owned_account(c, account_id).await?;
        }
        let tag = match b.tag {
            Some(t) => norm_tags(&[t]).into_iter().next().unwrap_or_default(),
            None => cur.tag,
        };
        sqlx::query("UPDATE subscriptions SET name=$1, amount=$2, cycle=$3, next_on=$4, account_id=$5, tag=$6, active=$7 WHERE id=$8")
            .bind(&name)
            .bind(amount)
            .bind(b.cycle.unwrap_or(cur.cycle).as_str())
            .bind(&next)
            .bind(account_id)
            .bind(&tag)
            .bind(b.active.unwrap_or(cur.active) as i64)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.post_due(c.user_id).await?;
        self.subscription(c, id).await
    }

    pub async fn delete_subscription(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        self.subscription(c, id).await?;
        sqlx::query("DELETE FROM subscriptions WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Add a transaction for every renewal of this person's subscriptions that has come due, and move each
    /// date on. Returns how many subscriptions were posted.
    pub(crate) async fn post_due(&self, user_id: i64) -> Result<usize> {
        self.post_due_where(Some(user_id)).await
    }

    /// The same for everyone: what the background job runs, so a renewal on a shared account shows up for
    /// the whole family whether or not its owner opens the app.
    pub(crate) async fn post_due_all(&self) -> Result<usize> {
        self.post_due_where(None).await
    }

    async fn post_due_where(&self, user_id: Option<i64>) -> Result<usize> {
        let now = self.today().to_string();
        let due = sqlx::query(
            "SELECT s.id FROM subscriptions s JOIN account_owners o ON o.account_id = s.account_id AND o.user_id = s.user_id \
             WHERE ($1::BIGINT IS NULL OR s.user_id = $1) AND s.active = 1 AND s.next_on IS NOT NULL AND s.next_on <> '' AND s.next_on <= $2",
        )
        .bind(user_id)
        .bind(&now)
        .fetch_all(&self.pool)
        .await?;
        let n = due.len();
        for r in due {
            self.post_renewals(r.get(0), &now).await?;
        }
        Ok(n)
    }

    /// One subscription, under a row lock so two readers never add the same renewal twice.
    async fn post_renewals(&self, id: i64, now: &str) -> Result<()> {
        let mut db = self.pool.begin().await?;
        let Some(r) = sqlx::query("SELECT * FROM subscriptions WHERE id = $1 AND active = 1 FOR UPDATE").bind(id).fetch_optional(&mut *db).await? else {
            return Ok(());
        };
        let s = sub_from(&r);
        let Some(first) = s.next.as_deref().and_then(|d| date(d).ok()) else { return Ok(()) };
        let user: i64 = r.get("user_id");
        let anchor = first.day();
        let mut tags = norm_tags(&[s.tag.clone()]);
        if !tags.iter().any(|t| t == "subscription") {
            tags.push("subscription".into());
        }
        let (mut at, mut last) = (first, None);
        for _ in 0..CATCH_UP {
            if at.to_string().as_str() > now {
                break;
            }
            let tx: i64 = sqlx::query(
                "INSERT INTO transactions (account_id, kind, amount, date, description, note, created_by) VALUES ($1,'debit',$2,$3,$4,$5,$6) RETURNING id",
            )
            .bind(s.account_id)
            .bind(-s.amount)
            .bind(at.to_string())
            .bind(&s.name)
            .bind("added automatically from subscriptions.")
            .bind(user)
            .fetch_one(&mut *db)
            .await?
            .get(0);
            for t in &tags {
                sqlx::query("INSERT INTO tx_tags (tx_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(tx).bind(t).execute(&mut *db).await?;
            }
            last = Some(at);
            at = step(at, s.cycle, anchor);
        }
        sqlx::query("UPDATE subscriptions SET next_on = $1, last_on = COALESCE($2, last_on) WHERE id = $3")
            .bind(at.to_string())
            .bind(last.map(|d| d.to_string()))
            .bind(id)
            .execute(&mut *db)
            .await?;
        db.commit().await?;
        Ok(())
    }
}
