use std::sync::Arc;

use router::{Request, Response, Router};
use utils::{Uuid, Value, json_obj, unix_now};

use crate::{Record, Table};

type Guard = Arc<dyn Fn(&Request) -> Option<Response> + Send + Sync>;

/// Which generic routes [`crud`] creates, and an optional guard (e.g. "must be logged in").
#[derive(Clone)]
pub struct CrudOptions {
    pub list: bool,
    pub create: bool,
    pub read: bool,
    pub update: bool,
    pub delete: bool,
    /// Runs before every route; return `Some(response)` to refuse the request.
    pub guard: Option<Guard>,
}

impl Default for CrudOptions {
    fn default() -> Self {
        CrudOptions { list: true, create: true, read: true, update: true, delete: true, guard: None }
    }
}

impl CrudOptions {
    pub fn read_only() -> Self {
        CrudOptions { create: false, update: false, delete: false, ..Default::default() }
    }
    pub fn guard(mut self, g: impl Fn(&Request) -> Option<Response> + Send + Sync + 'static) -> Self {
        self.guard = Some(Arc::new(g));
        self
    }
}

fn parse_id(body: &Value) -> Result<Uuid, Response> {
    body.get("id")
        .and_then(|i| i.as_str())
        .and_then(Uuid::parse)
        .ok_or_else(|| Response::bad_request("`id` (uuid) is required"))
}

fn db_err(e: crate::DbError) -> Response {
    Response::internal(e.to_string())
}

