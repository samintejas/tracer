use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate};
use sqlx::{AssertSqlSafe, Row};

use crate::api::*;
use crate::{Caller, Result, Store, today, visible};

/// Investments and transfers move money without spending it.
const NOT_SPENDING: &str = "investment";

struct Row_ {
    date: NaiveDate,
    amount: i64,
    kind: TxKind,
    category: String,
}

impl Row_ {
    fn is_spend(&self) -> bool {
        self.kind == TxKind::Debit && self.category != NOT_SPENDING
    }

    fn is_income(&self) -> bool {
        self.kind == TxKind::Credit
    }
}

fn next_day(today: NaiveDate, day: u32) -> NaiveDate {
    // the next date on or after today that falls on `day` (clamped to the month's length)
    let clamp = |y: i32, m: u32| {
        let last = (NaiveDate::from_ymd_opt(if m == 12 { y + 1 } else { y }, if m == 12 { 1 } else { m + 1 }, 1).unwrap() - Duration::days(1)).day();
        NaiveDate::from_ymd_opt(y, m, day.min(last)).unwrap()
    };
    let this = clamp(today.year(), today.month());
    if this >= today {
        this
    } else if today.month() == 12 {
        clamp(today.year() + 1, 1)
    } else {
        clamp(today.year(), today.month() + 1)
    }
}

fn month_key(d: NaiveDate) -> String {
    format!("{:04}-{:02}", d.year(), d.month())
}

