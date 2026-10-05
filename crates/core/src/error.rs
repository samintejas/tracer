use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// No valid sign-in.
    Unauthorized,
    /// Signed in, but this token or person may not do that.
    Forbidden(String),
    NotFound(&'static str),
    BadRequest(String),
    Conflict(String),
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn bad(msg: impl Into<String>) -> Self {
        Error::BadRequest(msg.into())
    }

    pub fn message(&self) -> String {
        match self {
            Error::Unauthorized => "not signed in".into(),
            Error::NotFound(what) => format!("{what} not found"),
            Error::Forbidden(m) | Error::BadRequest(m) | Error::Conflict(m) | Error::Internal(m) => m.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            if db.is_unique_violation() {
                return Error::Conflict("already exists".into());
            }
        }
        tracing::error!("database error: {e}");
        Error::Internal("database error".into())
    }
}