/// JSON crud routes for a table, all `POST` with JSON bodies (`list` also answers `GET`):
///
/// | route     | body                              | answers                                    |
/// |-----------|-----------------------------------|--------------------------------------------|
/// | `/list`   | `{"limit":20,"offset":0}`         | `{"msg","content":{length,limit,offset,data}}` |
/// | `/create` | the record's fields               | `201 {"msg","content":<id>}`               |
/// | `/read`   | `{"id"}`                          | `{"msg","content":<record>}`               |
/// | `/update` | `{"id", ...changed fields}`       | `{"msg","content":<id>}`                   |
/// | `/delete` | `{"id"}`                          | `{"msg"}`                                  |
///
/// `#[record(hidden)]` fields are never returned and cannot be set. `id` and `created_at` are
/// assigned on create and immutable afterwards.
pub fn crud<V: Record>(table: Table<V>, opts: CrudOptions) -> Router {
    let mut r = Router::new();
    let guarded = |opts: &CrudOptions, f: Arc<dyn Fn(&Request) -> Response + Send + Sync>| {
        let g = opts.guard.clone();
        move |req: &Request| {
            if let Some(g) = &g {
                if let Some(deny) = g(req) {
                    return deny;
                }
            }
            f(req)
        }
    };

    if opts.list {
        let t = table.clone();
        let list: Arc<dyn Fn(&Request) -> Response + Send + Sync> = Arc::new(move |req| {
            let body = if req.body.is_empty() { Value::object() } else { req.json().unwrap_or_else(|_| Value::object()) };
            let pick = |name: &str| body.get(name).and_then(|v| v.as_u64()).or_else(|| req.query_param(name).and_then(|s| s.parse().ok()));
            let limit = pick("limit").unwrap_or(20).clamp(1, 500) as usize;
            let offset = pick("offset").unwrap_or(0) as usize;
            match t.list(offset, limit) {
                Ok(items) => {
                    let data: Vec<Value> = items.iter().map(|i| i.public_value()).collect();
                    Response::json(&json_obj! {
                        "msg" => "Successfully found list",
                        "content" => json_obj! {"length" => data.len(), "limit" => limit, "offset" => offset, "data" => data},
                    })
                }
                Err(e) => db_err(e),
            }
        });
        r = r.post("/list", guarded(&opts, list.clone())).get("/list", guarded(&opts, list));
    }
    if opts.create {
        let t = table.clone();
        let f: Arc<dyn Fn(&Request) -> Response + Send + Sync> = Arc::new(move |req| {
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
            if body.get("created_at").is_none() {
                body.set("created_at", unix_now());
            }
            let record = match V::from_value(&body) {
                Ok(r) => r,
                Err(e) => return Response::bad_request(e),
            };
            match t.create(&record) {
                Ok(true) => Response::json(&json_obj! {"msg" => "Successfully created", "content" => id}).status(201),
                Ok(false) => Response::error(409, "Already exists"),
                Err(e) => db_err(e),
            }
        });
        r = r.post("/create", guarded(&opts, f));
    }
    if opts.read {
        let t = table.clone();
        let f: Arc<dyn Fn(&Request) -> Response + Send + Sync> = Arc::new(move |req| {
            let body = req.json().unwrap_or_else(|_| Value::object());
            let id = match parse_id(&body).or_else(|e| req.query_param("id").and_then(Uuid::parse).ok_or(e)) {
                Ok(i) => i,
                Err(e) => return e,
            };
            match t.get(&id) {
                Ok(Some(v)) => Response::json(&json_obj! {"msg" => "Successfully found", "content" => v.public_value()}),
                Ok(None) => Response::error(404, "Nothing found"),
                Err(e) => db_err(e),
            }
        });
        r = r.post("/read", guarded(&opts, f.clone())).get("/read", guarded(&opts, f));
    }
    if opts.update {
        let t = table.clone();
        let f: Arc<dyn Fn(&Request) -> Response + Send + Sync> = Arc::new(move |req| {
            let body = match req.json() {
                Ok(b) => b,
                Err(e) => return Response::bad_request(e),
            };
            let id = match parse_id(&body) {
                Ok(i) => i,
                Err(e) => return e,
            };
            let Some(patch) = body.as_object() else { return Response::bad_request("expected a json object") };
            let mut failure: Option<String> = None;
            let result = t.update(&id, |rec| {
                let mut merged = rec.to_value();
                if let Some(o) = merged.as_object_mut() {
                    for (k, v) in patch {
                        if k == "id" || k == "created_at" || V::hidden_fields().contains(&k.as_str()) {
                            continue;
                        }
                        o.insert(k.clone(), v.clone());
                    }
                }
                match V::from_value(&merged) {
                    Ok(new) => *rec = new,
                    Err(e) => failure = Some(e),
                }
            });
            if let Some(e) = failure {
                return Response::bad_request(e);
            }
            match result {
                Ok(Some(_)) => Response::json(&json_obj! {"msg" => "Successfully updated", "content" => id}),
                Ok(None) => Response::error(404, "Nothing found"),
                Err(e) => db_err(e),
            }
        });
        r = r.post("/update", guarded(&opts, f));
    }
    if opts.delete {
        let t = table;
        let f: Arc<dyn Fn(&Request) -> Response + Send + Sync> = Arc::new(move |req| {
            let body = req.json().unwrap_or_else(|_| Value::object());
            let id = match parse_id(&body) {
                Ok(i) => i,
                Err(e) => return e,
            };
            match t.remove(&id) {
                Ok(true) => Response::ok_msg("Successfully deleted"),
                Ok(false) => Response::error(404, "Nothing found"),
                Err(e) => db_err(e),
            }
        });
        r = r.post("/delete", guarded(&opts, f));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[derive(Clone, Debug, PartialEq, Record)]
    struct Note {
        id: Uuid,
        title: String,
        #[record(default)]
        created_at: u64,
        #[record(hidden, default)]
        secret: String,
    }

    fn call(r: &Router, path: &str, body: &str) -> Response {
        r.handle(Request::post(path, body))
    }

    #[test]
    fn full_cycle() {
        let table: Table<Note> = Table::new(Database::memory().scope("t").unwrap()).unwrap();
        let r = crud(table.clone(), CrudOptions::default());

        let c = call(&r, "/create", r#"{"title":"hello","secret":"nope","id":"ignored"}"#);
        assert_eq!(c.status, 201, "{}", c.body_str());
        let id = c.json_body().unwrap().get("content").unwrap().as_str().unwrap().to_string();
        let stored = table.get(&Uuid::parse(&id).unwrap()).unwrap().unwrap();
        assert_eq!(stored.secret, "", "hidden fields cannot be set through crud");
        assert!(stored.created_at > 0);

        let read = call(&r, "/read", &format!(r#"{{"id":"{id}"}}"#)).json_body().unwrap();
        assert_eq!(read.path("content.title").unwrap().as_str(), Some("hello"));
        assert!(read.path("content.secret").is_none());

        let up = call(&r, "/update", &format!(r#"{{"id":"{id}","title":"changed","created_at":5,"secret":"x"}}"#));
        assert_eq!(up.status, 200);
        let n = table.get(&Uuid::parse(&id).unwrap()).unwrap().unwrap();
        assert_eq!(n.title, "changed");
        assert_eq!(n.created_at, stored.created_at, "created_at is immutable");
        assert_eq!(n.secret, "");

        let list = call(&r, "/list", r#"{"limit":10}"#).json_body().unwrap();
        assert_eq!(list.path("content.length").unwrap().as_i64(), Some(1));
        assert_eq!(r.handle(Request::get("/list?limit=1")).status, 200);

        assert_eq!(call(&r, "/delete", &format!(r#"{{"id":"{id}"}}"#)).status, 200);
        assert_eq!(call(&r, "/delete", &format!(r#"{{"id":"{id}"}}"#)).status, 404);
        assert_eq!(call(&r, "/read", &format!(r#"{{"id":"{id}"}}"#)).status, 404);
    }

    #[test]
    fn validation_and_guard() {
        let table: Table<Note> = Table::new(Database::memory().scope("t").unwrap()).unwrap();
        let r = crud(table, CrudOptions::default().guard(|req| (req.header("x-key") != Some("k")).then(Response::unauthorized)));
        let denied = r.handle(Request::post("/create", r#"{"title":"x"}"#));
        assert_eq!(denied.status, 401);
        let post = |p: &str, b: &str| r.handle(Request::post(p, b).with_header("x-key", "k"));
        assert_eq!(post("/create", "{}").status, 400); // title required
        assert_eq!(post("/create", "[1]").status, 400);
        assert_eq!(post("/create", "garbage").status, 400);
        assert_eq!(post("/read", r#"{"id":"nope"}"#).status, 400);
    }

    #[test]
    fn read_only() {
        let table: Table<Note> = Table::new(Database::memory().scope("t").unwrap()).unwrap();
        let r = crud(table, CrudOptions::read_only());
        assert_eq!(call(&r, "/create", "{}").status, 404);
        assert_eq!(call(&r, "/list", "").status, 200);
    }
}
