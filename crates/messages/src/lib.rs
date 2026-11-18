//! # Messages
//!
//! Group chat with members, stored in a database scope.
//!
//! * [`Messages`] is the service: groups, members, posting, history.
//! * [`router`] exposes it over HTTP (`/message/...`) plus an **ingest** endpoint
//!   (`POST /message/ingest/:source`, bearer token) that external bridges use to push messages in.
//! * [`relay::RelayServer`] is a tiny line-based TCP relay (`AUTH <token>`, `SEND <group> <text>`)
//!   for simple bots and scripts.
//!
//! Honest scope: Tunic does **not** speak XMPP, Signal or SimpleX natively. Implement the
//! [`Bridge`] trait (or run a bridge process that calls the ingest endpoint) to connect them.

pub mod relay;

use database::{Database, DbError, Record, Table, Uuid};
use element::{Page, heading, table, text};
use router::{Request, Response, Router};
use std::fmt;
use user::Users;
use utils::{Value, unix_now};

#[derive(Clone, Debug, PartialEq, Record)]
pub struct ChatGroup {
    pub id: Uuid,
    pub name: String,
    pub owner: Uuid,
    pub members: Vec<Uuid>,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Message {
    pub id: Uuid,
    pub group_id: Uuid,
    /// `None` for messages that came in through a bridge.
    pub sender: Option<Uuid>,
    /// Display name: username, or `source:name` for ingested messages.
    pub author: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Debug)]
pub enum MsgError {
    NotFound,
    Forbidden,
    Invalid(String),
    Storage(String),
}
impl fmt::Display for MsgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MsgError::NotFound => write!(f, "Nothing found"),
            MsgError::Forbidden => write!(f, "Forbidden"),
            MsgError::Invalid(s) => write!(f, "{s}"),
            MsgError::Storage(s) => write!(f, "storage error: {s}"),
        }
    }
}
impl std::error::Error for MsgError {}
impl From<DbError> for MsgError {
    fn from(e: DbError) -> Self {
        MsgError::Storage(e.to_string())
    }
}

/// Connect an outside chat network. `deliver` is called for every message posted in a group.
pub trait Bridge: Send + Sync {
    fn name(&self) -> &str;
    fn deliver(&self, group: &ChatGroup, message: &Message);
}

pub const MAX_BODY: usize = 8000;

#[derive(Clone)]
pub struct Messages {
    pub groups: Table<ChatGroup>,
    pub messages: Table<Message>,
    bridges: std::sync::Arc<std::sync::Mutex<Vec<std::sync::Arc<dyn Bridge>>>>,
}

impl Messages {
    pub fn new(db: Database) -> Result<Messages, MsgError> {
        Ok(Messages { groups: Table::new(db.clone())?, messages: Table::new(db)?, bridges: Default::default() })
    }

    pub fn add_bridge(&self, b: std::sync::Arc<dyn Bridge>) {
        utils::lock(&self.bridges).push(b);
    }

    pub fn create_group(&self, owner: Uuid, name: &str) -> Result<ChatGroup, MsgError> {
        let name = name.trim();
        if name.is_empty() || name.len() > 100 {
            return Err(MsgError::Invalid("group name must be 1-100 characters".into()));
        }
        let g = ChatGroup { id: Uuid::new_v4(), name: name.into(), owner, members: vec![owner], created_at: unix_now() };
        self.groups.insert(&g)?;
        Ok(g)
    }

    fn member_group(&self, user: &Uuid, group: &Uuid) -> Result<ChatGroup, MsgError> {
        let g = self.groups.get(group)?.ok_or(MsgError::NotFound)?;
        if g.members.contains(user) { Ok(g) } else { Err(MsgError::NotFound) }
    }

    pub fn groups_of(&self, user: &Uuid) -> Result<Vec<ChatGroup>, MsgError> {
        Ok(self.groups.find(|g| g.members.contains(user))?)
    }

    pub fn add_member(&self, by: &Uuid, group: &Uuid, user: Uuid) -> Result<ChatGroup, MsgError> {
        let g = self.member_group(by, group)?;
        if g.owner != *by {
            return Err(MsgError::Forbidden);
        }
        if g.members.contains(&user) {
            return Ok(g);
        }
        self.groups.update(group, |g| g.members.push(user))?.ok_or(MsgError::NotFound)
    }

