use crate::{Result, Store};

/// What one run of the background job did.
#[derive(Debug, Default, Clone, Copy)]
pub struct JobReport {
    pub renewals: usize,
    pub reminders: usize,
    pub pruned: u64,
}

impl Store {
    /// The work nobody asks for: post subscription renewals, make reminders, and tidy up what has expired.
    /// Safe to run as often as you like and from more than one place: every step is once-only.
    pub async fn run_jobs(&self) -> Result<JobReport> {
        let mut report = JobReport { renewals: self.post_due_all().await?, ..Default::default() };
        let people: Vec<i64> = sqlx::query_scalar("SELECT id FROM users WHERE notify_card = 1 OR notify_emi = 1").fetch_all(&self.pool).await?;
        for id in people {
            // one person's trouble should not stop everyone else's reminders
            match self.make_reminders(id).await {
                Ok(n) => report.reminders += n,
                Err(e) => tracing::warn!("reminders for user {id}: {e}"),
            }
        }
        report.pruned = self.prune().await?;
        Ok(report)
    }

    async fn prune(&self) -> Result<u64> {
        let mut n = 0;
        n += sqlx::query(
            "DELETE FROM tokens WHERE kind = 'session' AND (created_at < utc_text(now() - interval '180 days') \
             OR COALESCE(last_used_at, created_at) < utc_text(now() - interval '30 days'))",
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        n += sqlx::query("DELETE FROM password_resets WHERE created_at < utc_text(now() - interval '1 hour')").execute(&self.pool).await?.rows_affected();
        n += sqlx::query("DELETE FROM notifications WHERE created_at < utc_text(now() - interval '90 days')").execute(&self.pool).await?.rows_affected();
        n += sqlx::query("UPDATE families SET invite_code = NULL, invite_expires = NULL WHERE invite_expires IS NOT NULL AND invite_expires < utc_now()")
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(n)
    }
}
