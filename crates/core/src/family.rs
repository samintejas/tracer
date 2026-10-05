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
        let Some(f) = sqlx::query("SELECT f.id, f.name, f.owner_id, f.invite_code FROM families f JOIN family_members m ON m.family_id = f.id WHERE m.user_id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        let id: i64 = f.get("id");
        let owner_id: i64 = f.get("owner_id");
        let members = sqlx::query("SELECT u.id, u.name, u.initials, u.email FROM users u JOIN family_members m ON m.user_id = u.id WHERE m.family_id = $1 ORDER BY u.id = $2 DESC, u.id")
            .bind(id)
            .bind(owner_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| Member { id: r.get("id"), name: r.get("name"), initials: r.get("initials"), email: r.get("email") })
            .collect();
        // only the owner sees the code
        let invite_code = if owner_id == user_id { f.get("invite_code") } else { None };
        Ok(Some(Family { id, name: f.get("name"), owner_id, invite_code, members }))
    }

    pub async fn create_family(&self, c: &Caller, b: NewFamily) -> Result<Family> {
        c.need("edit")?;
        let name = clean(&b.name).to_lowercase();
        if name.is_empty() {
            return Err(Error::bad("name your family"));
        }
        if self.family(c.user_id).await?.is_some() {
            return Err(Error::Conflict("you are already in a family".into()));
        }
        let id = sqlx::query("INSERT INTO families (name, owner_id) VALUES ($1, $2) RETURNING id").bind(&name).bind(c.user_id).fetch_one(&self.pool).await?.get::<i64, _>(0);
        sqlx::query("INSERT INTO family_members (family_id, user_id) VALUES ($1, $2)").bind(id).bind(c.user_id).execute(&self.pool).await?;
        self.family(c.user_id).await?.ok_or(Error::NotFound("family"))
    }

    /// Make a one-time invite code. A new code replaces the last unused one. Owner only.
    pub async fn generate_invite(&self, c: &Caller) -> Result<Family> {
        c.need("edit")?;
        let f = self.family(c.user_id).await?.ok_or(Error::NotFound("family"))?;
        if f.owner_id != c.user_id {
            return Err(Error::Forbidden("only the family owner can invite".into()));
        }
        if f.members.len() >= 6 {
            return Err(Error::Conflict("this family is full".into()));
        }
        sqlx::query("UPDATE families SET invite_code = $1 WHERE id = $2").bind(invite_code()).bind(f.id).execute(&self.pool).await?;
        self.family(c.user_id).await?.ok_or(Error::NotFound("family"))
    }

    pub async fn join_family(&self, c: &Caller, b: JoinFamily) -> Result<Family> {
        c.need("edit")?;
        if self.family(c.user_id).await?.is_some() {
            return Err(Error::Conflict("you are already in a family".into()));
        }
        let code = clean(&b.code).to_uppercase();
        if code.is_empty() {
            return Err(Error::bad("enter the invite code"));
        }
        let id: i64 = sqlx::query("SELECT id FROM families WHERE invite_code = $1")
            .bind(&code)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| Error::bad("that code does not match a family, or it was already used"))?
            .get(0);
        sqlx::query("INSERT INTO family_members (family_id, user_id) VALUES ($1, $2)").bind(id).bind(c.user_id).execute(&self.pool).await?;
        // each code works once
        sqlx::query("UPDATE families SET invite_code = NULL WHERE id = $1").bind(id).execute(&self.pool).await?;
        let me = self.user(c.user_id).await?;
        let f = self.family(c.user_id).await?.ok_or(Error::NotFound("family"))?;
        for m in f.members.iter().filter(|m| m.id != c.user_id) {
            self.notify(m.id, &format!("{} joined {}", me.name, f.name), "shared and joint accounts are now visible to both of you", "settings/family", None).await?;
        }
        Ok(f)
    }

    async fn drop_member(&self, user_id: i64) -> Result<()> {
        // a joint account stays with the people who are still together: the leaver drops off it
        sqlx::query(
            "DELETE FROM account_owners WHERE user_id = $1 AND account_id IN \
             (SELECT account_id FROM account_owners GROUP BY account_id HAVING COUNT(*) > 1)",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE accounts SET visibility = 'private' WHERE id IN (SELECT account_id FROM account_owners WHERE user_id = $1)")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM family_members WHERE user_id = $1").bind(user_id).execute(&self.pool).await?;
        Ok(())
    }

    /// Leave the family. Your accounts become private again and you drop off joint ones. If the owner
    /// leaves, the longest-standing member becomes owner; the family goes when the last person does.
    pub async fn leave_family(&self, c: &Caller) -> Result<()> {
        c.need("edit")?;
        let Some(f) = self.family(c.user_id).await? else { return Ok(()) };
        self.drop_member(c.user_id).await?;
        match f.members.iter().find(|m| m.id != c.user_id) {
            None => {
                sqlx::query("DELETE FROM families WHERE id = $1").bind(f.id).execute(&self.pool).await?;
            }
            Some(next) if f.owner_id == c.user_id => {
                sqlx::query("UPDATE families SET owner_id = $1 WHERE id = $2").bind(next.id).bind(f.id).execute(&self.pool).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Delete the family: everyone goes back to a personal account and keeps what they own. Shared access
    /// ends. Owner only.
    pub async fn delete_family(&self, c: &Caller) -> Result<()> {
        c.need("edit")?;
        let f = self.family(c.user_id).await?.ok_or(Error::NotFound("family"))?;
        if f.owner_id != c.user_id {
            return Err(Error::Forbidden("only the family owner can delete it".into()));
        }
        sqlx::query("UPDATE accounts SET visibility = 'private' WHERE id IN (SELECT o.account_id FROM account_owners o JOIN family_members m ON m.user_id = o.user_id WHERE m.family_id = $1)")
            .bind(f.id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM families WHERE id = $1").bind(f.id).execute(&self.pool).await?;
        Ok(())
    }
}
