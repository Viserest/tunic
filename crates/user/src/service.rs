use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use database::{Database, DbError, Record, Table};
use router::Request;
use utils::password::{DEFAULT_ITERATIONS, hash_with, verify_password};
use utils::{Uuid, lock, unix_now};

#[derive(Clone, Debug, PartialEq, Record)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    #[record(hidden)]
    pub password_hash: String,
    #[record(default)]
    pub is_admin: bool,
    #[record(default)]
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug)]
pub struct UsersConfig {
    /// Session lifetime in seconds.
    pub session_ttl: u64,
    pub min_password_len: usize,
    /// Failed logins per username before it is locked for `lockout_secs`.
    pub max_failures: u32,
    pub lockout_secs: u64,
    /// PBKDF2 iterations for new password hashes (existing hashes keep their own).
    pub password_iterations: u32,
}

impl Default for UsersConfig {
    fn default() -> Self {
        UsersConfig { session_ttl: 6 * 3600, min_password_len: 8, max_failures: 5, lockout_secs: 60, password_iterations: DEFAULT_ITERATIONS }
    }
}

const NAMESPACE: Uuid = Uuid([0x74, 0x75, 0x6e, 0x69, 0x63, 0x2d, 0x75, 0x73, 0x65, 0x72, 0x73, 0, 0, 0, 0, 1]);

/// A directory of users and their sessions.
#[derive(Clone)]
pub struct Users {
    users: Table<User>,
    sessions: Table<Session>,
    pub config: Arc<UsersConfig>,
    failures: Arc<Mutex<HashMap<String, (u32, u64)>>>,
    /// Hash verified when the username does not exist, so timing does not reveal valid names.
    dummy_hash: Arc<String>,
}

