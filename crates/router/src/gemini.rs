//! Gemini codec. Gemini mandates TLS: supply a TLS [`crate::Acceptor`] for this protocol in
//! production, or terminate TLS in front of tunic.

use std::io::Write;
use std::time::Instant;

use crate::server::{Conn, read_line, wait_for_data};
use crate::{Method, Protocol, Request, Response, http_to_gemini, normalize_host, parse_query, reason};
use utils::percent_decode;

pub(crate) fn serve(conn: &mut Conn) {
    if !wait_for_data(conn, conn.limits.request_timeout) {
        return;
    }
    let deadline = Instant::now() + conn.limits.request_timeout;
    let line = match read_line(&mut conn.reader, 1024, deadline) {
        Ok(Some(l)) => l,
        Ok(None) => return,
        Err(_) => {
            let _ = conn.reader.get_mut().write_all(b"59 Bad request\r\n");
            return;
        }
    };
    let reply = match String::from_utf8(line).ok().and_then(|l| parse_url(&l)) {
        None => b"59 Bad request\r\n".to_vec(),
        Some((host, path, query)) => {
            let mut req = Request::new(Method::Get, &path);
            req.path = percent_decode(&path, false);
            req.protocol = Protocol::Gemini;
            req.host = Some(host);
            req.peer = conn.peer;
            req.local_port = conn.local_port;
            req.query = if query.is_empty() {
                Vec::new()
            } else if query.contains('=') {
                parse_query(&query)
            } else {
                // gemini input: the whole (decoded) query string is the user's answer
                vec![("q".into(), percent_decode(&query, false))]
            };
            encode(&conn.router.handle(req))
        }
    };
    let _ = conn.reader.get_mut().write_all(&reply);
    let _ = conn.reader.get_mut().flush();
}

/// `gemini://host[:port]/path?query` -> (host, path, query)
fn parse_url(line: &str) -> Option<(String, String, String)> {
    let rest = line.trim().strip_prefix("gemini://")?;
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let tail = tail.split('#').next().unwrap_or("");
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, q),
        None => (tail, ""),
    };
    let path = if path.is_empty() { "/" } else { path };
    Some((normalize_host(authority), path.to_string(), query.to_string()))
}

fn encode(resp: &Response) -> Vec<u8> {
    let code = http_to_gemini(resp.status);
    match code {
        20 => {
            let mut out = format!("20 {}\r\n", resp.content_type.replace(['\r', '\n'], " ")).into_bytes();
            out.extend_from_slice(&resp.body);
            out
        }
        30 | 31 => {
            let loc = resp.headers.get("location").unwrap_or("/");
            format!("{code} {loc}\r\n").into_bytes()
        }
        _ => {
            let msg = crate::gopher::error_text(resp);
            let msg = if msg.is_empty() { reason(resp.status).to_string() } else { msg };
            format!("{code} {}\r\n", msg.chars().take(1000).collect::<String>()).into_bytes()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(parse_url("gemini://Example.com:1965/a%20b?x"), Some(("example.com".into(), "/a%20b".into(), "x".into())));
        assert_eq!(parse_url("gemini://example.com"), Some(("example.com".into(), "/".into(), "".into())));
        assert_eq!(parse_url("http://example.com/"), None);
        assert_eq!(parse_url("gemini:///x"), None);
        assert_eq!(parse_url("gemini://user@example.com/"), None);
    }

    #[test]
    fn encoding() {
        assert_eq!(encode(&Response::gemtext("# hi")), b"20 text/gemini; charset=utf-8\r\n# hi".to_vec());
        assert_eq!(encode(&Response::redirect("/x")), b"30 /x\r\n".to_vec());
        assert_eq!(encode(&Response::not_found()), b"51 Not Found\r\n".to_vec());
    }
}
