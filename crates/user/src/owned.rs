use database::{Record, Table};
use router::{Request, Response, Router};
use utils::{Uuid, Value, json_obj, unix_now};

use crate::Users;

/// A record that belongs to a user.
pub trait Owned {
    fn owner(&self) -> Uuid;
}

/// Per-user json crud routes (`/list`, `/create`, `/read`, `/update`, `/delete`).
///
/// Same shapes as [`database::crud`], but every route needs a login and only ever touches the
/// caller's own records: `owner` is set from the session on create, cannot be changed, and another
/// user's record looks like "Nothing found".
pub fn owned_crud<V: Record + Owned>(users: Users, table: Table<V>) -> Router {
    fn auth(users: &Users, req: &Request) -> Result<Uuid, Response> {
        users.from_request(req).map(|u| u.id).ok_or_else(Response::unauthorized)
    }
    fn id_of(req: &Request) -> Result<Uuid, Response> {
        let body = req.json().unwrap_or_else(|_| Value::object());
        body.get("id")
            .and_then(|i| i.as_str())
            .map(String::from)
            .or_else(|| req.query_param("id").map(String::from))
            .and_then(|s| Uuid::parse(&s))
            .ok_or_else(|| Response::bad_request("`id` (uuid) is required"))
    }
    fn db_err(e: database::DbError) -> Response {
        Response::internal(e.to_string())
    }
    fn mine<V: Record + Owned>(t: &Table<V>, id: &Uuid, me: &Uuid) -> Result<Option<V>, Response> {
        Ok(t.get(id).map_err(db_err)?.filter(|v| v.owner() == *me))
    }

    let (u, t) = (users.clone(), table.clone());
    let list = move |req: &Request| {
        let me = match auth(&u, req) {
            Ok(m) => m,
            Err(r) => return r,
        };
        let body = if req.body.is_empty() { Value::object() } else { req.json().unwrap_or_else(|_| Value::object()) };
        let pick = |n: &str| body.get(n).and_then(|v| v.as_u64()).or_else(|| req.query_param(n).and_then(|s| s.parse().ok()));
        let limit = pick("limit").unwrap_or(20).clamp(1, 500) as usize;
        let offset = pick("offset").unwrap_or(0) as usize;
        match t.find(|v| v.owner() == me) {
            Ok(all) => {
                let data: Vec<Value> = all.iter().skip(offset).take(limit).map(|v| v.public_value()).collect();
                Response::json(&json_obj! {"msg" => "Successfully found list", "content" => json_obj! {"length" => data.len(), "total" => all.len(), "limit" => limit, "offset" => offset, "data" => data}})
            }
            Err(e) => db_err(e),
        }
    };

    let (u, t) = (users.clone(), table.clone());
    let create = move |req: &Request| {
        let me = match auth(&u, req) {
            Ok(m) => m,
            Err(r) => return r,
        };
        let mut body = match req.json() {
            Ok(b) if b.as_object().is_some() => b,
            Ok(_) => return Response::bad_request("expected a json object"),
            Err(e) => return Response::bad_request(e),
        };
        if let Some(o) = body.as_object_mut() {
            for h in V::hidden_fields() {
                o.remove(*h);
            }
        }
        let id = Uuid::new_v4();
        body.set("id", id);
        body.set("owner", me);
        let now = unix_now();
        body.set("created_at", now);
        body.set("updated_at", now);
        match V::from_value(&body) {
            Ok(rec) => match t.create(&rec) {
                Ok(_) => Response::json(&json_obj! {"msg" => "Successfully created", "content" => id}).status(201),
                Err(e) => db_err(e),
            },
            Err(e) => Response::bad_request(e),
        }
    };

    let (u, t) = (users.clone(), table.clone());
    let read = move |req: &Request| {
        let me = match auth(&u, req) {
            Ok(m) => m,
            Err(r) => return r,
        };
        let id = match id_of(req) {
            Ok(i) => i,
            Err(r) => return r,
        };
        match mine(&t, &id, &me) {
            Ok(Some(v)) => Response::json(&json_obj! {"msg" => "Successfully found", "content" => v.public_value()}),
            Ok(None) => Response::error(404, "Nothing found"),
            Err(r) => r,
        }
    };

    let (u, t) = (users.clone(), table.clone());
    let update = move |req: &Request| {
        let me = match auth(&u, req) {
            Ok(m) => m,
            Err(r) => return r,
        };
        let body = match req.json() {
            Ok(b) => b,
            Err(e) => return Response::bad_request(e),
        };
        let id = match id_of(req) {
            Ok(i) => i,
            Err(r) => return r,
        };
        let Some(patch) = body.as_object() else { return Response::bad_request("expected a json object") };
        match mine(&t, &id, &me) {
            Ok(Some(_)) => {}
            Ok(None) => return Response::error(404, "Nothing found"),
            Err(r) => return r,
        }
        let mut failure = None;
        let now = unix_now();
        let res = t.update(&id, |rec| {
            let mut merged = rec.to_value();
            if let Some(o) = merged.as_object_mut() {
                for (k, v) in patch {
                    if matches!(k.as_str(), "id" | "owner" | "created_at" | "updated_at") || V::hidden_fields().contains(&k.as_str()) {
                        continue;
                    }
                    o.insert(k.clone(), v.clone());
                }
                if o.contains_key("updated_at") {
                    o.insert("updated_at".into(), now.into());
                }
            }
            match V::from_value(&merged) {
                Ok(n) => *rec = n,
                Err(e) => failure = Some(e),
            }
        });
        if let Some(e) = failure {
            return Response::bad_request(e);
        }
        match res {
            Ok(_) => Response::json(&json_obj! {"msg" => "Successfully updated", "content" => id}),
            Err(e) => db_err(e),
        }
    };

    let (u, t) = (users, table);
    let delete = move |req: &Request| {
        let me = match auth(&u, req) {
            Ok(m) => m,
            Err(r) => return r,
        };
        let id = match id_of(req) {
            Ok(i) => i,
            Err(r) => return r,
        };
        match mine(&t, &id, &me) {
            Ok(Some(_)) => match t.remove(&id) {
                Ok(_) => Response::ok_msg("Successfully deleted"),
                Err(e) => db_err(e),
            },
            Ok(None) => Response::error(404, "Nothing found"),
            Err(r) => r,
        }
    };

    Router::new()
        .get("/list", list.clone())
        .post("/list", list)
        .post("/create", create)
        .get("/read", read.clone())
        .post("/read", read)
        .post("/update", update)
        .post("/delete", delete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::Database;

    #[derive(Clone, Debug, PartialEq, Record)]
    struct Note {
        id: Uuid,
        owner: Uuid,
        text: String,
        #[record(default)]
        created_at: u64,
        #[record(default)]
        updated_at: u64,
    }
    impl Owned for Note {
        fn owner(&self) -> Uuid {
            self.owner
        }
    }

    #[test]
    fn users_only_see_their_own_records() {
        let db = Database::memory();
        let users = Users::with_config(db.scope("user").unwrap(), crate::UsersConfig { password_iterations: 50, ..Default::default() }).unwrap();
        users.create_user("alice", "alice password", false).unwrap();
        users.create_user("bob", "bobs password", false).unwrap();
        let cookie = |n: &str, p: &str| format!("session_id={}", users.login(n, p).unwrap().1.id);
        let (a, b) = (cookie("alice", "alice password"), cookie("bob", "bobs password"));
        let r = owned_crud(users.clone(), Table::<Note>::new(db.scope("notes").unwrap()).unwrap());
        let post = |p: &str, body: &str, c: &str| r.handle(Request::post(p, body).with_header("cookie", c));

        assert_eq!(r.handle(Request::get("/list")).status, 401);
        let created = post("/create", r#"{"text":"secret","owner":"00000000-0000-0000-0000-000000000000"}"#, &a);
        assert_eq!(created.status, 201);
        let id = created.json_body().unwrap().get("content").unwrap().as_str().unwrap().to_string();
        let body = format!(r#"{{"id":"{id}"}}"#);

        assert_eq!(post("/read", &body, &a).status, 200);
        assert_eq!(post("/read", &body, &b).status, 404, "other users cannot see it");
        assert_eq!(post("/update", &format!(r#"{{"id":"{id}","text":"hacked"}}"#), &b).status, 404);
        assert_eq!(post("/delete", &body, &b).status, 404);
        assert_eq!(post("/list", "{}", &b).json_body().unwrap().path("content.total").unwrap().as_i64(), Some(0));
        assert_eq!(post("/list", "{}", &a).json_body().unwrap().path("content.total").unwrap().as_i64(), Some(1));

        // owner cannot be reassigned
        let alice = users.find_by_name("alice").unwrap().id;
        let bob = users.find_by_name("bob").unwrap().id;
        assert_eq!(post("/update", &format!(r#"{{"id":"{id}","text":"edited","owner":"{bob}"}}"#), &a).status, 200);
        let read = post("/read", &body, &a).json_body().unwrap();
        assert_eq!(read.path("content.text").unwrap().as_str(), Some("edited"));
        assert_eq!(read.path("content.owner").unwrap().as_str(), Some(alice.to_string().as_str()));
        assert_eq!(post("/delete", &body, &a).status, 200);
    }
}
