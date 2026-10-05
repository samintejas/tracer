//! tracer's business logic. Every front end (REST, MCP, CLI) calls these functions and nothing else touches
//! the database, so a rule lives in exactly one place.

mod accounts;
mod auth;
mod error;
mod family;
mod insights;
mod notify;
mod transactions;

use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

pub use error::{Error, Result};
pub use tracer_api as api;

/// The database and every operation on it.
#[derive(Clone)]
pub struct Store {
    pub(crate) pool: SqlitePool,
}

/// Who is acting and what they may do. A web session may do anything; a connector token only its scopes.
#[derive(Debug, Clone)]
pub struct Caller {
    pub user_id: i64,
    scopes: Vec<String>,
}

impl Caller {
    /// Everything: the web app, and the CLI acting as a person.
    pub fn full(user_id: i64) -> Self {
        Caller { user_id, scopes: api::SCOPES.iter().map(|s| s.to_string()).collect() }
    }

    pub fn scoped(user_id: i64, scopes: Vec<String>) -> Self {
        Caller { user_id, scopes }
    }

    pub fn can(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    pub fn need(&self, scope: &str) -> Result<()> {
        if self.can(scope) {
            Ok(())
        } else {
            Err(Error::Forbidden(format!("this token does not have the '{scope}' scope")))
        }
    }

    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

impl Store {
    /// Open (creating if missing) a SQLite file or `sqlite::memory:` and run migrations.
    pub async fn open(url: &str) -> Result<Store> {
        let opts = SqliteConnectOptions::from_str(url)
            .map_err(|e| Error::bad(format!("bad database url: {e}")))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let max = if url.contains(":memory:") { 1 } else { 5 };
        let pool = SqlitePoolOptions::new().max_connections(max).connect_with(opts).await?;
        sqlx::migrate!("./migrations").run(&pool).await.map_err(|e| Error::Internal(format!("migration failed: {e}")))?;
        Ok(Store { pool })
    }

    pub async fn memory() -> Result<Store> {
        Store::open("sqlite::memory:").await
    }
}

pub(crate) fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

pub(crate) fn clean(s: &str) -> String {
    s.trim().to_string()
}

pub(crate) fn tags(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in raw.iter().flat_map(|t| t.split(',')) {
        let t = t.trim().to_lowercase();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

pub(crate) fn date_or_today(d: Option<&str>) -> Result<String> {
    match d.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(today().to_string()),
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(|d| d.to_string())
            .map_err(|_| Error::bad(format!("invalid date '{s}', expected YYYY-MM-DD"))),
    }
}

/// SQL: accounts (alias `a`) that `user` can see: their own, plus `shared` ones owned by family members.
/// The id is an integer formatted into the text, so there is nothing to inject and no placeholder to mix.
pub(crate) fn visible(user: i64) -> String {
    VISIBLE.replace("{U}", &user.to_string())
}

const VISIBLE: &str = "(EXISTS (SELECT 1 FROM account_owners o WHERE o.account_id = a.id AND o.user_id = {U}) \
 OR (a.visibility = 'shared' AND EXISTS (SELECT 1 FROM account_owners o \
     JOIN family_members f1 ON f1.user_id = o.user_id \
     JOIN family_members f2 ON f2.family_id = f1.family_id \
     WHERE o.account_id = a.id AND f2.user_id = {U})))";

