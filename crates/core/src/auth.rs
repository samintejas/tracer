use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, Row};

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean};

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_token(prefix: &str) -> String {
    let mut raw = [0u8; 32];
    rand::fill(&mut raw);
    format!("{prefix}_{}", hex::encode(raw))
}

fn hash_password(pw: &str) -> Result<String> {
    let mut salt_bytes = [0u8; 16];
    rand::fill(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| Error::Internal(e.to_string()))?;
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::Internal(e.to_string()))
}

fn verify_password(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok()).unwrap_or(false)
}

pub(crate) fn initials_of(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let s: String = match words.as_slice() {
        [] => "?".into(),
        [one] => one.chars().take(2).collect(),
        [first, .., last] => format!("{}{}", first.chars().next().unwrap(), last.chars().next().unwrap()),
    };
    s.to_lowercase()
}

fn user_from(r: &sqlx::postgres::PgRow) -> User {
    User {
        id: r.get("id"),
        name: r.get("name"),
        email: r.get("email"),
        initials: r.get("initials"),
        phone: r.get("phone"),
        currency: r.get("currency"),
        picture: r.get("picture"),
        notify_card: r.get::<i64, _>("notify_card") != 0,
        notify_emi: r.get::<i64, _>("notify_emi") != 0,
        notify_joint: r.get::<i64, _>("notify_joint") != 0,
    }
}

const USER_COLS: &str = "id, name, email, initials, phone, currency, picture, notify_card, notify_emi, notify_joint";

/// Passwords are at least this long.
pub const MIN_PASSWORD: usize = 12;

fn check_password(pw: &str) -> Result<()> {
    if pw.chars().count() < MIN_PASSWORD {
        return Err(Error::bad(format!("password needs {MIN_PASSWORD} characters or more")));
    }
    Ok(())
}

fn check_email(e: &str) -> Result<String> {
    let e = clean(e).to_lowercase();
    match e.split_once('@') {
        Some((l, d)) if !l.is_empty() && d.contains('.') && !e.contains(' ') => Ok(e),
        _ => Err(Error::bad("enter a valid email")),
    }
}

impl Store {
    pub async fn sign_up(&self, b: SignUp) -> Result<Session> {
        let name = clean(&b.name).to_lowercase();
        if name.is_empty() {
            return Err(Error::bad("enter your name"));
        }
        let email = check_email(&b.email)?;
        check_password(&b.password)?;
        let hash = hash_password(&b.password)?;
        let id = sqlx::query("INSERT INTO users (name, email, password_hash, initials) VALUES ($1, $2, $3, $4) RETURNING id")
            .bind(&name)
            .bind(&email)
            .bind(&hash)
            .bind(initials_of(&name))
            .fetch_one(&self.pool)
            .await
            .map_err(|e| match Error::from(e) {
                Error::Conflict(_) => Error::Conflict("an account with this email already exists".into()),
                other => other,
            })?
            .get::<i64, _>(0);
        self.open_session(id).await
    }

