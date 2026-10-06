//! Forgotten passwords. The store makes and checks the one-time token; sending it is the server's job.

use sqlx::Row;

use crate::social::{hash, random};
use crate::{Error, Result, Store};

/// Who to write to, and what to put in the link.
pub struct ResetTicket {
    pub name: String,
    pub email: String,
    pub token: String,
}

impl Store {
    /// Make a reset token for the account with this address, or `None` when there is none. The caller must
    /// answer the same either way, so nobody can use this to find out who has an account.
    pub async fn start_reset(&self, email: &str) -> Result<Option<ResetTicket>> {
        let Some(row) = sqlx::query("SELECT id, name, email FROM users WHERE lower(email) = lower($1)").bind(email.trim()).fetch_optional(&self.pool).await? else {
            return Ok(None);
        };
        let token = random("rst_", 32);
        sqlx::query(
            "INSERT INTO password_resets (token_hash, user_id) VALUES ($1, $2) \
             ON CONFLICT (user_id) DO UPDATE SET token_hash = EXCLUDED.token_hash, created_at = utc_now()",
        )
        .bind(hash(&token))
        .bind(row.get::<i64, _>("id"))
        .execute(&self.pool)
        .await?;
        Ok(Some(ResetTicket { name: row.get("name"), email: row.get("email"), token }))
    }

    /// Spend a reset token on a new password. Everyone is signed out afterwards.
    pub async fn finish_reset(&self, token: &str, password: &str) -> Result<()> {
        // a password that is too weak should not use up the link
        crate::auth::check_password(password)?;
        let user_id: i64 = sqlx::query("DELETE FROM password_resets WHERE token_hash = $1 AND created_at > utc_text(now() - interval '1 hour') RETURNING user_id")
            .bind(hash(token.trim()))
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| Error::bad("this link has expired or was already used: ask for a new one"))?
            .get(0);
        self.set_password(user_id, password).await
    }
}
