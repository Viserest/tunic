use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use utils::{Value, lock};

use crate::{Backend, DbError, LocalBackend};

const MAX_SEGMENTS: usize = 8;
const MAX_SEGMENT_LEN: usize = 64;

/// A `/`-separated namespace: letters, digits, `_` and `-` in each segment.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Scope(String);

pub(crate) fn valid_segment(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_SEGMENT_LEN && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Scope {
    pub fn new(name: &str) -> Result<Scope, DbError> {
        let segs: Vec<&str> = name.split('/').collect();
        if segs.len() > MAX_SEGMENTS || !segs.iter().all(|s| valid_segment(s)) {
            return Err(DbError::InvalidScope(format!("`{name}` (use up to {MAX_SEGMENTS} segments of letters, digits, `_`, `-`)")));
        }
        Ok(Scope(name.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn child(&self, name: &str) -> Result<Scope, DbError> {
        Scope::new(&format!("{}/{}", self.0, name))
    }
    /// `true` if `ns` is this scope or one of its children.
    pub fn contains(&self, ns: &str) -> bool {
        ns == self.0 || (ns.starts_with(&self.0) && ns.as_bytes().get(self.0.len()) == Some(&b'/'))
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

type Locks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

/// A handle to a [`Backend`], optionally narrowed to a [`Scope`].
///
/// The root handle (no scope) can only create scoped handles. All reads and writes need a scope,
/// so code given `db.scope("myapp")` cannot touch anybody else's data.
#[derive(Clone)]
pub struct Database {
    backend: Arc<dyn Backend>,
    scope: Option<Scope>,
    locks: Locks,
}

impl Database {
    pub fn new(backend: Arc<dyn Backend>) -> Database {
        Database { backend, scope: None, locks: Default::default() }
    }

    /// Volatile database in memory.
    pub fn memory() -> Database {
        Database::new(Arc::new(LocalBackend::memory()))
    }

    /// Database stored in a directory on the filesystem.
    pub fn open(dir: impl AsRef<Path>) -> Result<Database, DbError> {
        Ok(Database::new(Arc::new(LocalBackend::open(dir)?)))
    }

    pub fn backend(&self) -> Arc<dyn Backend> {
        self.backend.clone()
    }

    pub fn scope_name(&self) -> Option<&str> {
        self.scope.as_ref().map(|s| s.as_str())
    }

    /// Narrow to a (child) scope. On a root handle this creates the top-level scope.
    pub fn scope(&self, name: &str) -> Result<Database, DbError> {
        let scope = match &self.scope {
            None => Scope::new(name)?,
            Some(s) => s.child(name)?,
        };
        Ok(Database { backend: self.backend.clone(), scope: Some(scope), locks: self.locks.clone() })
    }

    fn ns(&self) -> Result<&str, DbError> {
        self.scope.as_ref().map(|s| s.as_str()).ok_or(DbError::ScopeRequired)
    }

    fn check_col(col: &str) -> Result<(), DbError> {
        if valid_segment(col) { Ok(()) } else { Err(DbError::Invalid(format!("bad collection name `{col}`"))) }
    }

    /// Lock shared by every handle on this database for one collection (used for read-modify-write).
    pub(crate) fn collection_lock(&self, col: &str) -> Result<Arc<Mutex<()>>, DbError> {
        let key = format!("{}/{}", self.ns()?, col);
        Ok(lock(&self.locks).entry(key).or_default().clone())
    }

    // ---- raw document access (used by Table, features that need untyped data, and the server)

    pub fn get(&self, col: &str, id: &str) -> Result<Option<Value>, DbError> {
        Self::check_col(col)?;
        self.backend.get(self.ns()?, col, id)
    }
    pub fn put(&self, col: &str, id: &str, value: &Value) -> Result<(), DbError> {
        Self::check_col(col)?;
        self.backend.put(self.ns()?, col, id, value)
    }
    pub fn delete(&self, col: &str, id: &str) -> Result<bool, DbError> {
        Self::check_col(col)?;
        self.backend.delete(self.ns()?, col, id)
    }
    pub fn list(&self, col: &str) -> Result<Vec<(String, Value)>, DbError> {
        Self::check_col(col)?;
        self.backend.list(self.ns()?, col)
    }
    pub fn collections(&self) -> Result<Vec<String>, DbError> {
        self.backend.collections(self.ns()?)
    }
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Database(scope={:?})", self.scope_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_validation() {
        assert!(Scope::new("a/b-c_d").is_ok());
        for bad in ["", "a//b", "../x", "a/../b", "a b", "a/", "/a", "é", &"x".repeat(65), "a/b/c/d/e/f/g/h/i"] {
            assert!(Scope::new(bad).is_err(), "{bad}");
        }
        let s = Scope::new("app").unwrap();
        assert!(s.contains("app") && s.contains("app/x") && !s.contains("apple") && !s.contains("ap"));
    }

    #[test]
    fn root_needs_scope() {
        let db = Database::memory();
        assert_eq!(db.get("c", "1").unwrap_err(), DbError::ScopeRequired);
        let s = db.scope("a").unwrap();
        s.put("c", "1", &Value::Int(1)).unwrap();
        assert_eq!(s.get("c", "1").unwrap(), Some(Value::Int(1)));
        // siblings and children are separate namespaces
        assert_eq!(db.scope("b").unwrap().get("c", "1").unwrap(), None);
        assert_eq!(s.scope("child").unwrap().get("c", "1").unwrap(), None);
        assert!(s.put("../c", "1", &Value::Null).is_err());
    }
}
