use database::{Record, Uuid};
use router::{Request, Response, Router};
use user::Users;
use utils::Value;

use crate::{Drive, DriveError, Level, safe_to_inline};

fn err(e: DriveError) -> Response {
    match e {
        DriveError::NotFound => Response::not_found(),
        DriveError::Forbidden => Response::forbidden(),
        DriveError::TooLarge(_) => Response::error(413, e.to_string()),
        DriveError::Invalid(m) => Response::bad_request(m),
        DriveError::Storage(m) => Response::internal(m),
    }
}

fn id_param(req: &Request) -> Option<Uuid> {
    req.param("id").and_then(|s| Uuid::parse(s))
}

fn body_json(req: &Request) -> Result<Value, Response> {
    req.json().map_err(Response::bad_request)
}

fn uuid_field(v: &Value, k: &str) -> Option<Uuid> {
    v.get(k).and_then(|x| x.as_str()).and_then(|s| Uuid::parse(s))
}

/// Routes for files, folders, sharing and playlists. Every route needs a logged-in user.
pub fn router(drive: Drive, users: Users) -> Router {
    macro_rules! authed {
        ($drive:expr, $users:expr, |$d:ident, $me:ident, $req:ident| $body:expr) => {{
            let ($d, u) = ($drive.clone(), $users.clone());
            move |$req: &Request| {
                let Some(me) = u.from_request($req) else { return Response::unauthorized() };
                let $me = me.id;
                let _ = &$d;
                $body
            }
        }};
    }

    let upload = authed!(drive, users, |d, me, req| {
        let name = req.header("Og-Filename").or(req.query_param("name")).unwrap_or("file").to_string();
        let dir = req.query_param("dir").and_then(|s| Uuid::parse(s));
        match d.upload(&me, &name, req.header("Content-Type"), &req.body, dir) {
            Ok(m) => Response::json(&m.public_value()).status(201),
            Err(e) => err(e),
        }
    });

    let list = authed!(drive, users, |d, me, req| {
        let dir = req.query_param("dir").and_then(|s| Uuid::parse(s));
        match d.list(&me, dir) {
            Ok((dirs, files)) => Response::json(&utils::json_obj! {
                "directories" => dirs.iter().map(|x| x.public_value()).collect::<Vec<_>>(),
                "files" => files.iter().map(|x| x.public_value()).collect::<Vec<_>>()
            }),
            Err(e) => err(e),
        }
    });

    let download = authed!(drive, users, |d, me, req| {
        let Some(id) = id_param(req) else { return Response::not_found() };
        match d.read(&me, &id) {
            Ok((m, data)) => {
                let inline = safe_to_inline(&m.content_type) && req.query_param("download").is_none();
                let disp = format!("{}; filename=\"{}\"", if inline { "inline" } else { "attachment" }, m.name);
                // uploaded content must never execute as part of our origin
                Response::bytes(&m.content_type, data)
                    .header("Content-Disposition", disp)
                    .header("X-Content-Type-Options", "nosniff")
                    .header("Content-Security-Policy", "sandbox; default-src 'none'")
                    .header("Cache-Control", "private, no-store")
            }
            Err(e) => err(e),
        }
    });

    let info = authed!(drive, users, |d, me, req| {
        let Some(id) = id_param(req) else { return Response::not_found() };
        match d.metadata(&me, &id) {
            Ok(m) => Response::json(&m.public_value()),
            Err(e) => err(e),
        }
    });

    let overwrite = authed!(drive, users, |d, me, req| {
        let Some(id) = id_param(req) else { return Response::not_found() };
        match d.overwrite(&me, &id, &req.body) {
            Ok(m) => Response::json(&m.public_value()),
            Err(e) => err(e),
        }
    });

    let delete = authed!(drive, users, |d, me, req| {
        let Some(id) = id_param(req) else { return Response::not_found() };
        match d.delete(&me, &id) {
            Ok(()) => Response::ok_msg("deleted"),
            Err(e) => err(e),
        }
    });

    let (d2, u2) = (drive.clone(), users.clone());
    let share = move |req: &Request| {
        let Some(me) = u2.from_request(req) else { return Response::unauthorized() };
        let Some(id) = id_param(req) else { return Response::not_found() };
        let v = match body_json(req) {
            Ok(v) => v,
            Err(r) => return r,
        };
        // the target may be given by username or id
        let target = match v.get("username").and_then(|x| x.as_str()) {
            Some(n) => u2.find_by_name(n).map(|u| u.id),
            None => uuid_field(&v, "user_id"),
        };
        let Some(target) = target else { return Response::bad_request("unknown user") };
        let remove = v.get("level").map(|l| l.is_null()).unwrap_or(false) || v.get("level").and_then(|l| l.as_str()) == Some("none");
        if remove {
            return match d2.unshare(&me.id, &id, &target) {
                Ok(_) => Response::ok_msg("unshared"),
                Err(e) => err(e),
            };
        }
        let Some(level) = v.get("level").and_then(|l| l.as_str()).and_then(Level::parse) else {
            return Response::bad_request("level must be commenter, viewer or editor");
        };
        match d2.share(&me.id, &id, &target, level) {
            Ok(p) => Response::json(&p.public_value()),
            Err(e) => err(e),
        }
    };

    let mkdir = authed!(drive, users, |d, me, req| {
        let v = match body_json(req) {
            Ok(v) => v,
            Err(r) => return r,
        };
        let Some(name) = v.get("name").and_then(|n| n.as_str()) else { return Response::bad_request("name is required") };
        match d.mkdir(&me, name, uuid_field(&v, "parent_id")) {
            Ok(x) => Response::json(&x.public_value()).status(201),
            Err(e) => err(e),
        }
    });

    let pl_list = authed!(drive, users, |d, me, req| {
        match d.playlists_of(&me) {
            Ok(l) => Response::json(&utils::json_obj! {"playlists" => l.iter().map(|x| x.public_value()).collect::<Vec<_>>()}),
            Err(e) => err(e),
        }
    });

    let pl_create = authed!(drive, users, |d, me, req| {
        let v = match body_json(req) {
            Ok(v) => v,
            Err(r) => return r,
        };
        match d.create_playlist(&me, v.get("name").and_then(|n| n.as_str()).unwrap_or("")) {
            Ok(p) => Response::json(&p.public_value()).status(201),
            Err(e) => err(e),
        }
    });

    let pl_add = authed!(drive, users, |d, me, req| {
        let Some(id) = id_param(req) else { return Response::not_found() };
        let v = match body_json(req) {
            Ok(v) => v,
            Err(r) => return r,
        };
        let Some(file) = uuid_field(&v, "file_id") else { return Response::bad_request("file_id is required") };
        match d.playlist_add(&me, &id, &file) {
            Ok(p) => Response::json(&p.public_value()),
            Err(e) => err(e),
        }
    });

    Router::new()
        .post("/storage/files", upload)
        .get("/storage/files", list)
        .get("/storage/files/:id", download)
        .get("/storage/files/:id/info", info)
        .put("/storage/files/:id", overwrite)
        .delete("/storage/files/:id", delete)
        .post("/storage/files/:id/share", share)
        .post("/storage/dirs", mkdir)
        .get("/media/playlists", pl_list)
        .post("/media/playlists", pl_create)
        .post("/media/playlists/:id/add", pl_add)
        .tagged("public")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DriveConfig;
    use database::Database;
    use router::Method;

    fn setup() -> (Router, Users) {
        let db = Database::memory();
        let users = Users::with_config(db.scope("user").unwrap(), user::UsersConfig { password_iterations: 50, ..Default::default() }).unwrap();
        for n in ["alice", "bobby"] {
            users.create_user(n, "long enough password", false).unwrap();
        }
        let dir = std::env::temp_dir().join(format!("tunic-driveweb-{}", Uuid::new_v4()));
        let drive = Drive::new(db.scope("drive").unwrap(), dir, DriveConfig::default()).unwrap();
        (router(drive, users.clone()), users)
    }

    fn cookie(u: &Users, n: &str) -> String {
        format!("session_id={}", u.login(n, "long enough password").unwrap().1.id)
    }

    fn call(r: &Router, c: Option<&str>, m: Method, target: &str, body: &[u8], extra: &[(&str, &str)]) -> Response {
        let mut req = Request::new(m, target);
        req.body = body.to_vec();
        if let Some(c) = c {
            req = req.with_header("Cookie", c);
        }
        for (k, v) in extra {
            req = req.with_header(k, v);
        }
        r.handle(req)
    }

    #[test]
    fn upload_download_share_over_http() {
        let (r, u) = setup();
        let (a, b) = (cookie(&u, "alice"), cookie(&u, "bobby"));
        assert_eq!(call(&r, None, Method::Post, "/storage/files", b"x", &[]).status, 401);
        let up = call(&r, Some(&a), Method::Post, "/storage/files", b"<script>alert(1)</script>", &[("Og-Filename", "evil.html")]);
        assert_eq!(up.status, 201);
        let id = up.json_body().unwrap().get("id").unwrap().as_str().unwrap().to_string();
        let path = format!("/storage/files/{id}");

        let dl = call(&r, Some(&a), Method::Get, &path, b"", &[]);
        assert_eq!(dl.status, 200);
        assert!(dl.headers.get("Content-Disposition").unwrap().starts_with("attachment"), "html is never inlined");
        assert_eq!(dl.headers.get("X-Content-Type-Options"), Some("nosniff"));
        assert!(dl.headers.get("Content-Security-Policy").unwrap().contains("sandbox"));

        assert_eq!(call(&r, Some(&b), Method::Get, &path, b"", &[]).status, 404);
        let sh = call(&r, Some(&a), Method::Post, &format!("{path}/share"), br#"{"username":"bobby","level":"viewer"}"#, &[]);
        assert_eq!(sh.status, 200, "{}", sh.body_str());
        assert_eq!(call(&r, Some(&b), Method::Get, &path, b"", &[]).status, 200);
        assert_eq!(call(&r, Some(&b), Method::Delete, &path, b"", &[]).status, 403);
        assert_eq!(call(&r, Some(&b), Method::Post, &format!("{path}/share"), br#"{"username":"alice","level":"viewer"}"#, &[]).status, 403);
        let listed = call(&r, Some(&b), Method::Get, "/storage/files", b"", &[]).json_body().unwrap();
        assert_eq!(listed.get("files").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(call(&r, Some(&a), Method::Post, &format!("{path}/share"), br#"{"username":"bobby","level":"none"}"#, &[]).status, 200);
        assert_eq!(call(&r, Some(&b), Method::Get, &path, b"", &[]).status, 404);
        assert_eq!(call(&r, Some(&a), Method::Delete, &path, b"", &[]).status, 200);
        assert_eq!(call(&r, Some(&a), Method::Get, &path, b"", &[]).status, 404);
    }

    #[test]
    fn images_inline_and_playlists() {
        let (r, u) = setup();
        let a = cookie(&u, "alice");
        let up = call(&r, Some(&a), Method::Post, "/storage/files", b"\x89PNG", &[("Og-Filename", "p.png")]);
        let id = up.json_body().unwrap().get("id").unwrap().as_str().unwrap().to_string();
        let dl = call(&r, Some(&a), Method::Get, &format!("/storage/files/{id}"), b"", &[]);
        assert!(dl.headers.get("Content-Disposition").unwrap().starts_with("inline"));
        let pl = call(&r, Some(&a), Method::Post, "/media/playlists", br#"{"name":"Pics"}"#, &[]);
        let pid = pl.json_body().unwrap().get("id").unwrap().as_str().unwrap().to_string();
        let add = call(&r, Some(&a), Method::Post, &format!("/media/playlists/{pid}/add"), format!(r#"{{"file_id":"{id}"}}"#).as_bytes(), &[]);
        assert_eq!(add.status, 200, "{}", add.body_str());
        assert_eq!(call(&r, Some(&a), Method::Post, "/storage/dirs", br#"{"name":"Docs"}"#, &[]).status, 201);
        assert_eq!(call(&r, Some(&a), Method::Post, "/storage/dirs", b"not json", &[]).status, 400);
    }
}
