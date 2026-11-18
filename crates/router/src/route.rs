use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use utils::NetworkRule;

use crate::{Method, Request, Response, normalize_host};

pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync + 'static>;

#[derive(Clone, Debug, PartialEq)]
enum Seg {
    Lit(String),
    Param(String),
    Rest(String),
}

fn parse_pattern(p: &str) -> Vec<Seg> {
    p.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(n) = s.strip_prefix(':') {
                Seg::Param(n.to_string())
            } else if let Some(n) = s.strip_prefix('*') {
                Seg::Rest(if n.is_empty() { "rest".into() } else { n.to_string() })
            } else {
                Seg::Lit(s.to_string())
            }
        })
        .collect()
}

fn match_path(segs: &[Seg], path: &str) -> Option<HashMap<String, String>> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut params = HashMap::new();
    for (i, seg) in segs.iter().enumerate() {
        match seg {
            Seg::Lit(l) => {
                if parts.get(i)? != l {
                    return None;
                }
            }
            Seg::Param(n) => {
                params.insert(n.clone(), parts.get(i)?.to_string());
            }
            Seg::Rest(n) => {
                params.insert(n.clone(), parts.get(i..).unwrap_or(&[]).join("/"));
                return Some(params);
            }
        }
    }
    if parts.len() == segs.len() { Some(params) } else { None }
}

#[derive(Clone)]
struct Route {
    method: Option<Method>,
    segs: Vec<Seg>,
    handler: Handler,
    tag: Option<String>,
}

/// Which route tags are served and from where.
///
/// A route carrying tag `admin` is only reachable when the tag is enabled, and (if an access rule
/// exists for the tag) only from client addresses inside the rule.
#[derive(Clone, Debug)]
pub struct Policy {
    pub all_tags: bool,
    pub tags: HashSet<String>,
    pub access: HashMap<String, Vec<NetworkRule>>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { all_tags: true, tags: HashSet::new(), access: HashMap::new() }
    }
}

impl Policy {
    pub fn tag_enabled(&self, tag: &str) -> bool {
        self.all_tags || self.tags.contains(tag)
    }
    pub fn allow(&mut self, tag: &str, rule: NetworkRule) {
        self.access.entry(tag.to_string()).or_default().push(rule);
    }
    pub fn permits(&self, tag: &str, peer: Option<std::net::SocketAddr>) -> bool {
        match (self.access.get(tag), peer) {
            (None, _) => true,
            // in-process calls (tests, library use) have no peer address
            (_, None) => true,
            (Some(rules), Some(p)) => rules.iter().any(|r| r.contains(p.ip())),
        }
    }
}

/// List of routes (method, path pattern, optional tag) and a handler per route.
///
/// Patterns: `/literal`, `/:param`, `/*rest`. Hosts (virtual hosts and reverse proxies) are
/// routed before paths.
#[derive(Clone, Default)]
pub struct Router {
    routes: Vec<Route>,
    fallback: Option<Handler>,
    hosts: Vec<(String, Router)>,
    pub policy: Policy,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn route<F>(mut self, method: Option<Method>, pattern: &str, handler: F) -> Self
    where
        F: Fn(&Request) -> Response + Send + Sync + 'static,
    {
        self.routes.push(Route { method, segs: parse_pattern(pattern), handler: Arc::new(handler), tag: None });
        self
    }

