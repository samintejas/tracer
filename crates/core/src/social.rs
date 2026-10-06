//! Signing in with another provider (Google, GitHub). The server talks to the provider; the rules for what
//! that sign-in means for an account live here.

use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::api::*;
use crate::auth::{initials_of, UNUSABLE_PASSWORD};
use crate::{Error, Result, Store, clean};

/// Providers a person can sign in with.
pub const PROVIDERS: [&str; 2] = ["google", "github"];

/// Who a provider says someone is, after it has been asked.
#[derive(Debug, Clone)]
pub struct ExternalIdentity {
    pub provider: String,
    /// The provider's own id for the person.
    pub subject: String,
    pub email: String,
    /// The provider vouches that the person controls `email`.
    pub email_verified: bool,
    pub name: String,
}

fn random(prefix: &str, bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    rand::fill(&mut raw[..]);
    format!("{prefix}{}", hex::encode(raw))
}

fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

impl Store {
    /// Start a provider sign-in: remember a one-time `state` and the PKCE `verifier` that goes with it.
    /// Returns `(state, verifier)`.
    pub async fn oauth_start(&self, provider: &str) -> Result<(String, String)> {
        if !PROVIDERS.contains(&provider) {
            return Err(Error::NotFound("provider"));
        }
        sqlx::query("DELETE FROM oauth_flows WHERE created_at < utc_text(now() - interval '10 minutes')").execute(&self.pool).await?;
        let (state, verifier) = (random("", 24), random("", 32));
        sqlx::query("INSERT INTO oauth_flows (state, provider, verifier) VALUES ($1, $2, $3)").bind(&state).bind(provider).bind(&verifier).execute(&self.pool).await?;
        Ok((state, verifier))
    }

    /// The provider came back: use up the `state` and give back its PKCE verifier. Works once, within ten
    /// minutes, for the provider it was made for.
    pub async fn oauth_finish(&self, provider: &str, state: &str) -> Result<String> {
        sqlx::query_scalar::<_, String>(
            "DELETE FROM oauth_flows WHERE state = $1 AND provider = $2 AND created_at > utc_text(now() - interval '10 minutes') RETURNING verifier",
        )
        .bind(state)
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| Error::bad("this sign-in expired or was already used: start again"))
    }

    /// Decide whose account a provider sign-in is, and hand back a one-time code for it.
    ///
    /// - Known (provider, id): that account.
    /// - Otherwise the provider must vouch for the email. If an account has it, the two are linked; that
    ///   proves the person owns the address, which a password sign-up never did, so anything set up under
    ///   that address before is cut off: its sessions end and its password stops working.
    /// - Otherwise a new account is made, if sign-ups are open.
    pub async fn external_sign_in(&self, who: ExternalIdentity) -> Result<String> {
        if !PROVIDERS.contains(&who.provider.as_str()) {
            return Err(Error::NotFound("provider"));
        }
        let mut db = self.pool.begin().await?;
        let known: Option<i64> = sqlx::query_scalar("SELECT user_id FROM identities WHERE provider = $1 AND subject = $2")
            .bind(&who.provider)
            .bind(&who.subject)
            .fetch_optional(&mut *db)
            .await?;
        let user_id = match known {
            Some(id) => id,
            None => {
                if !who.email_verified {
                    return Err(Error::Forbidden(format!("{} has not verified your email address: verify it there and try again", who.provider)));
                }
                let email = crate::auth::check_email(&who.email)?;
                let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE lower(email) = $1 FOR UPDATE").bind(&email).fetch_optional(&mut *db).await?;
                let id = match existing {
                    Some(id) => {
                        sqlx::query("DELETE FROM tokens WHERE user_id = $1 AND kind = 'session'").bind(id).execute(&mut *db).await?;
                        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2 AND password_hash <> $1").bind(UNUSABLE_PASSWORD).bind(id).execute(&mut *db).await?;
                        id
                    }
                    None => {
                        if !self.cfg.signups_open {
                            return Err(Error::Forbidden("sign-ups are closed on this server".into()));
                        }
                        let name = match clean(&who.name).to_lowercase() {
                            n if n.is_empty() => email.split('@').next().unwrap_or("me").to_string(),
                            n => n,
                        };
                        sqlx::query_scalar("INSERT INTO users (name, email, password_hash, initials) VALUES ($1, $2, $3, $4) RETURNING id")
                            .bind(&name)
                            .bind(&email)
                            .bind(UNUSABLE_PASSWORD)
                            .bind(initials_of(&name))
                            .fetch_one(&mut *db)
                            .await?
                    }
                };
                sqlx::query("INSERT INTO identities (user_id, provider, subject, email) VALUES ($1, $2, $3, $4)")
                    .bind(id)
                    .bind(&who.provider)
                    .bind(&who.subject)
                    .bind(&email)
                    .execute(&mut *db)
                    .await?;
                id
            }
        };
        let code = random("lgc_", 32);
        sqlx::query("DELETE FROM login_codes WHERE created_at < utc_text(now() - interval '2 minutes')").execute(&mut *db).await?;
        sqlx::query("INSERT INTO login_codes (code_hash, user_id) VALUES ($1, $2)").bind(hash(&code)).bind(user_id).execute(&mut *db).await?;
        db.commit().await?;
        Ok(code)
    }

    /// Swap a one-time code from a provider sign-in for a session.
    pub async fn redeem_login_code(&self, code: &str) -> Result<Session> {
        let user_id: i64 = sqlx::query("DELETE FROM login_codes WHERE code_hash = $1 AND created_at > utc_text(now() - interval '2 minutes') RETURNING user_id")
            .bind(hash(code.trim()))
            .fetch_optional(&self.pool)
            .await?
            .ok_or(Error::Unauthorized)?
            .get(0);
        self.open_session(user_id).await
    }

    /// The providers this person has signed in with.
    pub async fn identities(&self, user_id: i64) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar("SELECT provider FROM identities WHERE user_id = $1 ORDER BY provider").bind(user_id).fetch_all(&self.pool).await?)
    }
}