    pub async fn sign_in(&self, b: SignIn) -> Result<Session> {
        let row = sqlx::query("SELECT id, password_hash FROM users WHERE lower(email) = lower($1)")
            .bind(clean(&b.email))
            .fetch_optional(&self.pool)
            .await?;
        let ok = match &row {
            Some(r) => verify_password(&b.password, r.get::<String, _>("password_hash").as_str()),
            None => {
                // spend the same time as a real check so timing does not reveal which emails exist
                let _ = verify_password(&b.password, "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
                false
            }
        };
        match (ok, row) {
            (true, Some(r)) => self.open_session(r.get("id")).await,
            _ => Err(Error::Unauthorized),
        }
    }

    async fn open_session(&self, user_id: i64) -> Result<Session> {
        let token = new_token("trs");
        sqlx::query("INSERT INTO tokens (user_id, kind, token_hash, tail) VALUES ($1, 'session', $2, $3)")
            .bind(user_id)
            .bind(hash_token(&token))
            .bind(&token[token.len() - 4..])
            .execute(&self.pool)
            .await?;
        Ok(Session { token, user: self.user(user_id).await? })
    }

    pub async fn sign_out(&self, token: &str) -> Result<()> {
        sqlx::query("DELETE FROM tokens WHERE token_hash = $1 AND kind = 'session'")
            .bind(hash_token(token))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Resolve a bearer token (session or connector) to who is acting. Sessions expire after 90 days.
    pub async fn authenticate(&self, token: &str) -> Result<Caller> {
        let row = sqlx::query(
            "SELECT id, user_id, kind, scopes FROM tokens WHERE token_hash = $1 \
             AND (kind = 'connector' OR created_at > utc_text(now() - interval '90 days'))",
        )
        .bind(hash_token(token))
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::Unauthorized)?;
        let (id, user_id): (i64, i64) = (row.get("id"), row.get("user_id"));
        let kind: String = row.get("kind");
        if kind == "connector" {
            sqlx::query("UPDATE tokens SET last_used_at = utc_now() WHERE id = $1").bind(id).execute(&self.pool).await?;
            let scopes: String = row.get("scopes");
            Ok(Caller::scoped(user_id, scopes.split(',').filter(|s| !s.is_empty()).map(String::from).collect()))
        } else {
            Ok(Caller::full(user_id))
        }
    }

    pub async fn user(&self, id: i64) -> Result<User> {
        let row = sqlx::query(AssertSqlSafe(format!("SELECT {USER_COLS} FROM users WHERE id = $1")))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(Error::NotFound("user"))?;
        Ok(user_from(&row))
    }

    pub async fn user_by_email(&self, email: &str) -> Result<User> {
        let row = sqlx::query(AssertSqlSafe(format!("SELECT {USER_COLS} FROM users WHERE lower(email) = lower($1)")))
            .bind(clean(email))
            .fetch_optional(&self.pool)
            .await?
            .ok_or(Error::NotFound("user"))?;
        Ok(user_from(&row))
    }

    pub async fn me(&self, c: &Caller) -> Result<Me> {
        Ok(Me { user: self.user(c.user_id).await?, family: self.family(c.user_id).await? })
    }

    pub async fn update_profile(&self, c: &Caller, b: UpdateProfile) -> Result<User> {
        c.need("edit")?;
        let u = self.user(c.user_id).await?;
        let name = b.name.map(|n| clean(&n).to_lowercase()).unwrap_or(u.name.clone());
        if name.is_empty() {
            return Err(Error::bad("enter your name"));
        }
        let initials = b.initials.map(|i| clean(&i).to_lowercase()).filter(|i| !i.is_empty()).unwrap_or_else(|| {
            if name != u.name { initials_of(&name) } else { u.initials.clone() }
        });
        let email = match b.email {
            Some(e) => check_email(&e)?,
            None => u.email.clone(),
        };
        let currency = b.currency.unwrap_or(u.currency.clone());
        if !["inr", "usd", "eur"].contains(&currency.as_str()) {
            return Err(Error::bad("currency must be inr, usd or eur"));
        }
        let picture = match b.picture {
            None => u.picture.clone(),
            Some(p) if p.is_empty() => None,
            Some(p) => {
                if !p.starts_with("data:image/") {
                    return Err(Error::bad("the picture must be an image"));
                }
                if p.len() > 400_000 {
                    return Err(Error::bad("the picture is too large: use one under 250 kb"));
                }
                Some(p)
            }
        };
        sqlx::query("UPDATE users SET name = $1, initials = $2, email = $3, phone = $4, currency = $5, picture = $6, notify_card = $7, notify_emi = $8, notify_joint = $9 WHERE id = $10")
            .bind(name)
            .bind(initials.chars().take(2).collect::<String>())
            .bind(email)
            .bind(b.phone.map(|p| clean(&p)).unwrap_or(u.phone))
            .bind(currency)
            .bind(picture)
            .bind(b.notify_card.unwrap_or(u.notify_card) as i64)
            .bind(b.notify_emi.unwrap_or(u.notify_emi) as i64)
            .bind(b.notify_joint.unwrap_or(u.notify_joint) as i64)
            .bind(c.user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| match Error::from(e) {
                Error::Conflict(_) => Error::Conflict("another account uses this email".into()),
                o => o,
            })?;
        self.user(c.user_id).await
    }

    pub async fn change_password(&self, c: &Caller, b: ChangePassword) -> Result<()> {
        c.need("edit")?;
        check_password(&b.new)?;
        let hash: String = sqlx::query("SELECT password_hash FROM users WHERE id = $1")
            .bind(c.user_id)
            .fetch_one(&self.pool)
            .await?
            .get(0);
        if !verify_password(&b.current, &hash) {
            return Err(Error::Forbidden("current password is wrong".into()));
        }
        self.set_password(c.user_id, &b.new).await
    }

    /// Set a password directly. For the CLI (there is no email to send a reset link from).
    pub async fn set_password(&self, user_id: i64, password: &str) -> Result<()> {
        check_password(password)?;
        sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(hash_password(password)?)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        // sign out everywhere else
        sqlx::query("DELETE FROM tokens WHERE user_id = $1 AND kind = 'session'").bind(user_id).execute(&self.pool).await?;
        Ok(())
    }

    // ---- connectors: api tokens for Claude, scripts and the CLI -------------------------------------

    pub async fn create_connector(&self, c: &Caller, b: NewConnector) -> Result<CreatedConnector> {
        c.need("edit")?;
        let name = clean(&b.name);
        if name.is_empty() {
            return Err(Error::bad("name the connector"));
        }
        let scopes: Vec<String> = if b.scopes.is_empty() { vec!["read".into(), "transactions".into()] } else { b.scopes };
        if let Some(bad) = scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
            return Err(Error::bad(format!("unknown scope '{bad}', use one of {SCOPES:?}")));
        }
        let token = new_token("trc");
        let id = sqlx::query("INSERT INTO tokens (user_id, kind, name, token_hash, tail, scopes) VALUES ($1, 'connector', $2, $3, $4, $5) RETURNING id")
            .bind(c.user_id)
            .bind(&name)
            .bind(hash_token(&token))
            .bind(&token[token.len() - 4..])
            .bind(scopes.join(","))
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>(0);
        let connector = self.connectors(c).await?.into_iter().find(|x| x.id == id).ok_or(Error::NotFound("connector"))?;
        Ok(CreatedConnector { token, connector })
    }

