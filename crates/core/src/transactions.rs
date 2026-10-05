use std::collections::HashMap;

use sqlx::{AssertSqlSafe, QueryBuilder, Row, Sqlite};

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean, date_or_today, tags as norm_tags, visible};

const MAX_ATTACHMENT: usize = 10 * 1024 * 1024;

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
    fn push_filter(qb: &mut QueryBuilder<Sqlite>, user: i64, f: &TxFilter) {
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
        let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
        Self::push_filter(&mut count, c.user_id, &f);
        let total: i64 = count.build().fetch_one(&self.pool).await?.get(0);
        let mut sums = QueryBuilder::<Sqlite>::new(
            "SELECT COALESCE(SUM(CASE WHEN t.kind = 'credit' THEN t.amount ELSE 0 END), 0), COALESCE(SUM(CASE WHEN t.kind = 'debit' THEN -t.amount ELSE 0 END), 0)",
        );
        Self::push_filter(&mut sums, c.user_id, &f);
        let sums = sums.build().fetch_one(&self.pool).await?;
        let (total_in, total_out): (i64, i64) = (sums.get(0), sums.get(1));

        let mut qb = QueryBuilder::<Sqlite>::new(
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
            Some("tag") => format!("(SELECT g.tag FROM tx_tags g WHERE g.tx_id = t.id ORDER BY g.rowid LIMIT 1) {dir}, t.date DESC, t.id DESC"),
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

    async fn hydrate(&self, rows: Vec<sqlx::sqlite::SqliteRow>) -> Result<Vec<Transaction>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<i64> = rows.iter().map(|r| r.get("id")).collect();
        let in_list = |qb: &mut QueryBuilder<Sqlite>| {
            qb.push(" IN (");
            let mut sep = qb.separated(", ");
            for id in &ids {
                sep.push_bind(*id);
            }
            qb.push(")");
        };
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT tx_id, tag FROM tx_tags WHERE tx_id");
        in_list(&mut qb);
        qb.push(" ORDER BY rowid");
        for r in qb.build().fetch_all(&self.pool).await? {
            tags.entry(r.get(0)).or_default().push(r.get(1));
        }
        let mut files: HashMap<i64, Vec<Attachment>> = HashMap::new();
        let mut qb = QueryBuilder::<Sqlite>::new("SELECT id, tx_id, name, size FROM attachments WHERE tx_id");
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
        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT t.id, t.account_id, t.kind, t.amount, t.date, t.description, t.note, t.transfer_id, t.created_by, \
             (SELECT t2.account_id FROM transactions t2 WHERE t.transfer_id IS NOT NULL AND t2.transfer_id = t.transfer_id AND t2.id <> t.id LIMIT 1) AS counterpart_id, \
             (SELECT name FROM users u WHERE u.id = t.created_by) AS by_name, \
             (SELECT initials FROM users u WHERE u.id = t.created_by) AS by_initials \
             FROM transactions t JOIN accounts a ON a.id = t.account_id WHERE ",
        );
        qb.push(visible(c.user_id)).push(" AND t.id = ").push_bind(id);
        let rows = qb.build().fetch_all(&self.pool).await?;
        self.hydrate(rows).await?.into_iter().next().ok_or(Error::NotFound("transaction"))
    }

    async fn set_tags(&self, tx: i64, tags: &[String]) -> Result<()> {
        sqlx::query("DELETE FROM tx_tags WHERE tx_id = ?").bind(tx).execute(&self.pool).await?;
        for t in tags {
            sqlx::query("INSERT OR IGNORE INTO tx_tags (tx_id, tag) VALUES (?, ?)").bind(tx).bind(t).execute(&self.pool).await?;
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
        let date = date_or_today(b.date.as_deref())?;
        let id = sqlx::query("INSERT INTO transactions (account_id, kind, amount, date, description, note, created_by) VALUES (?,?,?,?,?,?,?)")
            .bind(b.account_id)
            .bind(b.kind.as_str())
            .bind(signed)
            .bind(&date)
            .bind(clean(&b.description))
            .bind(clean(&b.note))
            .bind(c.user_id)
            .execute(&self.pool)
            .await?
            .last_insert_rowid();
        // the first tag groups it in insights, so there is always one
        let mut tags = norm_tags(&b.tags);
        if tags.is_empty() {
            tags.push(if signed > 0 { "income".into() } else { "other".into() });
        }
        self.set_tags(id, &tags).await?;
        self.tell_others(c, &account, signed, &b.description).await?;
        self.transaction(c, id).await
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
        let to = self.account_unchecked(c.user_id, b.to_account_id).await?;
        let date = date_or_today(b.date.as_deref())?;
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
        let desc = if clean(&b.description).is_empty() { auto } else { clean(&b.description) };
        let back = format!("from {}", from.name);
        let mut ids = Vec::new();
        for (acct, amt, d) in [(from.id, -amount, desc.clone()), (to.id, amount, if clean(&b.description).is_empty() { back } else { desc })] {
            let id = sqlx::query("INSERT INTO transactions (account_id, kind, amount, date, description, note, created_by) VALUES (?, 'transfer', ?, ?, ?, ?, ?)")
                .bind(acct)
                .bind(amt)
                .bind(&date)
                .bind(d)
                .bind(clean(&b.note))
                .bind(c.user_id)
                .execute(&self.pool)
                .await?
                .last_insert_rowid();
            ids.push(id);
        }
        for id in &ids {
            sqlx::query("UPDATE transactions SET transfer_id = ? WHERE id = ?").bind(ids[0]).bind(id).execute(&self.pool).await?;
            self.set_tags(*id, &tags).await?;
        }
        self.tell_others(c, &to, amount, &format!("transfer from {}", from.name)).await?;
        let mut out = Vec::new();
        for id in ids {
            out.push(self.transaction(c, id).await?);
        }
        Ok(out)
    }

    /// The ids sharing a transfer with `id`, including `id`.
    async fn legs(&self, id: i64, transfer_id: Option<i64>) -> Result<Vec<i64>> {
        Ok(match transfer_id {
            None => vec![id],
            Some(t) => sqlx::query("SELECT id FROM transactions WHERE transfer_id = ?").bind(t).fetch_all(&self.pool).await?.iter().map(|r| r.get(0)).collect(),
        })
    }

    pub async fn update_transaction(&self, c: &Caller, id: i64, b: UpdateTransaction) -> Result<Transaction> {
        c.need("edit")?;
        let tx = self.transaction(c, id).await?;
        self.owned_account(c, tx.account_id).await?;
        let legs = self.legs(id, tx.transfer_id).await?;
        if let Some(a) = b.amount {
            let m = magnitude(a)?;
            for leg in &legs {
                let sign: i64 = sqlx::query("SELECT amount FROM transactions WHERE id = ?").bind(leg).fetch_one(&self.pool).await?.get::<i64, _>(0).signum();
                sqlx::query("UPDATE transactions SET amount = ? WHERE id = ?").bind(sign * m).bind(leg).execute(&self.pool).await?;
            }
        }
        if let Some(d) = &b.date {
            let d = date_or_today(Some(d))?;
            for leg in &legs {
                sqlx::query("UPDATE transactions SET date = ? WHERE id = ?").bind(&d).bind(leg).execute(&self.pool).await?;
            }
        }
        for leg in &legs {
            if let Some(d) = &b.description {
                sqlx::query("UPDATE transactions SET description = ? WHERE id = ?").bind(clean(d)).bind(leg).execute(&self.pool).await?;
            }
            if let Some(n) = &b.note {
                sqlx::query("UPDATE transactions SET note = ? WHERE id = ?").bind(clean(n)).bind(leg).execute(&self.pool).await?;
            }
            if let Some(t) = &b.tags {
                self.set_tags(*leg, &norm_tags(t)).await?;
            }
            sqlx::query("UPDATE transactions SET updated_at = datetime('now') WHERE id = ?").bind(leg).execute(&self.pool).await?;
        }
        if let Some(kind) = b.kind.filter(|k| *k != tx.kind) {
            if tx.transfer_id.is_some() || kind == TxKind::Transfer {
                return Err(Error::bad("a transfer cannot become money in or out, or the other way; delete it and add a new one"));
            }
            sqlx::query("UPDATE transactions SET kind = ?, amount = CASE WHEN ? = 'debit' THEN -ABS(amount) ELSE ABS(amount) END WHERE id = ?")
                .bind(kind.as_str())
                .bind(kind.as_str())
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        if let Some(acct) = b.account_id.filter(|a| *a != tx.account_id) {
            if tx.transfer_id.is_some() {
                return Err(Error::bad("a transfer cannot be moved to another account; delete it and add a new one"));
            }
            self.owned_account(c, acct).await?;
            sqlx::query("UPDATE transactions SET account_id = ? WHERE id = ?").bind(acct).bind(id).execute(&self.pool).await?;
        }
        self.transaction(c, id).await
    }

    /// Delete a transaction (both legs of a transfer). Returns how many rows went.
    pub async fn delete_transaction(&self, c: &Caller, id: i64) -> Result<u64> {
        c.need("edit")?;
        let tx = self.transaction(c, id).await?;
        self.owned_account(c, tx.account_id).await?;
        let mut n = 0;
        for leg in self.legs(id, tx.transfer_id).await? {
            n += sqlx::query("DELETE FROM transactions WHERE id = ?").bind(leg).execute(&self.pool).await?.rows_affected();
        }
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
        fn cell(s: &str) -> String {
            if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
        }
        let accounts = self.accounts(c, true).await?;
        let mut out = String::from("accounts\nname,kind,owners,who sees it,balance\n");
        for a in &accounts {
            let owners = a.owners.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(" and ");
            let vis = if a.joint { "joint" } else { a.visibility.as_str() };
            let bal = if a.kind.is_liability() { -a.balance } else { a.balance };
            out.push_str(&format!("{},{},{},{},{}\n", cell(&a.name), a.kind.as_str(), cell(&owners), vis, tracer_api::money::format_minor(bal)));
        }
        out.push_str("\ntransactions\ndate,description,tags,account,kind,amount,by,note\n");
        let mut offset = 0;
        loop {
            let page = self.transactions(c, TxFilter { limit: Some(500), offset: Some(offset), ..Default::default() }).await?;
            for t in &page.items {
                let acct = accounts.iter().find(|a| a.id == t.account_id).map(|a| a.name.as_str()).unwrap_or("");
                out.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    t.date, cell(&t.description), cell(&t.tags.join(" ")), cell(acct), t.kind.as_str(), tracer_api::money::format_minor(t.amount), cell(&t.created_by.name), cell(&t.note)
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
        let tx = self.transaction(c, tx_id).await?;
        self.owned_account(c, tx.account_id).await?;
        let name: String = name.rsplit(['/', '\\']).next().unwrap_or("file").chars().take(120).collect();
        let size = data.len() as i64;
        let id = sqlx::query("INSERT INTO attachments (tx_id, name, size, mime, data) VALUES (?,?,?,?,?)")
            .bind(tx_id)
            .bind(&name)
            .bind(size)
            .bind(mime)
            .bind(data)
            .execute(&self.pool)
            .await?
            .last_insert_rowid();
        Ok(Attachment { id, name, size })
    }

    /// `(name, mime, bytes)` of an attachment on a transaction the caller can see.
    pub async fn attachment(&self, c: &Caller, id: i64) -> Result<(String, String, Vec<u8>)> {
        c.need("transactions")?;
        let r = sqlx::query("SELECT tx_id, name, mime, data FROM attachments WHERE id = ?").bind(id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound("attachment"))?;
        self.transaction(c, r.get("tx_id")).await?;
        Ok((r.get("name"), r.get("mime"), r.get("data")))
    }

    pub async fn delete_attachment(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        let tx_id: i64 = sqlx::query("SELECT tx_id FROM attachments WHERE id = ?").bind(id).fetch_optional(&self.pool).await?.ok_or(Error::NotFound("attachment"))?.get(0);
        let tx = self.transaction(c, tx_id).await?;
        self.owned_account(c, tx.account_id).await?;
        sqlx::query("DELETE FROM attachments WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }
}
