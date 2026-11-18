//! Reverse proxy handler (like a minimal caddy/nginx `proxy_pass`).

use std::time::Duration;

use crate::{Handler, Headers, Request, Response, http_request};

const HOP_BY_HOP: &[&str] =
    &["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade", "content-length", "host"];

/// Forward every request to `upstream` (`host:port`) and relay the answer.
pub fn proxy_handler(upstream: &str) -> impl Fn(&Request) -> Response + Send + Sync + 'static + use<> {
    let upstream = upstream.to_string();
    move |req: &Request| forward(&upstream, req)
}

#[allow(dead_code)]
fn _assert_handler(h: Handler) -> Handler {
    h
}

fn forward(upstream: &str, req: &Request) -> Response {
    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|(k, _)| !HOP_BY_HOP.contains(&k.to_ascii_lowercase().as_str()))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if let Some(h) = req.headers.get("host") {
        headers.push(("Host".into(), h.to_string()));
        headers.push(("X-Forwarded-Host".into(), h.to_string()));
    }
    if let Some(p) = req.peer {
        headers.push(("X-Forwarded-For".into(), p.ip().to_string()));
    }
    headers.push(("X-Forwarded-Proto".into(), req.protocol.name().into()));
    let hdr_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    match http_request(upstream, req.method.as_str(), &req.target, &hdr_refs, &req.body, Duration::from_secs(30)) {
        Ok(up) => {
            let mut out = Response::new(up.status);
            out.content_type = up.headers.get("content-type").unwrap_or("application/octet-stream").to_string();
            let mut h = Headers::new();
            for (k, v) in up.headers.iter() {
                if !HOP_BY_HOP.contains(&k.to_ascii_lowercase().as_str()) && !k.eq_ignore_ascii_case("content-type") {
                    h.append(k, v);
                }
            }
            out.headers = h;
            out.body = up.body;
            out
        }
        Err(e) => Response::error(502, format!("Bad gateway: {e}")),
    }
}