    pub fn remove_member(&self, by: &Uuid, group: &Uuid, user: &Uuid) -> Result<ChatGroup, MsgError> {
        let g = self.member_group(by, group)?;
        if g.owner != *by && by != user {
            return Err(MsgError::Forbidden);
        }
        if *user == g.owner {
            return Err(MsgError::Invalid("the owner cannot leave; delete the group instead".into()));
        }
        self.groups.update(group, |g| g.members.retain(|m| m != user))?.ok_or(MsgError::NotFound)
    }

    pub fn delete_group(&self, by: &Uuid, group: &Uuid) -> Result<(), MsgError> {
        let g = self.member_group(by, group)?;
        if g.owner != *by {
            return Err(MsgError::Forbidden);
        }
        self.groups.remove(group)?;
        for m in self.messages.find(|m| m.group_id == *group)? {
            self.messages.remove(&m.id)?;
        }
        Ok(())
    }

    fn store(&self, g: &ChatGroup, sender: Option<Uuid>, author: &str, body: &str) -> Result<Message, MsgError> {
        let body = body.trim();
        if body.is_empty() || body.len() > MAX_BODY {
            return Err(MsgError::Invalid(format!("message must be 1-{MAX_BODY} bytes")));
        }
        let m = Message { id: Uuid::new_v4(), group_id: g.id, sender, author: author.into(), body: body.into(), created_at: unix_now() };
        self.messages.insert(&m)?;
        let bridges: Vec<_> = utils::lock(&self.bridges).clone();
        for b in bridges {
            b.deliver(g, &m);
        }
        Ok(m)
    }

    pub fn post(&self, user: &Uuid, author: &str, group: &Uuid, body: &str) -> Result<Message, MsgError> {
        let g = self.member_group(user, group)?;
        self.store(&g, Some(*user), author, body)
    }

    /// Post on behalf of an external network (bypasses membership; callers must be trusted).
    pub fn ingest(&self, group: &Uuid, source: &str, author: &str, body: &str) -> Result<Message, MsgError> {
        let g = self.groups.get(group)?.ok_or(MsgError::NotFound)?;
        let clean = |s: &str| s.chars().filter(|c| !c.is_control()).take(60).collect::<String>();
        self.store(&g, None, &format!("{}:{}", clean(source), clean(author)), body)
    }

    /// Newest-last history; `since` is exclusive (unix seconds).
    pub fn history(&self, user: &Uuid, group: &Uuid, since: u64, limit: usize) -> Result<Vec<Message>, MsgError> {
        self.member_group(user, group)?;
        let mut v = self.messages.find(|m| m.group_id == *group && m.created_at > since)?;
        v.sort_by_key(|m| m.created_at);
        let n = v.len();
        Ok(v.split_off(n.saturating_sub(limit)))
    }
}

fn err(e: MsgError) -> Response {
    match e {
        MsgError::NotFound => Response::not_found(),
        MsgError::Forbidden => Response::forbidden(),
        MsgError::Invalid(m) => Response::bad_request(m),
        MsgError::Storage(m) => Response::internal(m),
    }
}

