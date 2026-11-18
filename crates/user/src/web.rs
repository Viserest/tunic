use element::{Element, Page, heading, link, section, text};
use router::{Request, Response, Router};
use utils::{Uuid, Value, json_obj};

use crate::Users;

fn login_page(req: &Request, base: &str, error: Option<&str>) -> Response {
    let mut page = Page::new("Login").add(heading(1, "Login"));
    if let Some(e) = error {
        page = page.add(text(e));
    }
    let form = Element::new("form")
        .attr("method", "post")
        .attr("action", format!("{base}/login"))
        .child(Element::new("input").attr("type", "text").attr("name", "username").attr("placeholder", "Username").attr("autocomplete", "username"))
        .child(Element::new("br"))
        .child(Element::new("input").attr("type", "password").attr("name", "password").attr("placeholder", "Password").attr("autocomplete", "current-password"))
        .child(Element::new("br"))
        .child(Element::new("input").attr("type", "submit").attr("value", "Login"));
    page = page.add(form);
    if !req.is_http() {
        page = page.add(text("Logging in requires http."));
    }
    Response::page(req, &page)
}

fn wants_json(req: &Request) -> bool {
    req.header("content-type").is_some_and(|c| c.starts_with("application/json"))
}

/// Login, logout, home, profile, password reset and (admin) user management.
///
/// `base` is where the router is mounted (`"/user"`), used for redirects and links.
/// Public routes (`login`, `logout`) carry tag `public`; everything else carries `admin` for
/// user management and `public` for the logged-in home/profile pages.
pub fn router(users: Users, base: &str) -> Router {
    let base = base.trim_end_matches('/').to_string();

    let u = users.clone();
    let b = base.clone();
    let login_get = move |req: &Request| -> Response {
        if u.from_request(req).is_some() {
            return Response::redirect(&format!("{b}/home"));
        }
        login_page(req, &b, None)
    };

    let u = users.clone();
    let b = base.clone();
    let login_post = move |req: &Request| -> Response {
        let (name, pass, json) = if wants_json(req) {
            match req.json() {
                Ok(v) => (
                    v.get("username").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    v.get("password").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    true,
                ),
                Err(e) => return Response::bad_request(e),
            }
        } else {
            let f = req.form();
            let get = |k: &str| f.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()).unwrap_or_default();
            (get("username"), get("password"), false)
        };
        match u.login(&name, &pass) {
            Ok((user, session)) => {
                let ttl = u.config.session_ttl;
                let resp = if json {
                    Response::json(&json_obj! {"msg" => "Successfully logged in", "location" => format!("{b}/home"), "session_id" => session.id, "is_admin" => user.is_admin})
                } else {
                    Response::redirect(&format!("{b}/home")).status(303)
                };
                resp.set_cookie("session_id", &session.id.to_string(), Some(ttl))
            }
            Err(e) => {
                if json {
                    Response::error(401, e)
                } else {
                    login_page(req, &b, Some(&e)).status(401)
                }
            }
        }
    };

    let u = users.clone();
    let b = base.clone();
    let logout = move |req: &Request| -> Response {
        if let Some(id) = req.cookie("session_id").and_then(|s| Uuid::parse(&s)) {
            u.logout(&id);
        }
        let r = if wants_json(req) { Response::ok_msg("Logged out") } else { Response::redirect(&format!("{b}/login")).status(303) };
        r.set_cookie("session_id", "", Some(0))
    };

    let public = Router::new().get("/login", login_get).post("/login", login_post).post("/logout", logout).tagged("public");

    let u = users.clone();
    let b = base.clone();
    let home = move |req: &Request| -> Response {
        let Some(user) = u.from_request(req) else { return Response::redirect(&format!("{b}/login")) };
        let mut page = Page::new("Home").add(heading(1, format!("Welcome, {}", user.username)));
        let mut nav = vec![link(format!("{b}/me"), "Profile")];
        if user.is_admin {
            nav.push(link(format!("{b}/users"), "Users"));
        }
        page = page.add(section(nav));
        page = page.add(Element::new("form").attr("method", "post").attr("action", format!("{b}/logout")).child(Element::new("input").attr("type", "submit").attr("value", "Logout")));
        Response::page(req, &page)
    };

    let u = users.clone();
    let me = move |req: &Request| -> Response {
        match u.from_request(req) {
            Some(user) => Response::json(&json_obj! {"id" => user.id, "username" => user.username, "is_admin" => user.is_admin, "created_at" => user.created_at}),
            None => Response::unauthorized(),
        }
    };

    let u = users.clone();
    let reset = move |req: &Request| -> Response {
        let Some(user) = u.from_request(req) else { return Response::unauthorized() };
        let body = match req.json() {
            Ok(b) => b,
            Err(e) => return Response::bad_request(e),
        };
        let s = |k: &str| body.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match u.reset_password(&user.id, &s("old_password"), &s("new_password")) {
            Ok(()) => Response::ok_msg("Successfully changed your password."),
            Err(e) => Response::bad_request(e),
        }
    };

    let private = Router::new().get("/home", home).get("/me", me).post("/reset-password", reset).tagged("public");

    // ---- admin
    let admin_only = |users: Users, f: fn(&Users, &Request) -> Response| {
        move |req: &Request| match users.from_request(req) {
            Some(u) if u.is_admin => f(&users, req),
            Some(_) => Response::forbidden(),
            None => Response::unauthorized(),
        }
    };
    let list = admin_only(users.clone(), |u, req| {
        let list = u.list();
        if req.header("accept").is_some_and(|a| a.contains("application/json")) {
            let arr: Vec<Value> = list.iter().map(|x| json_obj! {"id" => x.id, "username" => x.username.as_str(), "is_admin" => x.is_admin}).collect();
            return Response::json(&json_obj! {"users" => arr});
        }
        let rows: Vec<Vec<String>> = list.iter().map(|x| vec![x.username.clone(), x.is_admin.to_string(), x.id.to_string()]).collect();
        Response::page(req, &Page::new("Users").add(heading(1, "Users")).add(element::table(["Username", "Admin", "Id"], rows)))
    });
    let create = admin_only(users.clone(), |u, req| {
        let b = match req.json() {
            Ok(b) => b,
            Err(e) => return Response::bad_request(e),
        };
        let s = |k: &str| b.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match u.create_user(&s("username"), &s("password"), b.get("is_admin").and_then(|a| a.as_bool()).unwrap_or(false)) {
            Ok(user) => Response::json(&json_obj! {"msg" => "Successfully created", "content" => user.id}).status(201),
            Err(e) => Response::bad_request(e),
        }
    });
    let delete = admin_only(users.clone(), |u, req| {
        let id = req.json().ok().and_then(|b| b.get("id").and_then(|i| i.as_str()).and_then(Uuid::parse));
        match id {
            Some(id) if u.from_request(req).is_some_and(|me| me.id == id) => Response::bad_request("you cannot delete yourself"),
            Some(id) if u.delete_user(&id) => Response::ok_msg("Successfully deleted"),
            Some(_) => Response::error(404, "Nothing found"),
            None => Response::bad_request("`id` (uuid) is required"),
        }
    });
    let set_pw = admin_only(users.clone(), |u, req| {
        let b = match req.json() {
            Ok(b) => b,
            Err(e) => return Response::bad_request(e),
        };
        let id = b.get("id").and_then(|i| i.as_str()).and_then(Uuid::parse);
        let pw = b.get("password").and_then(|p| p.as_str()).unwrap_or("");
        match id {
            Some(id) if pw.chars().count() >= u.config.min_password_len => match u.set_password(&id, pw) {
                Ok(()) => Response::ok_msg("Successfully changed password"),
                Err(e) => Response::error(404, e),
            },
            _ => Response::bad_request("`id` and a long enough `password` are required"),
        }
    });
    let admin = Router::new()
        .get("/users", list)
        .post("/users/create", create)
        .post("/users/delete", delete)
        .post("/users/password", set_pw)
        .tagged("admin");

    Router::new().merge(public).merge(private).merge(admin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::Database;

    fn app() -> (Router, Users) {
        let u = Users::with_config(Database::memory().scope("users").unwrap(), crate::UsersConfig { password_iterations: 100, ..Default::default() }).unwrap();
        u.create_user("root", "root password", true).unwrap();
        u.create_user("bob", "bobs password", false).unwrap();
        (router(u.clone(), "/user"), u)
    }

    fn json_post(path: &str, body: &str) -> Request {
        Request::post(path, body).with_header("content-type", "application/json")
    }

    fn login(r: &Router, name: &str, pw: &str) -> String {
        let resp = r.handle(json_post("/login", &format!(r#"{{"username":"{name}","password":"{pw}"}}"#)));
        assert_eq!(resp.status, 200, "{}", resp.body_str());
        let c = resp.headers.get("set-cookie").unwrap().to_string();
        c.split(';').next().unwrap().to_string()
    }

    #[test]
    fn login_flow_json_and_form() {
        let (r, _) = app();
        assert_eq!(r.handle(json_post("/login", r#"{"username":"root","password":"bad"}"#)).status, 401);
        let cookie = login(&r, "root", "root password");
        let me = r.handle(Request::get("/me").with_header("cookie", &cookie));
        assert_eq!(me.json_body().unwrap().get("username").unwrap().as_str(), Some("root"));
        assert_eq!(r.handle(Request::get("/me")).status, 401);
        // browser style form post redirects and sets the cookie
        let f = r.handle(Request::post("/login", "username=bob&password=bobs+password"));
        assert_eq!(f.status, 303);
        assert!(f.headers.get("set-cookie").unwrap().contains("HttpOnly"));
        assert_eq!(f.headers.get("location"), Some("/user/home"));
        let bad = r.handle(Request::post("/login", "username=bob&password=x"));
        assert_eq!(bad.status, 401);
        assert!(bad.body_str().contains("invalid username or password"));
        // home needs a session
        assert_eq!(r.handle(Request::get("/home")).status, 302);
        assert!(r.handle(Request::get("/home").with_header("cookie", &cookie)).body_str().contains("Welcome, root"));
        // logout ends the session
        r.handle(Request::post("/logout", "").with_header("cookie", &cookie));
        assert_eq!(r.handle(Request::get("/me").with_header("cookie", &cookie)).status, 401);
    }

    #[test]
    fn admin_routes_are_protected() {
        let (r, u) = app();
        let admin = login(&r, "root", "root password");
        let bob = login(&r, "bob", "bobs password");
        assert_eq!(r.handle(Request::get("/users")).status, 401);
        assert_eq!(r.handle(Request::get("/users").with_header("cookie", &bob)).status, 403);
        assert_eq!(r.handle(Request::get("/users").with_header("cookie", &admin)).status, 200);
        let c = r.handle(json_post("/users/create", r#"{"username":"newbie","password":"newbie password"}"#).with_header("cookie", &admin));
        assert_eq!(c.status, 201);
        assert!(u.find_by_name("newbie").is_some());
        assert_eq!(r.handle(json_post("/users/create", r#"{"username":"x","password":"newbie password"}"#).with_header("cookie", &bob)).status, 403);
        let id = u.find_by_name("newbie").unwrap().id;
        assert_eq!(r.handle(json_post("/users/delete", &format!(r#"{{"id":"{id}"}}"#)).with_header("cookie", &admin)).status, 200);
        let me = u.find_by_name("root").unwrap().id;
        assert_eq!(r.handle(json_post("/users/delete", &format!(r#"{{"id":"{me}"}}"#)).with_header("cookie", &admin)).status, 400);
    }

    #[test]
    fn password_reset_route() {
        let (r, _) = app();
        let bob = login(&r, "bob", "bobs password");
        let bad = r.handle(json_post("/reset-password", r#"{"old_password":"nope","new_password":"brand new pass"}"#).with_header("cookie", &bob));
        assert_eq!(bad.status, 400);
        let ok = r.handle(json_post("/reset-password", r#"{"old_password":"bobs password","new_password":"brand new pass"}"#).with_header("cookie", &bob));
        assert_eq!(ok.status, 200);
        login(&r, "bob", "brand new pass");
    }

    #[test]
    fn tags_are_set() {
        let (r, _) = app();
        let tags: Vec<_> = r.describe().into_iter().map(|(_, p, t)| (p, t)).collect();
        assert!(tags.contains(&("/login".into(), Some("public".into()))));
        assert!(tags.contains(&("/users".into(), Some("admin".into()))));
    }
}
