use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum DbError {
    /// Operation needs a scope (a root [`crate::Database`] has none).
    ScopeRequired,
    InvalidScope(String),
    Invalid(String),
    Io(String),
    Remote(String),
    Denied(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::ScopeRequired => write!(f, "a database scope is required"),
            DbError::InvalidScope(s) => write!(f, "invalid scope: {s}"),
            DbError::Invalid(s) => write!(f, "invalid data: {s}"),
            DbError::Io(s) => write!(f, "storage error: {s}"),
            DbError::Remote(s) => write!(f, "remote database error: {s}"),
            DbError::Denied(s) => write!(f, "access denied: {s}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<std::io::Error> for DbError {
    fn from(e: std::io::Error) -> Self {
        DbError::Io(e.to_string())
    }
}
