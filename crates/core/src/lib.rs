//! tracer's business logic. Every front end (REST, MCP, CLI) calls these functions and nothing else touches
//! the database, so a rule lives in exactly one place.

mod accounts;
mod assets;
mod auth;
mod error;
mod family;
mod insights;
mod notify;
mod scheduler;
mod social;
mod subscriptions;
mod transactions;

use std::str::FromStr;
use std::sync::Arc;

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

pub use error::{Error, Result};
pub use scheduler::JobReport;
pub use social::{ExternalIdentity, PROVIDERS};
pub use transactions::INLINE_TYPES;
pub use tracer_api as api;

/// Settings that change how rules behave. The defaults suit a first run.
#[derive(Debug, Clone)]
pub struct Config {
    /// Whose "today" it is: dates a person leaves blank, renewals and reminders follow this zone.
    pub tz: chrono_tz::Tz,
    /// Anyone may create an account. Turn it off once the people who should be in are in; the CLI can still
    /// add people.
    pub signups_open: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { tz: chrono_tz::UTC, signups_open: true }
    }
}

/// The database and every operation on it.
#[derive(Clone)]
pub struct Store {
    pub(crate) pool: PgPool,
    pub(crate) cfg: Arc<Config>,
}

/// Who is acting and what they may do. A web session may do anything; a connector token only its scopes.
#[derive(Debug, Clone)]
pub struct Caller {
    pub user_id: i64,
    scopes: Vec<String>,
    interactive: bool,
}

impl Caller {
    /// Everything: the web app, and the CLI acting as a person.
    pub fn full(user_id: i64) -> Self {
        Caller { user_id, scopes: api::SCOPES.iter().map(|s| s.to_string()).collect(), interactive: true }
    }

    /// An api token: only its scopes, and never a person at a keyboard.
    pub fn scoped(user_id: i64, scopes: Vec<String>) -> Self {
        Caller { user_id, scopes, interactive: false }
    }

    /// A person signed in (the web app, or the CLI as them), not a token that was handed to something.
    pub fn is_interactive(&self) -> bool {
        self.interactive
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

/// Where the database is when nothing says otherwise: the one `docker compose up -d` starts.
pub const DEFAULT_DB: &str = "postgres://tracer:tracer@localhost:5432/tracer";

impl Store {
    /// Connect to Postgres (`postgres://user:pass@host:port/db`) and bring the schema up to date.
    pub async fn open(url: &str) -> Result<Store> {
        Store::open_pool(url, 10).await
    }

    /// The same, with room for `max` connections at once.
    pub async fn open_pool(url: &str, max: u32) -> Result<Store> {
        let opts = PgConnectOptions::from_str(url).map_err(|e| Error::bad(format!("bad database url: {e}")))?;
        Store::connect(opts, max.max(2)).await
    }

    async fn connect(opts: PgConnectOptions, max: u32) -> Result<Store> {
        let pool = PgPoolOptions::new()
            .max_connections(max)
            .connect_with(opts)
            .await
            .map_err(|e| Error::Internal(format!("cannot reach the database: {e}. is it running? (docker compose up -d)")))?;
        sqlx::migrate!("./migrations").run(&pool).await.map_err(|e| Error::Internal(format!("migration failed: {e}")))?;
        Ok(Store { pool, cfg: Arc::new(Config::default()) })
    }

    /// The same database with other settings.
    pub fn with_config(mut self, cfg: Config) -> Store {
        self.cfg = Arc::new(cfg);
        self
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Today in the configured zone.
    pub(crate) fn today(&self) -> chrono::NaiveDate {
        chrono::Utc::now().with_timezone(&self.cfg.tz).date_naive()
    }

    /// An explicit date as `YYYY-MM-DD`, or today when there is none.
    pub(crate) fn date_or_today(&self, d: Option<&str>) -> Result<String> {
        match d.map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(self.today().to_string()),
            Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| d.to_string())
                .map_err(|_| Error::bad(format!("invalid date '{s}', expected YYYY-MM-DD"))),
        }
    }

    /// Run one statement as is. For tests that need to age a row; nothing else should call it.
    #[doc(hidden)]
    pub async fn raw(&self, sql: &str) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_string())).execute(&self.pool).await?;
        Ok(())
    }

    /// Is the database answering? For a health check.
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// A store of its own for one test: a fresh schema in the database at `TRACER_TEST_DB` (default: the
    /// compose one). Schemas left by runs more than ten minutes old are dropped on the way.
    pub async fn test() -> Result<Store> {
        use sqlx::Row;
        let url = std::env::var("TRACER_TEST_DB").unwrap_or_else(|_| DEFAULT_DB.into());
        let base = PgConnectOptions::from_str(&url).map_err(|e| Error::bad(format!("bad database url: {e}")))?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(base.clone())
            .await
            .map_err(|e| Error::Internal(format!("tests need postgres: {e}. start it with: docker compose up -d")))?;
        let now = chrono::Utc::now().timestamp();
        let old = sqlx::query("SELECT nspname FROM pg_namespace WHERE nspname LIKE 'test\\_%'").fetch_all(&admin).await?;
        for r in old {
            let name: String = r.get(0);
            let born: i64 = name.split('_').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            if now - born > 600 {
                // the name came from the catalog and matches test_<digits>_<hex>
                let _ = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA IF EXISTS \"{name}\" CASCADE"))).execute(&admin).await;
            }
        }
        let mut raw = [0u8; 6];
        rand::fill(&mut raw);
        let schema = format!("test_{now}_{}", hex::encode(raw));
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA \"{schema}\""))).execute(&admin).await?;
        admin.close().await;
        Store::connect(base.options([("search_path", schema.as_str())]), 3).await
    }
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

