use sqlx::Row;

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean};

fn invite_code() -> String {
    // no 0/O/1/I so a code read aloud is not ambiguous
    const A: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
    let mut raw = [0u8; 8];
    rand::fill(&mut raw);
    let c: Vec<char> = raw.iter().map(|b| A[*b as usize % A.len()] as char).collect();
    format!("{}-{}", c[..4].iter().collect::<String>(), c[4..].iter().collect::<String>())
}

impl Store {
    pub(crate) async fn family(&self, user_id: i64) -> Result<Option<Family>> {
        let Some(f) = sqlx::query("SELECT f.id, f.name, f.invite_code FROM families f JOIN family_members m ON m.family_id = f.id WHERE m.user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        let id: i64 = f.get("id");
        let members = sqlx::query("SELECT u.id, u.name, u.initials FROM users u JOIN family_members m ON m.user_id = u.id WHERE m.family_id = ? ORDER BY u.id")
            .bind(id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| Member { id: r.get("id"), name: r.get("name"), initials: r.get("initials") })
            .collect();
        Ok(Some(Family { id, name: f.get("name"), invite_code: f.get("invite_code"), members }))
    }

    pub async fn create_family(&self, c: &Caller, b: NewFamily) -> Result<Family> {
        c.need("edit")?;
        let name = clean(&b.name).to_lowercase();
        if name.is_empty() {
            return Err(Error::bad("name your family"));
        }
        if self.family(c.user_id).await?.is_some() {
            return Err(Error::Conflict("you are already in a family; leave it first".into()));
        }
        let id = sqlx::query("INSERT INTO families (name, invite_code) VALUES (?, ?)")
            .bind(&name)
            .bind(invite_code())
            .execute(&self.pool)
            .await?
            .last_insert_rowid();
        sqlx::query("INSERT INTO family_members (family_id, user_id) VALUES (?, ?)").bind(id).bind(c.user_id).execute(&self.pool).await?;
        self.family(c.user_id).await?.ok_or(Error::NotFound("family"))
    }

    pub async fn join_family(&self, c: &Caller, b: JoinFamily) -> Result<Family> {
        c.need("edit")?;
        if self.family(c.user_id).await?.is_some() {
            return Err(Error::Conflict("you are already in a family; leave it first".into()));
        }
        let code = clean(&b.code).to_uppercase();
        let id: i64 = sqlx::query("SELECT id FROM families WHERE invite_code = ?")
            .bind(&code)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| Error::bad("that invite code does not match a family"))?
            .get(0);
        let n: i64 = sqlx::query("SELECT COUNT(*) FROM family_members WHERE family_id = ?").bind(id).fetch_one(&self.pool).await?.get(0);
        if n >= 6 {
            return Err(Error::Conflict("this family is full".into()));
        }
        sqlx::query("INSERT INTO family_members (family_id, user_id) VALUES (?, ?)").bind(id).bind(c.user_id).execute(&self.pool).await?;
        let me = self.user(c.user_id).await?;
        let others = sqlx::query("SELECT user_id FROM family_members WHERE family_id = ? AND user_id <> ?")
            .bind(id)
            .bind(c.user_id)
            .fetch_all(&self.pool)
            .await?;
        for o in others {
            self.notify(o.get(0), &format!("{} joined your family", me.name)).await?;
        }
        self.family(c.user_id).await?.ok_or(Error::NotFound("family"))
    }

    /// Leave the family. Accounts you own become private again; the family is deleted when the last
    /// person leaves.
    pub async fn leave_family(&self, c: &Caller) -> Result<()> {
        c.need("edit")?;
        let Some(f) = self.family(c.user_id).await? else { return Ok(()) };
        // a joint account stays with the people who are still together: the leaver drops off it
        sqlx::query(
            "DELETE FROM account_owners WHERE user_id = ? AND account_id IN \
             (SELECT account_id FROM account_owners GROUP BY account_id HAVING COUNT(*) > 1)",
        )
        .bind(c.user_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE accounts SET visibility = 'private' WHERE id IN (SELECT account_id FROM account_owners WHERE user_id = ?)")
            .bind(c.user_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM family_members WHERE user_id = ?").bind(c.user_id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM families WHERE id = ? AND NOT EXISTS (SELECT 1 FROM family_members WHERE family_id = ?)")
            .bind(f.id)
            .bind(f.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