/// `ingest_token`: bearer token accepted by `/message/ingest/:source`; `None` disables the endpoint.
pub fn router(msgs: Messages, users: Users, ingest_token: Option<String>) -> Router {
    macro_rules! authed {
        (|$m:ident, $u:ident, $me:ident, $req:ident| $body:expr) => {{
            let ($m, $u) = (msgs.clone(), users.clone());
            move |$req: &Request| {
                let Some($me) = $u.from_request($req) else { return Response::unauthorized() };
                let _ = &$u;
                $body
            }
        }};
    }
    let gid = |req: &Request| req.param("id").and_then(Uuid::parse);

    let list = authed!(|m, u, me, req| match m.groups_of(&me.id) {
        Ok(g) => Response::json(&utils::json_obj! {"groups" => g.iter().map(|x| x.public_value()).collect::<Vec<_>>()}),
        Err(e) => err(e),
    });
    let page = authed!(|m, u, me, req| {
        let g = m.groups_of(&me.id).unwrap_or_default();
        let rows = g.iter().map(|x| vec![x.name.clone(), x.members.len().to_string(), x.id.to_string()]).collect::<Vec<_>>();
        Response::page(req, &Page::new("Messages").add(heading(1, "Messages")).add(text(format!("{} group(s)", rows.len()))).add(table(["Group", "Members", "Id"], rows)))
    });
    let create = authed!(|m, u, me, req| {
        let v = match req.json() {
            Ok(v) => v,
            Err(e) => return Response::bad_request(e),
        };
        match m.create_group(me.id, v.get("name").and_then(|n| n.as_str()).unwrap_or("")) {
            Ok(g) => Response::json(&g.public_value()).status(201),
            Err(e) => err(e),
        }
    });
    let delete = authed!(|m, u, me, req| match req.param("id").and_then(Uuid::parse) {
        Some(id) => match m.delete_group(&me.id, &id) {
            Ok(()) => Response::ok_msg("deleted"),
            Err(e) => err(e),
        },
        None => Response::not_found(),
    });
    let members = authed!(|m, u, me, req| {
        let Some(id) = req.param("id").and_then(Uuid::parse) else { return Response::not_found() };
        let v = match req.json() {
            Ok(v) => v,
            Err(e) => return Response::bad_request(e),
        };
        let Some(target) = v.get("username").and_then(|n| n.as_str()).and_then(|n| u.find_by_name(n)) else { return Response::bad_request("unknown user") };
        let r = if v.get("remove").and_then(|b| b.as_bool()) == Some(true) { m.remove_member(&me.id, &id, &target.id) } else { m.add_member(&me.id, &id, target.id) };
        match r {
            Ok(g) => Response::json(&g.public_value()),
            Err(e) => err(e),
        }
    });
    let post = authed!(|m, u, me, req| {
        let Some(id) = req.param("id").and_then(Uuid::parse) else { return Response::not_found() };
        let v = match req.json() {
            Ok(v) => v,
            Err(e) => return Response::bad_request(e),
        };
        match m.post(&me.id, &me.username, &id, v.get("body").and_then(|b| b.as_str()).unwrap_or("")) {
            Ok(x) => Response::json(&x.public_value()).status(201),
            Err(e) => err(e),
        }
    });
    let history = authed!(|m, u, me, req| {
        let Some(id) = req.param("id").and_then(Uuid::parse) else { return Response::not_found() };
        let since = req.query_param("since").and_then(|s| s.parse().ok()).unwrap_or(0);
        let limit = req.query_param("limit").and_then(|s| s.parse().ok()).unwrap_or(100usize).min(1000);
        match m.history(&me.id, &id, since, limit) {
            Ok(h) => Response::json(&utils::json_obj! {"messages" => h.iter().map(|x| x.public_value()).collect::<Vec<_>>()}),
            Err(e) => err(e),
        }
    });

    let m2 = msgs.clone();
    let ingest = move |req: &Request| {
        let Some(expected) = ingest_token.as_deref() else { return Response::not_found() };
        match req.bearer_token() {
            Some(t) if utils::password::ct_eq(t.as_bytes(), expected.as_bytes()) => {}
            _ => return Response::unauthorized(),
        }
        let v: Value = match req.json() {
            Ok(v) => v,
            Err(e) => return Response::bad_request(e),
        };
        let Some(group) = v.get("group_id").and_then(|g| g.as_str()).and_then(Uuid::parse) else { return Response::bad_request("group_id is required") };
        let source = req.param("source").unwrap_or("bridge");
        let author = v.get("author").and_then(|a| a.as_str()).unwrap_or("unknown");
        match m2.ingest(&group, source, author, v.get("body").and_then(|b| b.as_str()).unwrap_or("")) {
            Ok(x) => Response::json(&x.public_value()).status(201),
            Err(e) => err(e),
        }
    };
    let _ = gid;

    Router::new()
        .get("/message", page)
        .get("/message/groups", list)
        .post("/message/groups", create)
        .delete("/message/groups/:id", delete)
        .post("/message/groups/:id/members", members)
        .post("/message/groups/:id/messages", post)
        .get("/message/groups/:id/messages", history)
        .post("/message/ingest/:source", ingest)
        .tagged("public")
}

#[cfg(test)]
mod tests {
    use super::*;
    use router::Method;
    use std::sync::{Arc, Mutex};

    struct Capture(Mutex<Vec<String>>);
    impl Bridge for Capture {
        fn name(&self) -> &str {
            "capture"
        }
        fn deliver(&self, g: &ChatGroup, m: &Message) {
            self.0.lock().unwrap().push(format!("{}:{}", g.name, m.body));
        }
    }

