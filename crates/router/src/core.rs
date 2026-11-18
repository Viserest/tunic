use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;

use utils::{Value, percent_decode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Http,
    Gopher,
    Gemini,
}

impl Protocol {
    pub const fn default_port(&self) -> u16 {
        match self {
            Protocol::Http => 80,
            Protocol::Gopher => 70,
            Protocol::Gemini => 1965,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Protocol::Http => "http",
            Protocol::Gopher => "gopher",
            Protocol::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Head,
    Options,
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn parse(s: &str) -> Option<Method> {
        Some(match s {
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "PATCH" => Method::Patch,
            "DELETE" => Method::Delete,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Case-insensitive header list (insertion ordered, repeats allowed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Headers(pub Vec<(String, String)>);

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.0.iter().filter(move |(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    /// Replace any existing header of that name.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        self.remove(name);
        self.0.push((name.to_string(), value.into()));
    }
    /// Add without removing existing ones (e.g. `Set-Cookie`).
    pub fn append(&mut self, name: &str, value: impl Into<String>) {
        self.0.push((name.to_string(), value.into()));
    }
    pub fn remove(&mut self, name: &str) {
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub protocol: Protocol,
    /// Http: the request method. Gopher and Gemini are always `GET`.
    pub method: Method,
    /// Percent-decoded path, always starting with `/`.
    pub path: String,
    /// The request target as received (path + query).
    pub target: String,
    pub query: Vec<(String, String)>,
    pub headers: Headers,
    pub body: Vec<u8>,
    /// Host name from the `Host` header / gemini URL (lower-case, without port).
    pub host: Option<String>,
    pub peer: Option<SocketAddr>,
    /// Port the request arrived on.
    pub local_port: u16,
    /// Path parameters filled in by the router (`/user/:id`).
    pub params: HashMap<String, String>,
    /// Free-form per-request data (middleware may attach e.g. the session user).
    pub extensions: HashMap<String, String>,
}

impl Request {
    pub fn new(method: Method, target: &str) -> Request {
        let (path, query) = split_target(target);
        Request {
            protocol: Protocol::Http,
            method,
            path,
            target: target.to_string(),
            query,
            headers: Headers::new(),
            body: Vec::new(),
            host: None,
            peer: None,
            local_port: 0,
            params: HashMap::new(),
            extensions: HashMap::new(),
        }
    }

    pub fn get(target: &str) -> Request {
        Request::new(Method::Get, target)
    }

    pub fn post(target: &str, body: impl Into<Vec<u8>>) -> Request {
        let mut r = Request::new(Method::Post, target);
        r.body = body.into();
        r
    }

    pub fn with_header(mut self, k: &str, v: &str) -> Self {
        if k.eq_ignore_ascii_case("host") {
            self.host = Some(normalize_host(v));
        }
        self.headers.set(k, v);
        self
    }

    pub fn with_peer(mut self, peer: SocketAddr) -> Self {
        self.peer = Some(peer);
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        for h in self.headers.get_all("cookie") {
            for part in h.split(';') {
                if let Some((k, v)) = part.trim().split_once('=') {
                    if k == name {
                        return Some(v.to_string());
                    }
                }
            }
        }
        None
    }

    pub fn bearer_token(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ")
    }

    pub fn body_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.body).ok()
    }

    /// Body parsed as JSON.
    pub fn json(&self) -> Result<Value, String> {
        let s = self.body_str().ok_or("body is not utf-8")?;
        utils::json::parse(s).map_err(|e| e.to_string())
    }

    /// Body parsed as `application/x-www-form-urlencoded`.
    pub fn form(&self) -> Vec<(String, String)> {
        parse_query(self.body_str().unwrap_or(""))
    }

    pub fn is_http(&self) -> bool {
        self.protocol == Protocol::Http
    }
}

pub fn normalize_host(h: &str) -> String {
    let h = h.trim().to_ascii_lowercase();
    if h.starts_with('[') {
        return h.split(']').next().unwrap_or("").trim_start_matches('[').to_string();
    }
    match h.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host.to_string(),
        _ => h,
    }
}

pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (percent_decode(k, true), percent_decode(v, true)),
            None => (percent_decode(p, true), String::new()),
        })
        .collect()
}

/// Split `/a%20b?x=1` into (decoded path, decoded query pairs).
pub fn split_target(target: &str) -> (String, Vec<(String, String)>) {
    let target = target.split('#').next().unwrap_or("");
    let (p, q) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };
    let mut path = percent_decode(p, false);
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    (path, parse_query(q))
}

#[derive(Debug, Clone)]
pub struct Response {
    /// Http status code. Gemini and Gopher codecs translate it.
    pub status: u16,
    /// Http `Content-Type` / gemini meta on success / gopher item hint.
    pub content_type: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Default for Response {
    fn default() -> Self {
        Response::not_found()
    }
}

pub const CT_TEXT: &str = "text/plain; charset=utf-8";
pub const CT_HTML: &str = "text/html; charset=utf-8";
pub const CT_JSON: &str = "application/json";
pub const CT_GEMTEXT: &str = "text/gemini; charset=utf-8";
pub const CT_GOPHER_MENU: &str = "application/x-gopher-menu";

impl Response {
    pub fn new(status: u16) -> Response {
        Response { status, content_type: CT_TEXT.into(), headers: Headers::new(), body: Vec::new() }
    }
    pub fn bytes(content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        Response { status: 200, content_type: content_type.into(), headers: Headers::new(), body: body.into() }
    }
    pub fn text(s: impl Into<String>) -> Response {
        Response::bytes(CT_TEXT, s.into().into_bytes())
    }
    pub fn html(s: impl Into<String>) -> Response {
        Response::bytes(CT_HTML, s.into().into_bytes())
    }
    pub fn gemtext(s: impl Into<String>) -> Response {
        Response::bytes(CT_GEMTEXT, s.into().into_bytes())
    }
    pub fn json(v: &Value) -> Response {
        Response::bytes(CT_JSON, v.to_json().into_bytes())
    }
    /// A page rendered for the protocol of `req`: html, gemtext or plain text.
    pub fn page(req: &Request, page: &element::Page) -> Response {
        match req.protocol {
            Protocol::Http => Response::html(page.to_html()),
            Protocol::Gemini => Response::gemtext(page.to_gemtext()),
            Protocol::Gopher => Response::text(page.to_text()),
        }
    }
    /// `{"msg": msg}` with the given status.
    pub fn error(status: u16, msg: impl Into<String>) -> Response {
        let mut r = Response::json(&utils::json_obj! {"msg" => msg.into()});
        r.status = status;
        r
    }
    pub fn ok_msg(msg: impl Into<String>) -> Response {
        Response::json(&utils::json_obj! {"msg" => msg.into()})
    }
    pub fn not_found() -> Response {
        let mut r = Response::text("Not Found");
        r.status = 404;
        r
    }
    pub fn bad_request(msg: impl Into<String>) -> Response {
        Response::error(400, msg)
    }
    pub fn unauthorized() -> Response {
        Response::error(401, "Unauthorized")
    }
    pub fn forbidden() -> Response {
        Response::error(403, "Forbidden")
    }
    pub fn internal(msg: impl Into<String>) -> Response {
        Response::error(500, msg)
    }
    pub fn redirect(location: &str) -> Response {
        let mut r = Response::new(302);
        r.headers.set("Location", location);
        r.body = format!("Redirecting to {location}").into_bytes();
        r
    }
    pub fn status(mut self, status: u16) -> Response {
        self.status = status;
        self
    }
    pub fn header(mut self, k: &str, v: impl Into<String>) -> Response {
        self.headers.set(k, v);
        self
    }
    pub fn content_type(mut self, ct: &str) -> Response {
        self.content_type = ct.into();
        self
    }
    /// Add a `Set-Cookie` header (HttpOnly, SameSite=Lax, Path=/).
    pub fn set_cookie(mut self, name: &str, value: &str, max_age: Option<u64>) -> Response {
        let mut c = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
        if let Some(a) = max_age {
            c.push_str(&format!("; Max-Age={a}"));
        }
        self.headers.append("Set-Cookie", c);
        self
    }
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn json_body(&self) -> Result<Value, String> {
        utils::json::parse(&self.body_str()).map_err(|e| e.to_string())
    }
}

pub fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Map an http status to the closest Gemini status code.
pub fn http_to_gemini(status: u16) -> u8 {
    match status {
        200..=299 => 20,
        301 | 308 => 31,
        300..=399 => 30,
        400 => 59,
        401 => 60,
        403 => 61,
        404 => 51,
        410 => 52,
        429 => 44,
        502 | 504 => 43,
        503 => 41,
        400..=499 => 59,
        500..=599 => 40,
        _ => 40,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_parsing() {
        let (p, q) = split_target("/a%20b/c?x=1&y=hello+world&z#frag");
        assert_eq!(p, "/a b/c");
        assert_eq!(q, vec![("x".into(), "1".into()), ("y".into(), "hello world".into()), ("z".into(), "".into())]);
    }

    #[test]
    fn cookies_and_headers() {
        let r = Request::get("/").with_header("Cookie", "a=1; session_id=abc; b=2").with_header("Host", "Example.COM:8080");
        assert_eq!(r.cookie("session_id").as_deref(), Some("abc"));
        assert_eq!(r.host.as_deref(), Some("example.com"));
        assert_eq!(r.header("COOKIE").is_some(), true);
    }

    #[test]
    fn status_mapping() {
        assert_eq!(http_to_gemini(200), 20);
        assert_eq!(http_to_gemini(404), 51);
        assert_eq!(http_to_gemini(302), 30);
        assert_eq!(http_to_gemini(500), 40);
    }

    #[test]
    fn response_builders() {
        let r = Response::error(400, "no");
        assert_eq!(r.status, 400);
        assert_eq!(r.json_body().unwrap().get("msg").unwrap().as_str(), Some("no"));
        assert_eq!(Response::redirect("/x").headers.get("location"), Some("/x"));
    }

    #[test]
    fn page_negotiation() {
        let page = element::Page::new("T").add(element::text("hi"));
        let mut req = Request::get("/");
        assert!(Response::page(&req, &page).content_type.starts_with("text/html"));
        req.protocol = Protocol::Gemini;
        assert!(Response::page(&req, &page).content_type.starts_with("text/gemini"));
    }
}
