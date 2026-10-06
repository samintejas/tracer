use std::collections::HashMap;

use sqlx::{AssertSqlSafe, PgConnection, Postgres, QueryBuilder, Row};

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean, tags as norm_tags, visible};

const MAX_ATTACHMENT: usize = 10 * 1024 * 1024;
/// Files on one transaction.
const MAX_FILES: i64 = 8;

/// File types the browser may show in place. Anything else is kept as opaque bytes and only ever downloaded,
/// so an uploaded page or script can never run as part of the app.
pub const INLINE_TYPES: [&str; 5] = ["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"];

fn safe_mime(mime: &str) -> String {
    let m = mime.split(';').next().unwrap_or("").trim().to_lowercase();
    if INLINE_TYPES.contains(&m.as_str()) { m } else { "application/octet-stream".into() }
}

/// A spreadsheet runs a cell that starts with one of these as a formula.
fn csv_cell(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

/// One row of a transaction, with the account it is on.
struct Leg {
    id: i64,
    account_id: i64,
}

fn split(s: &Option<String>) -> Vec<String> {
    s.as_deref().unwrap_or("").split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

fn magnitude(a: i64) -> Result<i64> {
    if a <= 0 {
        return Err(Error::bad("amount must be above zero"));
    }
    Ok(a)
}

impl Store {
    /// Every filter of [`TxFilter`] as a WHERE clause over `t` (transactions) and `a` (accounts).
    fn push_filter(qb: &mut QueryBuilder<Postgres>, user: i64, f: &TxFilter) {
        qb.push(" FROM transactions t JOIN accounts a ON a.id = t.account_id WHERE ");
        qb.push(visible(user));
        if let Some(id) = f.account_id {
            qb.push(" AND t.account_id = ").push_bind(id);
        }
        let accts: Vec<i64> = split(&f.accounts).iter().filter_map(|s| s.parse().ok()).collect();
        if !accts.is_empty() {
            qb.push(" AND t.account_id IN (");
            let mut sep = qb.separated(", ");
            for a in accts {
                sep.push_bind(a);
            }
            qb.push(")");
        }
        if f.collapse_transfers == Some(true) {
            qb.push(" AND NOT (t.kind = 'transfer' AND t.amount > 0)");
        }
        if let Some(m) = f.member_id {
            qb.push(" AND t.created_by = ").push_bind(m);
        }
        let kinds = split(&f.kinds);
        if !kinds.is_empty() {
            qb.push(" AND t.kind IN (");
            let mut sep = qb.separated(", ");
            for k in kinds {
                sep.push_bind(k);
            }
            qb.push(")");
        }
        let tags = norm_tags(&split(&f.tags));
        for t in tags {
            qb.push(" AND EXISTS (SELECT 1 FROM tx_tags g WHERE g.tx_id = t.id AND g.tag = ").push_bind(t).push(")");
        }
        if let Some(d) = f.from.as_deref().filter(|s| !s.is_empty()) {
            qb.push(" AND t.date >= ").push_bind(d);
        }
        if let Some(d) = f.to.as_deref().filter(|s| !s.is_empty()) {
            qb.push(" AND t.date <= ").push_bind(d);
        }
        if let Some(q) = f.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let like = format!("%{}%", q.to_lowercase().replace('%', "\\%").replace('_', "\\_"));
            qb.push(" AND (lower(t.description) LIKE ").push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR lower(t.note) LIKE ").push_bind(like.clone());
            qb.push(" ESCAPE '\\' OR EXISTS (SELECT 1 FROM tx_tags g WHERE g.tx_id = t.id AND g.tag LIKE ").push_bind(like);
            qb.push(" ESCAPE '\\'))");
        }
    }

    pub async fn transactions(&self, c: &Caller, f: TxFilter) -> Result<TxPage> {
        c.need("transactions")?;
        let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*)");
        Self::push_filter(&mut count, c.user_id, &f);
        let total: i64 = count.build().fetch_one(&self.pool).await?.get(0);
        let mut sums = QueryBuilder::<Postgres>::new(
            "SELECT COALESCE(SUM(CASE WHEN t.kind = 'credit' THEN t.amount ELSE 0 END), 0)::BIGINT, COALESCE(SUM(CASE WHEN t.kind = 'debit' THEN -t.amount ELSE 0 END), 0)::BIGINT",
        );
        Self::push_filter(&mut sums, c.user_id, &f);
        let sums = sums.build().fetch_one(&self.pool).await?;
        let (total_in, total_out): (i64, i64) = (sums.get(0), sums.get(1));

        let mut qb = QueryBuilder::<Postgres>::new(
            "SELECT t.id, t.account_id, t.kind, t.amount, t.date, t.description, t.note, t.transfer_id, t.created_by, \
             (SELECT t2.account_id FROM transactions t2 WHERE t.transfer_id IS NOT NULL AND t2.transfer_id = t.transfer_id AND t2.id <> t.id LIMIT 1) AS counterpart_id, \
             (SELECT name FROM users u WHERE u.id = t.created_by) AS by_name, \
             (SELECT initials FROM users u WHERE u.id = t.created_by) AS by_initials",
        );
        Self::push_filter(&mut qb, c.user_id, &f);
        let dir = if f.dir.as_deref() == Some("asc") { "ASC" } else { "DESC" };
        let order = match f.sort.as_deref() {
            Some("description") => format!("lower(t.description) {dir}, t.id DESC"),
            Some("amount") => format!("t.amount {dir}, t.id DESC"),
            Some("tag") => format!("(SELECT g.tag FROM tx_tags g WHERE g.tx_id = t.id ORDER BY g.id LIMIT 1) {dir}, t.date DESC, t.id DESC"),
            Some("account") => format!("lower(a.name) {dir}, t.date DESC, t.id DESC"),
            Some("person") => format!("(SELECT lower(u.name) FROM users u WHERE u.id = t.created_by) {dir}, t.date DESC, t.id DESC"),
            _ => format!("t.date {dir}, t.id {dir}"),
        };
        qb.push(format!(" ORDER BY {order} LIMIT "));
        qb.push_bind(f.limit.unwrap_or(50).clamp(1, 500) as i64);
        qb.push(" OFFSET ").push_bind(f.offset.unwrap_or(0) as i64);
        let rows = qb.build().fetch_all(&self.pool).await?;
        Ok(TxPage { items: self.hydrate(rows).await?, total, total_in, total_out })
    }

    async fn hydrate(&self, rows: Vec<sqlx::postgres::PgRow>) -> Result<Vec<Transaction>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<i64> = rows.iter().map(|r| r.get("id")).collect();
        let in_list = |qb: &mut QueryBuilder<Postgres>| {
            qb.push(" IN (");
            let mut sep = qb.separated(", ");
            for id in &ids {
                sep.push_bind(*id);
            }
            qb.push(")");
        };
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        let mut qb = QueryBuilder::<Postgres>::new("SELECT tx_id, tag FROM tx_tags WHERE tx_id");
        in_list(&mut qb);
        qb.push(" ORDER BY id");
        for r in qb.build().fetch_all(&self.pool).await? {
            tags.entry(r.get(0)).or_default().push(r.get(1));
        }
        let mut files: HashMap<i64, Vec<Attachment>> = HashMap::new();
        let mut qb = QueryBuilder::<Postgres>::new("SELECT id, tx_id, name, size FROM attachments WHERE tx_id");
        in_list(&mut qb);
        qb.push(" ORDER BY id");
        for r in qb.build().fetch_all(&self.pool).await? {
            files.entry(r.get(1)).or_default().push(Attachment { id: r.get(0), name: r.get(2), size: r.get(3) });
        }
        Ok(rows
            .iter()
            .map(|r| {
                let id: i64 = r.get("id");
                Transaction {
                    id,
                    account_id: r.get("account_id"),
                    kind: TxKind::parse(&r.get::<String, _>("kind")).unwrap_or(TxKind::Debit),
                    amount: r.get("amount"),
                    date: r.get("date"),
                    description: r.get("description"),
                    tags: tags.remove(&id).unwrap_or_default(),
                    note: r.get("note"),
                    counterpart_id: r.get("counterpart_id"),
                    transfer_id: r.get("transfer_id"),
                    created_by: Member { id: r.get("created_by"), name: r.get("by_name"), initials: r.get("by_initials"), email: String::new() },
                    attachments: files.remove(&id).unwrap_or_default(),
                }
            })
            .collect())
    }

    pub async fn transaction(&self, c: &Caller, id: i64) -> Result<Transaction> {
        c.need("transactions")?;
        self.tx_for(c.user_id, id).await
    }

    /// A transaction the person can see, whatever scope the caller holds: writes return what they made.
    pub(crate) async fn tx_for(&self, user_id: i64, id: i64) -> Result<Transaction> {
        let mut qb = QueryBuilder::<Postgres>::new(
            "SELECT t.id, t.account_id, t.kind, t.amount, t.date, t.description, t.note, t.transfer_id, t.created_by, \
             (SELECT t2.account_id FROM transactions t2 WHERE t.transfer_id IS NOT NULL AND t2.transfer_id = t.transfer_id AND t2.id <> t.id LIMIT 1) AS counterpart_id, \
             (SELECT name FROM users u WHERE u.id = t.created_by) AS by_name, \
             (SELECT initials FROM users u WHERE u.id = t.created_by) AS by_initials \
             FROM transactions t JOIN accounts a ON a.id = t.account_id WHERE ",
        );
        qb.push(visible(user_id)).push(" AND t.id = ").push_bind(id);
        let rows = qb.build().fetch_all(&self.pool).await?;
        self.hydrate(rows).await?.into_iter().next().ok_or(Error::NotFound("transaction"))
    }

    async fn set_tags(db: &mut PgConnection, tx: i64, tags: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM tx_tags WHERE tx_id = $1").bind(tx).execute(&mut *db).await?;
        for t in tags {
            sqlx::query("INSERT INTO tx_tags (tx_id, tag) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(tx).bind(t).execute(&mut *db).await?;
        }
        Ok(())
    }

    pub async fn add_transaction(&self, c: &Caller, b: NewTransaction) -> Result<Transaction> {
        c.need("add")?;
        let amount = magnitude(b.amount)?;
        let account = self.owned_account(c, b.account_id).await?;
        let signed = match b.kind {
            TxKind::Debit => -amount,
            TxKind::Credit => amount,
            TxKind::Transfer => return Err(Error::bad("use a transfer to move money between your accounts")),
        };
        let date = self.date_or_today(b.date.as_deref())?;
        // the first tag groups it in insights, so there is always one
        let mut tags = norm_tags(&b.tags);
        if tags.is_empty() {
            tags.push(if signed > 0 { "income".into() } else { "other".into() });
        }
        let mut db = self.pool.begin().await?;
        let id = sqlx::query("INSERT INTO transactions (account_id, kind, amount, date, description, note, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
            .bind(b.account_id)
            .bind(b.kind.as_str())
            .bind(signed)
            .bind(&date)
            .bind(clean(&b.description))
            .bind(clean(&b.note))
            .bind(c.user_id)
            .fetch_one(&mut *db)
            .await?
            .get::<i64, _>(0);
        Self::set_tags(&mut db, id, &tags).await?;
        db.commit().await?;
        if let Err(e) = self.tell_others(c, &account, signed, &b.description).await {
            tracing::warn!("could not notify the others about transaction {id}: {e}");
        }
        self.tx_for(c.user_id, id).await
    }

    /// Tell everyone else who can see this account that something was added to it.
    async fn tell_others(&self, c: &Caller, account: &Account, signed: i64, desc: &str) -> Result<()> {
        let me = self.user(c.user_id).await?;
        let mut people: Vec<i64> = account.owners.iter().map(|o| o.id).filter(|id| *id != c.user_id).collect();
        if account.visibility == Visibility::Shared {
            if let Some(f) = self.family(c.user_id).await? {
                for m in f.members {
                    if m.id != c.user_id && !people.contains(&m.id) {
                        people.push(m.id);
                    }
                }
            }
        }
        let what = if clean(desc).is_empty() { "a transaction".to_string() } else { clean(desc) };
        let verb = if signed < 0 { "spent" } else { "received" };
        let first = me.name.split(' ').next().unwrap_or("someone");
        for p in people {
            let them = self.user(p).await?;
            if !them.notify_joint {
                continue;
            }
            let amt = crate::notify::show_money(signed, &them.currency);
            self.notify(p, &format!("{first} {verb} {amt}"), &format!("{what} · {}", account.name), "transactions", None).await?;
        }
        Ok(())
    }

    pub async fn transfer(&self, c: &Caller, b: NewTransfer) -> Result<Vec<Transaction>> {
        c.need("add")?;
        let amount = magnitude(b.amount)?;
        if b.from_account_id == b.to_account_id {
            return Err(Error::bad("choose two different accounts"));
        }
        let from = self.owned_account(c, b.from_account_id).await?;
        let to = self.owned_account(c, b.to_account_id).await.map_err(|e| match e {
            Error::Forbidden(_) => Error::Forbidden("a transfer goes between accounts you own".into()),
            other => other,
        })?;
        let date = self.date_or_today(b.date.as_deref())?;
        // a transfer says what it was for when nobody typed it
        let (auto, extra) = match to.kind {
            AccountKind::Credit => (format!("card bill, {}", to.name), Some("card payment")),
            AccountKind::Loan => (format!("emi, {}", to.name), Some("emi")),
            AccountKind::Investment => (format!("invested in {}", to.name), Some("investment")),
            AccountKind::Bank => (format!("to {}", to.name), None),
        };
        let mut tags: Vec<String> = vec!["transfer".into()];
        tags.extend(extra.map(String::from));
        for t in norm_tags(&b.tags) {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        let typed = clean(&b.description);
        let (out_desc, in_desc) = if typed.is_empty() { (auto, format!("from {}", from.name)) } else { (typed.clone(), typed) };
        let mut db = self.pool.begin().await?;
        let mut ids = Vec::new();
        for (acct, amt, d) in [(from.id, -amount, out_desc), (to.id, amount, in_desc)] {
            let id = sqlx::query("INSERT INTO transactions (account_id, kind, amount, date, description, note, created_by) VALUES ($1, 'transfer', $2, $3, $4, $5, $6) RETURNING id")
                .bind(acct)
                .bind(amt)
                .bind(&date)
                .bind(d)
                .bind(clean(&b.note))
                .bind(c.user_id)
                .fetch_one(&mut *db)
                .await?
                .get::<i64, _>(0);
            ids.push(id);
        }
        sqlx::query("UPDATE transactions SET transfer_id = $1 WHERE id = ANY($2)").bind(ids[0]).bind(&ids).execute(&mut *db).await?;
        for id in &ids {
            Self::set_tags(&mut db, *id, &tags).await?;
        }
        db.commit().await?;
        if let Err(e) = self.tell_others(c, &to, amount, &format!("transfer from {}", from.name)).await {
            tracing::warn!("could not notify the others about transfer {}: {e}", ids[0]);
        }
        let mut out = Vec::new();
        for id in ids {
            out.push(self.tx_for(c.user_id, id).await?);
        }
        Ok(out)
    }

    /// Every row of the transaction: itself, or both legs of a transfer.
    async fn legs(&self, db: &mut PgConnection, id: i64, transfer_id: Option<i64>) -> Result<Vec<Leg>> {
        let rows = match transfer_id {
            None => sqlx::query("SELECT id, account_id FROM transactions WHERE id = $1").bind(id).fetch_all(&mut *db).await?,
            Some(t) => sqlx::query("SELECT id, account_id FROM transactions WHERE transfer_id = $1 ORDER BY id").bind(t).fetch_all(&mut *db).await?,
        };
        Ok(rows.iter().map(|r| Leg { id: r.get(0), account_id: r.get(1) }).collect())
    }

    /// The caller must own the account of every leg: a change to a transfer changes both.
    async fn own_all(&self, c: &Caller, legs: &[Leg]) -> Result<()> {
        for l in legs {
            self.owned_account(c, l.account_id).await?;
        }
        Ok(())
    }

    pub async fn update_transaction(&self, c: &Caller, id: i64, b: UpdateTransaction) -> Result<Transaction> {
        c.need("edit")?;
        let tx = self.tx_for(c.user_id, id).await?;
        let mut db = self.pool.begin().await?;
        let legs = self.legs(&mut db, id, tx.transfer_id).await?;
        self.own_all(c, &legs).await?;
        // everything that can be refused is refused before anything changes
        let amount = b.amount.map(magnitude).transpose()?;
        let date = b.date.as_deref().map(|d| self.date_or_today(Some(d))).transpose()?;
        let kind = b.kind.filter(|k| *k != tx.kind);
        if kind.is_some() && (tx.transfer_id.is_some() || kind == Some(TxKind::Transfer)) {
            return Err(Error::bad("a transfer cannot become money in or out, or the other way; delete it and add a new one"));
        }
        let account = b.account_id.filter(|a| *a != tx.account_id);
        if let Some(acct) = account {
            if tx.transfer_id.is_some() {
                return Err(Error::bad("a transfer cannot be moved to another account; delete it and add a new one"));
            }
            self.owned_account(c, acct).await?;
        }
        for leg in &legs {
            if let Some(m) = amount {
                sqlx::query("UPDATE transactions SET amount = SIGN(amount)::BIGINT * $1 WHERE id = $2").bind(m).bind(leg.id).execute(&mut *db).await?;
            }
            if let Some(d) = &date {
                sqlx::query("UPDATE transactions SET date = $1 WHERE id = $2").bind(d).bind(leg.id).execute(&mut *db).await?;
            }
            if let Some(d) = &b.description {
                sqlx::query("UPDATE transactions SET description = $1 WHERE id = $2").bind(clean(d)).bind(leg.id).execute(&mut *db).await?;
            }
            if let Some(n) = &b.note {
                sqlx::query("UPDATE transactions SET note = $1 WHERE id = $2").bind(clean(n)).bind(leg.id).execute(&mut *db).await?;
            }
            if let Some(t) = &b.tags {
                Self::set_tags(&mut db, leg.id, &norm_tags(t)).await?;
            }
            sqlx::query("UPDATE transactions SET updated_at = utc_now() WHERE id = $1").bind(leg.id).execute(&mut *db).await?;
        }
        if let Some(kind) = kind {
            sqlx::query("UPDATE transactions SET kind = $1, amount = CASE WHEN $1 = 'debit' THEN -ABS(amount) ELSE ABS(amount) END WHERE id = $2")
                .bind(kind.as_str())
                .bind(id)
                .execute(&mut *db)
                .await?;
        }
        if let Some(acct) = account {
            sqlx::query("UPDATE transactions SET account_id = $1 WHERE id = $2").bind(acct).bind(id).execute(&mut *db).await?;
        }
        db.commit().await?;
        self.tx_for(c.user_id, id).await
    }

    /// Delete a transaction (both legs of a transfer). Returns how many rows went.
    pub async fn delete_transaction(&self, c: &Caller, id: i64) -> Result<u64> {
        c.need("edit")?;
        let tx = self.tx_for(c.user_id, id).await?;
        let mut db = self.pool.begin().await?;
        let legs = self.legs(&mut db, id, tx.transfer_id).await?;
        self.own_all(c, &legs).await?;
        let ids: Vec<i64> = legs.iter().map(|l| l.id).collect();
        let n = sqlx::query("DELETE FROM transactions WHERE id = ANY($1)").bind(&ids).execute(&mut *db).await?.rows_affected();
        db.commit().await?;
        Ok(n)
    }

    /// Tags you have used, with how often, most used first. For suggestions.
    pub async fn tags(&self, c: &Caller) -> Result<Vec<(String, i64)>> {
        c.need("transactions")?;
        let sql = format!(
            "SELECT g.tag, COUNT(*) AS n FROM tx_tags g JOIN transactions t ON t.id = g.tx_id JOIN accounts a ON a.id = t.account_id \
             WHERE {} GROUP BY g.tag ORDER BY n DESC, g.tag",
            visible(c.user_id)
        );
        let rows = sqlx::query(AssertSqlSafe(sql)).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// Everything the caller can see, as one csv: accounts first, then transactions, newest first.
    pub async fn export_csv(&self, c: &Caller) -> Result<String> {
        c.need("read")?;
        c.need("transactions")?;
        let cell = csv_cell;
        let accounts = self.accounts(c, true).await?;
        let mut out = String::from("accounts\nname,kind,owners,who sees it,balance\n");
        for a in &accounts {
            let owners = a.owners.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(" and ");
            let vis = if a.joint { "joint" } else { a.visibility.as_str() };
            let bal = if a.kind.is_liability() { -a.balance } else { a.balance };
            out.push_str(&format!("{},{},{},{},{}\n", cell(&a.name), a.kind.as_str(), cell(&owners), vis, pebblelab_api::money::format_minor(bal)));
        }
        out.push_str("\ntransactions\ndate,description,tags,account,kind,amount,by,note\n");
        let mut offset = 0;
        loop {
            let page = self.transactions(c, TxFilter { limit: Some(500), offset: Some(offset), ..Default::default() }).await?;
            for t in &page.items {
                let acct = accounts.iter().find(|a| a.id == t.account_id).map(|a| a.name.as_str()).unwrap_or("");
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    t.date, cell(&t.description), cell(&t.tags.join(" ")), cell(acct), t.kind.as_str(), pebblelab_api::money::format_minor(t.amount), cell(&t.created_by.name), cell(&t.note)
                ));
            }
            offset += 500;
            if page.items.len() < 500 {
                break;
            }
        }
        Ok(out)
    }

    // ---- attachments ---------------------------------------------------------------------------------

    pub async fn add_attachment(&self, c: &Caller, tx_id: i64, name: &str, mime: &str, data: Vec<u8>) -> Result<Attachment> {
        c.need("edit")?;
        if data.is_empty() {
            return Err(Error::bad("the file is empty"));
        }
        if data.len() > MAX_ATTACHMENT {
            return Err(Error::bad("files can be at most 10 MB"));
        }
        let tx = self.tx_for(c.user_id, tx_id).await?;
        self.owned_account(c, tx.account_id).await?;
        let have: i64 = sqlx::query("SELECT COUNT(*) FROM attachments WHERE tx_id = $1").bind(tx_id).fetch_one(&self.pool).await?.get(0);
        if have >= MAX_FILES {
            return Err(Error::bad(format!("a transaction can hold {MAX_FILES} files")));
        }
        let mime = safe_mime(mime);
        let name: String = name.rsplit(['/', '\\']).next().unwrap_or("file").chars().take(120).collect();
        let size = data.len() as i64;
        let id = sqlx::query("INSERT INTO attachments (tx_id, name, size, mime, data) VALUES ($1,$2,$3,$4,$5) RETURNING id")
            .bind(tx_id)
            .bind(&name)
            .bind(size)
            .bind(&mime)
            .bind(data)
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>(0);
        Ok(Attachment { id, name, size })
    }

    /// `(name, mime, bytes)` of an attachment on a transaction the caller can see.
    pub async fn attachment(&self, c: &Caller, id: i64) -> Result<(String, String, Vec<u8>)> {
        c.need("transactions")?;
        let r = sqlx::query("SELECT tx_id, name, mime, data FROM attachments WHERE id = $1").bind(id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound("attachment"))?;
        self.tx_for(c.user_id, r.get("tx_id")).await?;
        Ok((r.get("name"), safe_mime(&r.get::<String, _>("mime")), r.get("data")))
    }

    pub async fn delete_attachment(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        let tx_id: i64 = sqlx::query("SELECT tx_id FROM attachments WHERE id = $1").bind(id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound("attachment"))?.get(0);
        let tx = self.tx_for(c.user_id, tx_id).await?;
        self.owned_account(c, tx.account_id).await?;
        sqlx::query("DELETE FROM attachments WHERE id = $1").bind(id).execute(&self.pool).await?;
        Ok(())
    }
}