fn valid_username(u: &str) -> bool {
    (3..=64).contains(&u.len()) && u.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

impl Users {
    /// `db` must be a scoped database handle.
    pub fn new(db: Database) -> Result<Users, DbError> {
        Users::with_config(db, UsersConfig::default())
    }

    pub fn with_config(db: Database, config: UsersConfig) -> Result<Users, DbError> {
        let dummy = hash_with("dummy password", config.password_iterations);
        Ok(Users {
            users: Table::new(db.clone())?,
            sessions: Table::new(db)?,
            config: Arc::new(config),
            failures: Default::default(),
            dummy_hash: Arc::new(dummy),
        })
    }

    pub fn users_table(&self) -> &Table<User> {
        &self.users
    }

    fn user_id(username: &str) -> Uuid {
        Uuid::from_name(&NAMESPACE, username.to_ascii_lowercase().as_bytes())
    }

    pub fn create_user(&self, username: &str, password: &str, is_admin: bool) -> Result<User, String> {
        if !valid_username(username) {
            return Err("username must be 3-64 characters of letters, digits, `.`, `_`, `-`".into());
        }
        if password.chars().count() < self.config.min_password_len {
            return Err(format!("password must be at least {} characters", self.config.min_password_len));
        }
        let user = User {
            id: Self::user_id(username),
            username: username.to_string(),
            password_hash: hash_with(password, self.config.password_iterations),
            is_admin,
            created_at: unix_now(),
        };
        match self.users.create(&user) {
            Ok(true) => Ok(user),
            Ok(false) => Err("username is taken".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn find_by_name(&self, username: &str) -> Option<User> {
        self.users.get(&Self::user_id(username)).ok().flatten()
    }

    pub fn get(&self, id: &Uuid) -> Option<User> {
        self.users.get(id).ok().flatten()
    }

    pub fn count(&self) -> usize {
        self.users.count().unwrap_or(0)
    }

    pub fn list(&self) -> Vec<User> {
        let mut v = self.users.all().unwrap_or_default();
        v.sort_by(|a, b| a.username.cmp(&b.username));
        v
    }

    /// Check a password. Throttled per username.
    pub fn authenticate(&self, username: &str, password: &str) -> Result<User, String> {
        let key = username.to_ascii_lowercase();
        let now = unix_now();
        {
            let f = lock(&self.failures);
            if let Some((n, since)) = f.get(&key) {
                if *n >= self.config.max_failures && now < since + self.config.lockout_secs {
                    return Err("too many failed attempts, try again later".into());
                }
            }
        }
        let user = self.find_by_name(username);
        let ok = match &user {
            Some(u) => verify_password(password, &u.password_hash),
            None => {
                let _ = verify_password(password, &self.dummy_hash);
                false
            }
        };
        let mut f = lock(&self.failures);
        if ok {
            f.remove(&key);
            Ok(user.unwrap())
        } else {
            let e = f.entry(key).or_insert((0, now));
            if e.0 >= self.config.max_failures && now >= e.1 + self.config.lockout_secs {
                *e = (0, now);
            }
            e.0 += 1;
            e.1 = now;
            Err("invalid username or password".into())
        }
    }

    /// Authenticate and open a session.
    pub fn login(&self, username: &str, password: &str) -> Result<(User, Session), String> {
        let user = self.authenticate(username, password)?;
        let now = unix_now();
        let s = Session { id: Uuid::new_v4(), user_id: user.id, created_at: now, expires_at: now + self.config.session_ttl };
        self.sessions.insert(&s).map_err(|e| e.to_string())?;
        Ok((user, s))
    }

    pub fn logout(&self, session_id: &Uuid) {
        let _ = self.sessions.remove(session_id);
    }

    /// The user owning a valid, unexpired session.
    pub fn session_user(&self, session_id: &Uuid) -> Option<User> {
        let s = self.sessions.get(session_id).ok().flatten()?;
        if s.expires_at < unix_now() {
            let _ = self.sessions.remove(session_id);
            return None;
        }
        self.get(&s.user_id)
    }

    /// The user behind a request: `session_id` cookie or `Authorization: Bearer <session id>`.
    pub fn from_request(&self, req: &Request) -> Option<User> {
        let id = req.cookie("session_id").or_else(|| req.bearer_token().map(String::from))?;
        self.session_user(&Uuid::parse(&id)?)
    }

    pub fn reset_password(&self, user_id: &Uuid, old: &str, new: &str) -> Result<(), String> {
        if new.chars().count() < self.config.min_password_len {
            return Err(format!("password must be at least {} characters", self.config.min_password_len));
        }
        let user = self.get(user_id).ok_or("no such user")?;
        if !verify_password(old, &user.password_hash) {
            return Err("old password is wrong".into());
        }
        self.set_password(user_id, new)
    }

    /// Set a password without knowing the old one (admin use). Ends all the user's sessions.
    pub fn set_password(&self, user_id: &Uuid, new: &str) -> Result<(), String> {
        let hash = hash_with(new, self.config.password_iterations);
        self.users.update(user_id, |u| u.password_hash = hash).map_err(|e| e.to_string())?.ok_or("no such user")?;
        if let Ok(all) = self.sessions.find(|s| s.user_id == *user_id) {
            for s in all {
                let _ = self.sessions.remove(&s.id);
            }
        }
        Ok(())
    }

    pub fn delete_user(&self, user_id: &Uuid) -> bool {
        let existed = self.users.remove(user_id).unwrap_or(false);
        if let Ok(all) = self.sessions.find(|s| s.user_id == *user_id) {
            for s in all {
                let _ = self.sessions.remove(&s.id);
            }
        }
        existed
    }

    /// Remove expired sessions. Returns how many were removed.
    pub fn purge_sessions(&self) -> usize {
        let now = unix_now();
        let old = self.sessions.find(|s| s.expires_at < now).unwrap_or_default();
        old.iter().filter(|s| self.sessions.remove(&s.id).unwrap_or(false)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn users() -> Users {
        Users::with_config(Database::memory().scope("users").unwrap(), UsersConfig { max_failures: 3, password_iterations: 100, ..Default::default() }).unwrap()
    }

    #[test]
    fn create_login_session() {
        let u = users();
        let alice = u.create_user("Alice", "correct horse", true).unwrap();
        assert!(alice.is_admin);
        assert!(u.create_user("alice", "another pass", false).is_err(), "names are case-insensitive");
        assert!(u.create_user("x", "correct horse", false).is_err());
        assert!(u.create_user("bob", "short", false).is_err());
        let (user, s) = u.login("ALICE", "correct horse").unwrap();
        assert_eq!(user.id, alice.id);
        assert_eq!(u.session_user(&s.id).unwrap().username, "Alice");
        u.logout(&s.id);
        assert!(u.session_user(&s.id).is_none());
        assert!(u.login("alice", "wrong").is_err());
        assert!(u.login("nobody", "whatever1").is_err());
    }

    #[test]
    fn lockout_after_failures() {
        let u = users();
        u.create_user("carol", "correct horse", false).unwrap();
        for _ in 0..3 {
            assert!(u.authenticate("carol", "bad").is_err());
        }
        let e = u.authenticate("carol", "correct horse").unwrap_err();
        assert!(e.contains("too many"), "{e}");
    }

    #[test]
    fn passwords_and_expiry() {
        let u = users();
        let d = u.create_user("dave", "old password", false).unwrap();
        let (_, s) = u.login("dave", "old password").unwrap();
        assert!(u.reset_password(&d.id, "nope", "new password").is_err());
        u.reset_password(&d.id, "old password", "new password").unwrap();
        assert!(u.session_user(&s.id).is_none(), "password change ends sessions");
        assert!(u.login("dave", "old password").is_err());
        assert!(u.login("dave", "new password").is_ok());
        // expired sessions
        let id = Uuid::new_v4();
        u.sessions.insert(&Session { id, user_id: d.id, created_at: 0, expires_at: 1 }).unwrap();
        assert!(u.session_user(&id).is_none());
    }

    #[test]
    fn request_lookup_and_stored_hash() {
        let u = users();
        u.create_user("erin", "correct horse", false).unwrap();
        let (_, s) = u.login("erin", "correct horse").unwrap();
        let by_cookie = Request::get("/").with_header("Cookie", &format!("session_id={}", s.id));
        let by_bearer = Request::get("/").with_header("Authorization", &format!("Bearer {}", s.id));
        assert!(u.from_request(&by_cookie).is_some() && u.from_request(&by_bearer).is_some());
        assert!(u.from_request(&Request::get("/")).is_none());
        let stored = u.find_by_name("erin").unwrap();
        assert!(stored.password_hash.starts_with("pbkdf2-sha256$") && !stored.password_hash.contains("correct"));
        assert!(User::hidden_fields().contains(&"password_hash"));
    }
}