    fn setup() -> (Messages, Users, Router) {
        let db = Database::memory();
        let users = Users::with_config(db.scope("user").unwrap(), user::UsersConfig { password_iterations: 50, ..Default::default() }).unwrap();
        for n in ["alice", "bobby", "carol"] {
            users.create_user(n, "long enough password", false).unwrap();
        }
        let m = Messages::new(db.scope("messages").unwrap()).unwrap();
        let r = router(m.clone(), users.clone(), Some("secret-token".into()));
        (m, users, r)
    }

    #[test]
    fn membership_rules() {
        let (m, u, _) = setup();
        let id = |n: &str| u.find_by_name(n).unwrap().id;
        let g = m.create_group(id("alice"), "team").unwrap();
        assert!(m.create_group(id("alice"), "  ").is_err());
        assert!(matches!(m.post(&id("bobby"), "bobby", &g.id, "hi"), Err(MsgError::NotFound)), "non-members can't tell the group exists");
        assert!(matches!(m.add_member(&id("bobby"), &g.id, id("bobby")), Err(MsgError::NotFound)));
        m.add_member(&id("alice"), &g.id, id("bobby")).unwrap();
        assert!(matches!(m.add_member(&id("bobby"), &g.id, id("carol")), Err(MsgError::Forbidden)));
        m.post(&id("bobby"), "bobby", &g.id, " hello ").unwrap();
        assert!(m.post(&id("bobby"), "bobby", &g.id, "   ").is_err());
        assert!(m.post(&id("bobby"), "bobby", &g.id, &"x".repeat(MAX_BODY + 1)).is_err());
        assert_eq!(m.history(&id("alice"), &g.id, 0, 10).unwrap()[0].body, "hello");
        m.remove_member(&id("bobby"), &g.id, &id("bobby")).unwrap();
        assert!(m.history(&id("bobby"), &g.id, 0, 10).is_err());
        assert!(m.remove_member(&id("alice"), &g.id, &id("alice")).is_err());
        assert!(matches!(m.delete_group(&id("bobby"), &g.id), Err(MsgError::NotFound)));
        m.delete_group(&id("alice"), &g.id).unwrap();
        assert!(m.messages.all().unwrap().is_empty());
    }

    #[test]
    fn bridges_and_ingest_over_http() {
        let (m, u, r) = setup();
        let cap = Arc::new(Capture(Mutex::new(vec![])));
        m.add_bridge(cap.clone());
        let cookie = format!("session_id={}", u.login("alice", "long enough password").unwrap().1.id);
        let call = |m: Method, t: &str, body: &str, h: &[(&str, &str)]| {
            let mut req = Request::new(m, t);
            req.body = body.as_bytes().to_vec();
            for (k, v) in h {
                req = req.with_header(k, v);
            }
            r.handle(req)
        };
        let g = call(Method::Post, "/message/groups", r#"{"name":"ops"}"#, &[("Cookie", &cookie)]);
        assert_eq!(g.status, 201);
        let gid = g.json_body().unwrap().get("id").unwrap().as_str().unwrap().to_string();
        let body = format!(r#"{{"group_id":"{gid}","author":"zed","body":"from xmpp"}}"#);
        assert_eq!(call(Method::Post, "/message/ingest/xmpp", &body, &[]).status, 401);
        assert_eq!(call(Method::Post, "/message/ingest/xmpp", &body, &[("Authorization", "Bearer wrong")]).status, 401);
        assert_eq!(call(Method::Post, "/message/ingest/xmpp", &body, &[("Authorization", "Bearer secret-token")]).status, 201);
        let hist = call(Method::Get, &format!("/message/groups/{gid}/messages"), "", &[("Cookie", &cookie)]).json_body().unwrap();
        let first = &hist.get("messages").unwrap().as_array().unwrap()[0];
        assert_eq!(first.get("author").unwrap().as_str(), Some("xmpp:zed"));
        assert_eq!(call(Method::Post, &format!("/message/groups/{gid}/messages"), r#"{"body":"native"}"#, &[("Cookie", &cookie)]).status, 201);
        assert_eq!(cap.0.lock().unwrap().as_slice(), ["ops:from xmpp", "ops:native"]);
        assert_eq!(call(Method::Get, "/message/groups", "", &[]).status, 401);
        // endpoint is off without a token
        let off = router(m, u, None);
        let mut req = Request::new(Method::Post, "/message/ingest/x");
        req.body = body.into_bytes();
        assert_eq!(off.handle(req.with_header("Authorization", "Bearer secret-token")).status, 404);
    }
}
