use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate};
use sqlx::{AssertSqlSafe, Row};

use crate::api::*;
use crate::{Caller, Result, Store, visible};

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
        let now = self.today();
        let window = q.days.unwrap_or(30).clamp(1, 365) as i64;
        let mut accounts = self.accounts(c, false).await?;
        if let Some(m) = q.member_id {
            accounts.retain(|a| a.owners.iter().any(|o| o.id == m));
        }
        // things you own are yours alone: they count in the family view and in yours, not in another member's
        let things = if q.member_id.is_none_or(|m| m == c.user_id) { self.things_value(c.user_id).await? } else { 0 };
        let held: i64 = accounts.iter().filter(|a| !a.kind.is_liability()).map(|a| a.balance).sum();
        let assets = held + things;
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
            things,
            loans: accounts.iter().filter(|a| a.loan.is_some()).cloned().collect(),
            investments: accounts.iter().filter(|a| a.kind == AccountKind::Investment).cloned().collect(),
            accounts,
        })
    }

    /// "ask pebblelab": answers a plain question from the caller's own data. Rule based on purpose: it only
    /// states figures it can compute, and says so when it cannot.
    pub async fn ask(&self, c: &Caller, question: &str) -> Result<String> {
        c.need("read")?;
        let q = question.to_lowercase();
        let ins = self.insights(c, InsightsQuery::default()).await?;
        let cur = self.user(c.user_id).await?.currency;
        let fmt = |v: i64| crate::notify::show_money(v, &cur);
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
        if has(&["matur", "deposit"]) {
            let mut lines: Vec<(String, String)> = ins
                .investments
                .iter()
                .filter_map(|a| a.deposit.as_ref().map(|d| (d.matures.clone(), format!("{}: matures {}, worth about {} then ({} left). worth about {} now.", a.name, d.matures, fmt(d.maturity_value), if d.months_left == 0 { "matured".to_string() } else { format!("{} months", d.months_left) }, fmt(d.value)))))
                .collect();
            if !lines.is_empty() {
                lines.sort();
                return Ok(lines.into_iter().map(|(_, l)| l).collect::<Vec<_>>().join("\n"));
            }
        }
        if has(&["invest", "sip", "mutual", "portfolio", "gold", "stock", "ppf", "epf", "nps"]) {
            if ins.investments.is_empty() {
                return Ok("you have no investments set up.".into());
            }
            // the gain only counts what we know the cost of
            let cost = |a: &Account| a.deposit.as_ref().map(|d| d.invested).or(a.details.invested).filter(|c| *c > 0);
            let known: Vec<&Account> = ins.investments.iter().filter(|a| cost(a).is_some()).collect();
            let put: i64 = known.iter().filter_map(|a| cost(a)).sum();
            let known_now: i64 = known.iter().map(|a| a.balance).sum();
            let now: i64 = ins.investments.iter().map(|a| a.balance).sum();
            let by_type = {
                let mut m: BTreeMap<String, i64> = BTreeMap::new();
                for a in &ins.investments {
                    let t = if a.details.invest_kind.is_empty() { "other" } else { a.details.invest_kind.as_str() };
                    *m.entry(t.to_string()).or_default() += a.balance;
                }
                m.into_iter().map(|(t, v)| format!("{t} {}", fmt(v))).collect::<Vec<_>>().join(", ")
            };
            let gain = if known.is_empty() {
                String::new()
            } else {
                format!(" the ones with a known cost are {} against {} put in, {} {}.", fmt(known_now), fmt(put), if known_now >= put { "up" } else { "down" }, fmt((known_now - put).abs()))
            };
            return Ok(format!("your {} investments are worth {} now: {by_type}.{gain}", ins.investments.len(), fmt(now)));
        }
        if has(&["subscription", "renew"]) {
            let subs: Vec<Subscription> = self.subscriptions(c).await?.into_iter().filter(|s| s.active).collect();
            if subs.is_empty() {
                return Ok("you have no active subscriptions.".into());
            }
            let monthly: i64 = subs.iter().map(|s| if s.cycle == Cycle::Yearly { s.amount / 12 } else { s.amount }).sum();
            let mut lines = vec![format!("{} active subscriptions, about {} a month ({} a year).", subs.len(), fmt(monthly), fmt(monthly * 12))];
            if let Some(n) = subs.iter().filter(|s| s.next.is_some()).min_by(|a, b| a.next.cmp(&b.next)) {
                lines.push(format!("next renewal: {} on {}, {}.", n.name, n.next.clone().unwrap_or_default(), fmt(n.amount)));
            }
            return Ok(lines.join("\n"));
        }
        if has(&["asset", "property", "gold", "vehicle"]) {
            let things = self.assets(c).await?;
            if things.is_empty() {
                return Ok("you have not added any assets.".into());
            }
            let (value, cost): (i64, i64) = things.iter().fold((0, 0), |(v, c), a| (v + a.value, c + a.cost));
            let mut lines = vec![format!("{} assets worth {} now.", things.len(), fmt(value))];
            if cost > 0 {
                lines.push(format!("you paid {}, so they are {} {}.", fmt(cost), if value >= cost { "up" } else { "down" }, fmt((value - cost).abs())));
            }
            return Ok(lines.join("\n"));
        }
        // "how much did i spend on <tag>": the longest tag that appears as whole words in the question
        let words = |t: &str| format!(" {} ", t.chars().map(|ch| if ch.is_alphanumeric() { ch } else { ' ' }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "));
        let padded = words(&q);
        let tags = self.tags(c).await?;
        let found = tags.iter().filter(|(t, _)| t.chars().count() >= 3 && padded.contains(&words(t))).max_by_key(|(t, _)| t.len());
        if let Some((tag, _)) = found {
            let page = self
                .transactions(c, TxFilter { tags: Some(tag.clone()), from: Some((self.today() - Duration::days(29)).to_string()), kinds: Some("debit".into()), limit: Some(500), ..Default::default() })
                .await?;
            let total: i64 = page.items.iter().map(|t| -t.amount).sum();
            return Ok(format!(
                "you spent {} on {tag} in the last 30 days, across {} {}.",
                fmt(total),
                page.total,
                if page.total == 1 { "transaction" } else { "transactions" }
            ));
        }
        if has(&["net worth", "worth", "how much do i have", "total"]) {
            return Ok(format!("you hold {} and owe {}, so you are worth {}.", fmt(ins.assets), fmt(ins.owed), fmt(ins.assets - ins.owed)));
        }
        if has(&["spend", "spent", "expense"]) {
            let top = ins.categories.first().map(|c| format!(" most of it on {} ({}).", c.tag, fmt(c.total))).unwrap_or_default();
            return Ok(format!("you spent {} in the last 30 days.{top}", fmt(ins.spending)));
        }
        if has(&["earn", "income", "salary"]) {
            return Ok(format!("you received {} in the last 30 days.", fmt(ins.income)));
        }
        Ok("i can answer questions about loans, what is due, investments, subscriptions, assets, net worth, and spending by tag, from your own transactions. try: when do my loans end?".into())
    }
}
