use sqlx::Row;

use crate::api::*;
use crate::{Caller, Error, Result, Store, clean};

fn asset_from(r: &sqlx::postgres::PgRow) -> Asset {
    Asset { id: r.get("id"), name: r.get("name"), kind: r.get("kind"), bought: r.get("bought"), cost: r.get("cost"), value: r.get("value"), note: r.get("note"), account: r.get::<Option<i64>, _>("acc_id").map(|id| LinkedAccount { id, name: r.get("acc_name") }) }
}

fn check_kind(k: &str) -> Result<()> {
    if ASSET_KINDS.contains(&k) { Ok(()) } else { Err(Error::bad(format!("kind must be one of: {}", ASSET_KINDS.join(", ")))) }
}

fn check_bought(b: &str) -> Result<()> {
    if b.is_empty() || tracer_api::loan::parse_ym(b).is_some() { Ok(()) } else { Err(Error::bad("bought must be a month as YYYY-MM")) }
}

fn non_negative(v: i64, what: &str) -> Result<i64> {
    if v < 0 { Err(Error::bad(format!("{what} cannot be negative"))) } else { Ok(v) }
}

/// An asset with the investment it stands behind, when there is one.
const SELECT: &str = "SELECT s.*, a.id AS acc_id, a.name AS acc_name FROM assets s LEFT JOIN accounts a ON a.asset_id = s.id";

impl Store {
    /// The things the caller owns outside any account. They are the caller's own: not shared with family.
    pub async fn assets(&self, c: &Caller) -> Result<Vec<Asset>> {
        c.need("read")?;
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} WHERE s.user_id = $1 ORDER BY s.value DESC, s.id"))).bind(c.user_id).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(asset_from).collect())
    }

    async fn asset(&self, c: &Caller, id: i64) -> Result<Asset> {
        let r = sqlx::query(sqlx::AssertSqlSafe(format!("{SELECT} WHERE s.id = $1 AND s.user_id = $2"))).bind(id).bind(c.user_id).fetch_optional(&self.pool).await?;
        r.as_ref().map(asset_from).ok_or(Error::NotFound("asset"))
    }

    pub async fn add_asset(&self, c: &Caller, b: NewAsset) -> Result<Asset> {
        c.need("add")?;
        let name = clean(&b.name).to_lowercase();
        if name.is_empty() {
            return Err(Error::bad("name the asset"));
        }
        check_kind(&b.kind)?;
        let bought = clean(&b.bought);
        check_bought(&bought)?;
        let id: i64 = sqlx::query("INSERT INTO assets (user_id, name, kind, bought, cost, value, note) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
            .bind(c.user_id)
            .bind(&name)
            .bind(&b.kind)
            .bind(&bought)
            .bind(non_negative(b.cost.unwrap_or(0), "price paid")?)
            .bind(non_negative(b.value, "value")?)
            .bind(clean(&b.note))
            .fetch_one(&self.pool)
            .await?
            .get(0);
        self.asset(c, id).await
    }

    pub async fn update_asset(&self, c: &Caller, id: i64, b: UpdateAsset) -> Result<Asset> {
        c.need("edit")?;
        let cur = self.asset(c, id).await?;
        let name = match &b.name {
            Some(n) if clean(n).is_empty() => return Err(Error::bad("name the asset")),
            Some(n) => clean(n).to_lowercase(),
            None => cur.name,
        };
        let kind = b.kind.unwrap_or(cur.kind);
        check_kind(&kind)?;
        let bought = b.bought.map(|s| clean(&s)).unwrap_or(cur.bought);
        check_bought(&bought)?;
        sqlx::query("UPDATE assets SET name=$1, kind=$2, bought=$3, cost=$4, value=$5, note=$6 WHERE id=$7")
            .bind(&name)
            .bind(&kind)
            .bind(&bought)
            .bind(non_negative(b.cost.unwrap_or(cur.cost), "price paid")?)
            .bind(non_negative(b.value.unwrap_or(cur.value), "value")?)
            .bind(b.note.map(|n| clean(&n)).unwrap_or(cur.note))
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.asset(c, id).await
    }

    pub async fn delete_asset(&self, c: &Caller, id: i64) -> Result<()> {
        c.need("edit")?;
        let asset = self.asset(c, id).await?;
        let mut db = self.pool.begin().await?;
        // an investment that took its value from this asset keeps that value, so its balance does not jump
        sqlx::query(
            "UPDATE accounts SET opening = $1 - (SELECT COALESCE(SUM(amount), 0) FROM transactions WHERE account_id = accounts.id), asset_id = NULL WHERE asset_id = $2",
        )
        .bind(asset.value)
        .bind(id)
        .execute(&mut *db)
        .await?;
        sqlx::query("DELETE FROM assets WHERE id = $1").bind(id).execute(&mut *db).await?;
        db.commit().await?;
        Ok(())
    }

    /// What the caller's things are worth together. A thing that is the value of an investment is left out:
    /// that account already counts it.
    pub(crate) async fn things_value(&self, user_id: i64) -> Result<i64> {
        Ok(sqlx::query("SELECT COALESCE(SUM(s.value), 0)::BIGINT FROM assets s WHERE s.user_id = $1 AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.asset_id = s.id AND a.archived = 0)").bind(user_id).fetch_one(&self.pool).await?.get(0))
    }
}