    pub fn get<F: Fn(&Request) -> Response + Send + Sync + 'static>(self, p: &str, h: F) -> Self {
        self.route(Some(Method::Get), p, h)
    }
    pub fn post<F: Fn(&Request) -> Response + Send + Sync + 'static>(self, p: &str, h: F) -> Self {
        self.route(Some(Method::Post), p, h)
    }
    pub fn put<F: Fn(&Request) -> Response + Send + Sync + 'static>(self, p: &str, h: F) -> Self {
        self.route(Some(Method::Put), p, h)
    }
    pub fn delete<F: Fn(&Request) -> Response + Send + Sync + 'static>(self, p: &str, h: F) -> Self {
        self.route(Some(Method::Delete), p, h)
    }
    /// Any method.
    pub fn any<F: Fn(&Request) -> Response + Send + Sync + 'static>(self, p: &str, h: F) -> Self {
        self.route(None, p, h)
    }

    /// Set the handler for requests which match nothing.
    pub fn fallback<F: Fn(&Request) -> Response + Send + Sync + 'static>(mut self, h: F) -> Self {
        self.fallback = Some(Arc::new(h));
        self
    }

    /// Tag every route that has no tag yet.
    pub fn tagged(mut self, tag: &str) -> Self {
        for r in &mut self.routes {
            r.tag.get_or_insert_with(|| tag.to_string());
        }
        self
    }

    /// Mount `other` under `prefix`. Tags on `other`'s routes are kept.
    pub fn nest(mut self, prefix: &str, other: Router) -> Self {
        let pre = parse_pattern(prefix);
        for mut r in other.routes {
            let mut segs = pre.clone();
            segs.extend(r.segs);
            r.segs = segs;
            self.routes.push(r);
        }
        self
    }

    /// Add all routes of `other` (same as `nest("/", other)`).
    pub fn merge(self, other: Router) -> Self {
        self.nest("/", other)
    }

    /// Serve `router` for requests whose `Host` is `host`.
    pub fn host(mut self, host: &str, router: Router) -> Self {
        self.hosts.push((normalize_host(host), router));
        self
    }

    /// Reverse proxy: forward everything for `host` to `upstream` (`host:port`).
    pub fn reverse_proxy(self, host: &str, upstream: &str) -> Self {
        let r = Router::new().fallback(crate::proxy::proxy_handler(upstream));
        self.host(host, r)
    }

    pub fn with_policy(mut self, p: Policy) -> Self {
        self.policy = p;
        self
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    /// Patterns and tags of the registered routes, for dashboards.
    pub fn describe(&self) -> Vec<(String, String, Option<String>)> {
        self.routes
            .iter()
            .map(|r| {
                let pat: String = r
                    .segs
                    .iter()
                    .map(|s| match s {
                        Seg::Lit(l) => format!("/{l}"),
                        Seg::Param(p) => format!("/:{p}"),
                        Seg::Rest(p) => format!("/*{p}"),
                    })
                    .collect();
                (r.method.map(|m| m.to_string()).unwrap_or_else(|| "ANY".into()), if pat.is_empty() { "/".into() } else { pat }, r.tag.clone())
            })
            .collect()
    }

    /// Handle a request. Never panics: handler panics become `500`.
    pub fn handle(&self, mut req: Request) -> Response {
        // 1. virtual hosts / reverse proxies
        if let Some(host) = req.host.clone() {
            if let Some((_, sub)) = self.hosts.iter().find(|(h, _)| *h == host) {
                let mut sub = sub.clone();
                sub.policy = self.policy.clone();
                return sub.handle_paths(req);
            }
        }
        req.params.clear();
        self.handle_paths(req)
    }

    fn handle_paths(&self, mut req: Request) -> Response {
        let method = if req.method == Method::Head { Method::Get } else { req.method };
        let mut path_matched = false;
        let mut allowed: Vec<&'static str> = Vec::new();
        for r in &self.routes {
            let Some(params) = match_path(&r.segs, &req.path) else { continue };
            path_matched = true;
            if let Some(m) = r.method {
                if m != method {
                    allowed.push(m.as_str());
                    continue;
                }
            }
            if let Some(tag) = &r.tag {
                if !self.policy.tag_enabled(tag) {
                    continue; // as if it did not exist
                }
                if !self.policy.permits(tag, req.peer) {
                    return Response::forbidden();
                }
            }
            req.params = params;
            let h = r.handler.clone();
            return run(&h, &req);
        }
        if let Some(f) = &self.fallback {
            return run(f, &req);
        }
        if path_matched && !allowed.is_empty() {
            allowed.sort();
            allowed.dedup();
            return Response::error(405, "Method Not Allowed").header("Allow", allowed.join(", "));
        }
        Response::not_found()
    }
}