    pub async fn connectors(&self, c: &Caller) -> Result<Vec<Connector>> {
        let rows = sqlx::query("SELECT id, name, scopes, tail, created_at, last_used_at FROM tokens WHERE user_id = $1 AND kind = 'connector' ORDER BY id")
            .bind(c.user_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| Connector {
                id: r.get("id"),
                name: r.get("name"),
                scopes: r.get::<String, _>("scopes").split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
                tail: r.get("tail"),
                created_at: r.get("created_at"),
                last_used_at: r.get("last_used_at"),
            })
            .collect())
    }

    /// Change what a connector token may do, without making a new token.
    pub async fn update_connector(&self, c: &Caller, id: i64, b: UpdateConnector) -> Result<Connector> {
        c.need("edit")?;
        if let Some(bad) = b.scopes.iter().find(|s| !SCOPES.contains(&s.as_str())) {
            return Err(Error::bad(format!("unknown scope '{bad}', use one of {SCOPES:?}")));
        }
        let n = sqlx::query("UPDATE tokens SET scopes = $1 WHERE id = $2 AND user_id = $3 AND kind = 'connector'")
            .bind(b.scopes.join(","))
            .bind(id)
            .bind(c.user_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if n == 0 {
            return Err(Error::NotFound("connector"));
        }
        self.connectors(c).await?.into_iter().find(|x| x.id == id).ok_or(Error::NotFound("connector"))
    }

    /// Sign out everywhere, this device included. Connector tokens keep working.
    pub async fn sign_out_all(&self, c: &Caller) -> Result<()> {
        sqlx::query("DELETE FROM tokens WHERE user_id = $1 AND kind = 'session'").bind(c.user_id).execute(&self.pool).await?;
        Ok(())
    }

    /// Delete the caller: their sign-in, the accounts only they own (with their transactions), and their
    /// place in a family. Joint accounts stay with the other owners. Needs the password. Cannot be undone.
    pub async fn delete_user(&self, c: &Caller, password: &str) -> Result<()> {
        c.need("edit")?;
        let hash: String = sqlx::query("SELECT password_hash FROM users WHERE id = $1").bind(c.user_id).fetch_one(&self.pool).await?.get(0);
        if !verify_password(password, &hash) {
            return Err(Error::Forbidden("password is wrong".into()));
        }
        self.leave_family(c).await?;
        // what is left is owned by this person alone
        let mine: Vec<i64> = sqlx::query("SELECT account_id FROM account_owners WHERE user_id = $1").bind(c.user_id).fetch_all(&self.pool).await?.iter().map(|r| r.get(0)).collect();
        for id in mine {
            self.remove_account(id).await?;
        }
        // their entries on accounts they no longer own are kept, under an owner of that account
        sqlx::query(
            "UPDATE transactions SET created_by = (SELECT o.user_id FROM account_owners o WHERE o.account_id = transactions.account_id LIMIT 1) WHERE created_by = $1",
        )
        .bind(c.user_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM users WHERE id = $1").bind(c.user_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn revoke_connector(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        let n = sqlx::query("DELETE FROM tokens WHERE id = $1 AND user_id = $2 AND kind = 'connector'")
            .bind(id)
            .bind(c.user_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if n == 0 { Err(Error::NotFound("connector")) } else { Ok(()) }
    }
}
