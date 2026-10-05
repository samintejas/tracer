use sqlx::Row;

use crate::api::*;
use crate::{Caller, Result, Store};

impl Store {
    pub(crate) async fn notify(&self, user_id: i64, text: &str) -> Result<()> {
        sqlx::query("INSERT INTO notifications (user_id, text) VALUES (?, ?)").bind(user_id).bind(text).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn notifications(&self, c: &Caller) -> Result<Vec<Notification>> {
        c.need("read")?;
        let rows = sqlx::query("SELECT id, text, read, created_at FROM notifications WHERE user_id = ? ORDER BY id DESC LIMIT 30")
            .bind(c.user_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| Notification { id: r.get("id"), text: r.get("text"), read: r.get::<i64, _>("read") != 0, created_at: r.get("created_at") })
            .collect())
    }

    /// Mark one notification read, or all of them when `id` is `None`.
    pub async fn mark_notifications_read(&self, c: &Caller, id: Option<i64>) -> Result<()> {
        sqlx::query("UPDATE notifications SET read = 1 WHERE user_id = ? AND (? IS NULL OR id = ?)")
            .bind(c.user_id)
            .bind(id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