fn run(h: &Handler, req: &Request) -> Response {
    match catch_unwind(AssertUnwindSafe(|| h(req))) {
        Ok(r) => r,
        Err(_) => Response::internal("Internal server error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &'static str) -> impl Fn(&Request) -> Response + Send + Sync + 'static {
        move |_| Response::text(s)
    }

    #[test]
    fn matches_methods_and_params() {
        let r = Router::new()
            .get("/", ok("home"))
            .get("/user/:id", |req| Response::text(req.param("id").unwrap().to_string()))
            .post("/user/:id", ok("post"))
            .get("/files/*path", |req| Response::text(req.param("path").unwrap().to_string()));
        assert_eq!(r.handle(Request::get("/")).body_str(), "home");
        assert_eq!(r.handle(Request::get("/user/42")).body_str(), "42");
        assert_eq!(r.handle(Request::post("/user/42", "")).body_str(), "post");
        assert_eq!(r.handle(Request::get("/files/a/b/c.txt")).body_str(), "a/b/c.txt");
        assert_eq!(r.handle(Request::get("/nope")).status, 404);
        let m = r.handle(Request::new(Method::Delete, "/user/1"));
        assert_eq!(m.status, 405);
        assert_eq!(m.headers.get("allow"), Some("GET, POST"));
        // trailing slash tolerated, HEAD falls back to GET
        assert_eq!(r.handle(Request::get("/user/7/")).body_str(), "7");
        assert_eq!(r.handle(Request::new(Method::Head, "/")).status, 200);
    }

    #[test]
    fn nest_and_tags() {
        let inner = Router::new().get("/page", ok("p")).tagged("admin");
        let mut r = Router::new().nest("/app", inner).get("/pub", ok("pub")).tagged("public");
        assert_eq!(r.handle(Request::get("/app/page")).body_str(), "p");
        // disable admin
        r.policy.all_tags = false;
        r.policy.tags.insert("public".into());
        assert_eq!(r.handle(Request::get("/app/page")).status, 404);
        assert_eq!(r.handle(Request::get("/pub")).status, 200);
    }

    #[test]
    fn access_rules() {
        let mut r = Router::new().get("/admin", ok("a")).tagged("admin");
        r.policy.allow("admin", NetworkRule::parse("10.1.0.0/16").unwrap());
        let inside = Request::get("/admin").with_peer("10.1.2.3:5555".parse().unwrap());
        let outside = Request::get("/admin").with_peer("8.8.8.8:5555".parse().unwrap());
        assert_eq!(r.handle(inside).status, 200);
        assert_eq!(r.handle(outside).status, 403);
        assert_eq!(r.handle(Request::get("/admin")).status, 200); // in-process, no peer
    }

    #[test]
    fn hosts() {
        let r = Router::new()
            .get("/", ok("default"))
            .host("a.example.com", Router::new().get("/", ok("site a")));
        assert_eq!(r.handle(Request::get("/").with_header("Host", "a.example.com:8080")).body_str(), "site a");
        assert_eq!(r.handle(Request::get("/").with_header("Host", "other.com")).body_str(), "default");
    }

    #[test]
    fn panics_become_500() {
        let r = Router::new().get("/boom", |_| panic!("boom"));
        assert_eq!(r.handle(Request::get("/boom")).status, 500);
    }

    #[test]
    fn fallback() {
        let r = Router::new().fallback(|req| Response::text(format!("fb {}", req.path)));
        assert_eq!(r.handle(Request::get("/x")).body_str(), "fb /x");
    }
}
