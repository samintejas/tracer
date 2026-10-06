use chrono::NaiveDate;
use sqlx::Row;

use crate::api::*;
use crate::{Caller, Result, Store};

fn sym(cur: &str) -> &'static str {
    match cur {
        "usd" => "$",
        "eur" => "€",
        _ => "₹",
    }
}

pub(crate) fn show_money(minor: i64, cur: &str) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    format!("{sign}{}{}", sym(cur), tracer_api::money::group_digits(minor, cur == "inr"))
}

fn short_date(d: &str) -> String {
    const M: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    match NaiveDate::parse_from_str(d, "%Y-%m-%d") {
        Ok(n) => format!("{:02} {}", chrono::Datelike::day(&n), M[chrono::Datelike::month0(&n) as usize]),
        Err(_) => d.to_string(),
    }
}

impl Store {
    /// `key` makes a reminder once: a second notification with the same key for the same person is dropped.
    pub(crate) async fn notify(&self, user_id: i64, title: &str, body: &str, link: &str, key: Option<&str>) -> Result<usize> {
        let n = sqlx::query("INSERT INTO notifications (user_id, title, body, link, key) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING")
            .bind(user_id)
            .bind(title)
            .bind(body)
            .bind(link)
            .bind(key)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(n as usize)
    }

    /// Card bills due within three days and emis leaving today become notifications, once each, if the
    /// person wants them. Returns how many were made.
    pub(crate) async fn make_reminders(&self, user_id: i64) -> Result<usize> {
        let me = self.user(user_id).await?;
        if !me.notify_card && !me.notify_emi {
            return Ok(0);
        }
        let now = self.today();
        let ins = self.insights(&Caller::full(user_id), InsightsQuery { member_id: Some(user_id), days: None }).await?;
        let mut made = 0;
        for d in &ins.dues {
            let Ok(date) = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d") else { continue };
            let days = (date - now).num_days();
            let is_loan = ins.loans.iter().any(|a| a.id == d.account_id);
            let key = format!("due:{}:{}", d.account_id, d.date);
            let link = format!("accounts/{}", d.account_id);
            if is_loan && me.notify_emi && days == 0 {
                made += self.notify(user_id, "loan emi leaves today", &format!("{} · {}", d.label, show_money(d.amount, &me.currency)), &link, Some(&key)).await?;
            } else if !is_loan && me.notify_card && (0..=3).contains(&days) {
                let when = match days {
                    0 => "today".to_string(),
                    1 => "in 1 day".to_string(),
                    n => format!("in {n} days"),
                };
                made += self.notify(user_id, &format!("card payment due {when}"), &format!("{} · {} due {}", d.label, show_money(d.amount, &me.currency), short_date(&d.date)), &link, Some(&key)).await?;
            }
        }
        Ok(made)
    }

    pub async fn notifications(&self, c: &Caller) -> Result<Vec<Notification>> {
        c.need("read")?;
        let rows = sqlx::query("SELECT id, title, body, link, read, created_at FROM notifications WHERE user_id = $1 ORDER BY id DESC LIMIT 30")
            .bind(c.user_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| Notification { id: r.get("id"), title: r.get("title"), body: r.get("body"), link: r.get("link"), read: r.get::<i64, _>("read") != 0, created_at: r.get("created_at") })
            .collect())
    }

    /// Mark one notification read, or all of them when `id` is `None`.
    pub async fn mark_notifications_read(&self, c: &Caller, id: Option<i64>) -> Result<()> {
        c.need("read")?;
        sqlx::query("UPDATE notifications SET read = 1 WHERE user_id = $1 AND ($2 IS NULL OR id = $3)")
            .bind(c.user_id)
            .bind(id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