impl Store {
    async fn flow_rows(&self, user_id: i64, member: Option<i64>, since: NaiveDate) -> Result<Vec<Row_>> {
        let sql = format!(
            "SELECT t.date, t.amount, t.kind, \
             COALESCE((SELECT tag FROM tx_tags g WHERE g.tx_id = t.id ORDER BY g.id LIMIT 1), 'untagged') AS category \
             FROM transactions t JOIN accounts a ON a.id = t.account_id \
             WHERE {} AND t.kind <> 'transfer' AND t.date >= $1 AND ($2 IS NULL OR t.created_by = $2)",
            visible(user_id)
        );
        let rows = sqlx::query(AssertSqlSafe(sql)).bind(since.to_string()).bind(member).fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                Some(Row_ {
                    date: NaiveDate::parse_from_str(&r.get::<String, _>("date"), "%Y-%m-%d").ok()?,
                    amount: r.get("amount"),
                    kind: TxKind::parse(&r.get::<String, _>("kind"))?,
                    category: r.get("category"),
                })
            })
            .collect())
    }

    pub async fn insights(&self, c: &Caller, q: InsightsQuery) -> Result<Insights> {
        c.need("read")?;
        let now = today();
        let window = q.days.unwrap_or(30).clamp(1, 365) as i64;
        let mut accounts = self.accounts(c, false).await?;
        if let Some(m) = q.member_id {
            accounts.retain(|a| a.owners.iter().any(|o| o.id == m));
        }
        let assets = accounts.iter().filter(|a| !a.kind.is_liability()).map(|a| a.balance).sum();
        let owed = accounts.iter().filter(|a| a.kind.is_liability()).map(|a| a.balance).sum();

        let first_month = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap() - Duration::days(1);
        let six_back = (0..5).fold(first_month, |d, _| NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap() - Duration::days(1));
        let since = NaiveDate::from_ymd_opt(six_back.year(), six_back.month(), 1).unwrap();
        let rows = self.flow_rows(c.user_id, q.member_id, since).await?;

        let from = now - Duration::days(window - 1);
        let in_window: Vec<&Row_> = rows.iter().filter(|r| r.date >= from && r.date <= now).collect();
        let income = in_window.iter().filter(|r| r.is_income()).map(|r| r.amount).sum();
        let spending = in_window.iter().filter(|r| r.is_spend()).map(|r| -r.amount).sum();
        let mut cat: BTreeMap<&str, i64> = BTreeMap::new();
        for r in in_window.iter().filter(|r| r.is_spend()) {
            *cat.entry(r.category.as_str()).or_default() += -r.amount;
        }
        let mut categories: Vec<CategoryTotal> = cat.into_iter().map(|(tag, total)| CategoryTotal { tag: tag.into(), total }).collect();
        categories.sort_by(|a, b| b.total.cmp(&a.total).then(a.tag.cmp(&b.tag)));

        let mut months: Vec<MonthFlow> = Vec::new();
        let mut d = NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap();
        for _ in 0..6 {
            months.push(MonthFlow { month: month_key(d), income: 0, spending: 0 });
            d = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap() - Duration::days(1);
        }
        months.reverse();
        for r in &rows {
            if let Some(m) = months.iter_mut().find(|m| m.month == month_key(r.date)) {
                if r.is_income() {
                    m.income += r.amount;
                } else if r.is_spend() {
                    m.spending += -r.amount;
                }
            }
        }

        // four calendar weeks, monday first, ending with this week (days after today are left out)
        let monday = now - Duration::days(now.weekday().num_days_from_monday() as i64 + 21);
        let days: Vec<DayTotal> = (0..=(now - monday).num_days())
            .map(|i| {
                let date = monday + Duration::days(i);
                let total = rows.iter().filter(|r| r.date == date && r.is_spend()).map(|r| -r.amount).sum();
                DayTotal { date: date.to_string(), total }
            })
            .collect();

        let mut dues = Vec::new();
        for a in &accounts {
            match a.kind {
                AccountKind::Credit if a.balance > 0 => {
                    if let Some(day) = a.details.due_day {
                        dues.push(Due { date: next_day(now, day).to_string(), label: a.name.clone(), account_id: a.id, amount: a.balance });
                    }
                }
                AccountKind::Loan => {
                    if let (Some(day), Some(l)) = (a.details.emi_day, &a.loan) {
                        if l.left > 0 {
                            dues.push(Due { date: next_day(now, day).to_string(), label: format!("{} emi", a.name), account_id: a.id, amount: l.emi });
                        }
                    }
                }
                _ => {}
            }
        }
        dues.sort_by(|a, b| a.date.cmp(&b.date));

        Ok(Insights {
            assets,
            owed,
            income,
            spending,
            categories,
            months,
            days,
            dues,
            loans: accounts.iter().filter(|a| a.loan.is_some()).cloned().collect(),
            investments: accounts.iter().filter(|a| a.kind == AccountKind::Investment).cloned().collect(),
            accounts,
        })
    }

    /// "ask tracer": answers a plain question from the caller's own data. Rule based on purpose: it only
    /// states figures it can compute, and says so when it cannot.
    pub async fn ask(&self, c: &Caller, question: &str) -> Result<String> {
        c.need("read")?;
        let q = question.to_lowercase();
        let ins = self.insights(c, InsightsQuery::default()).await?;
        let fmt = |v: i64| format!("₹{}", tracer_api::money::group_digits(v, true));
        let has = |words: &[&str]| words.iter().any(|w| q.contains(w));

        if has(&["loan", "emi"]) && has(&["end", "left", "when", "finish", "emi"]) {
            if ins.loans.is_empty() {
                return Ok("you have no loans set up.".into());
            }
            return Ok(ins
                .loans
                .iter()
                .filter_map(|a| a.loan.as_ref().map(|l| (a, l)))
                .map(|(a, l)| format!("{}: {} months left, last emi {}. emi {}. {} left.", a.name, l.left, l.end, fmt(l.emi), fmt(l.balance)))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        if has(&["due", "bill", "pay next", "upcoming"]) {
            if ins.dues.is_empty() {
                return Ok("nothing is due.".into());
            }
            return Ok(ins.dues.iter().map(|d| format!("{}: {} on {}.", d.label, fmt(d.amount), d.date)).collect::<Vec<_>>().join("\n"));
        }
        if has(&["invest", "sip", "mutual", "portfolio"]) {
            if ins.investments.is_empty() {
                return Ok("you have no investments set up.".into());
            }
            let put: i64 = ins.investments.iter().filter_map(|a| a.details.invested).sum();
            let now: i64 = ins.investments.iter().map(|a| a.balance).sum();
            return Ok(format!(
                "you have put {} into {} investments. they are worth {} now, {} {}.",
                fmt(put),
                ins.investments.len(),
                fmt(now),
                if now >= put { "up" } else { "down" },
                fmt((now - put).abs())
            ));
        }
        if has(&["net worth", "worth", "how much do i have", "total"]) {
            return Ok(format!("you hold {} and owe {}, so you are worth {}.", fmt(ins.assets), fmt(ins.owed), fmt(ins.assets - ins.owed)));
        }
        // "how much did i spend on <tag>"
        let tags = self.tags(c).await?;
        if let Some((tag, _)) = tags.iter().find(|(t, _)| q.contains(t.as_str())) {
            let page = self
                .transactions(c, TxFilter { tags: Some(tag.clone()), from: Some((today() - Duration::days(29)).to_string()), kinds: Some("debit".into()), limit: Some(500), ..Default::default() })
                .await?;
            let total: i64 = page.items.iter().map(|t| -t.amount).sum();
            return Ok(format!(
                "you spent {} on {tag} in the last 30 days, across {} {}.",
                fmt(total),
                page.total,
                if page.total == 1 { "transaction" } else { "transactions" }
            ));
        }
        if has(&["spend", "spent", "expense"]) {
            let top = ins.categories.first().map(|c| format!(" most of it on {} ({}).", c.tag, fmt(c.total))).unwrap_or_default();
            return Ok(format!("you spent {} in the last 30 days.{top}", fmt(ins.spending)));
        }
        if has(&["earn", "income", "salary"]) {
            return Ok(format!("you received {} in the last 30 days.", fmt(ins.income)));
        }
        Ok("i can answer questions about loans, what is due, investments, net worth, and spending by tag, from your own transactions. try: when do my loans end?".into())
    }
}
