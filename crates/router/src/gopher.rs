//! Gopher (RFC 1436) codec.

use std::io::Write;
use std::time::Instant;

use crate::server::{Conn, read_line, wait_for_data};
use crate::{CT_GOPHER_MENU, Method, Protocol, Request, Response};

/// One line of a gopher menu.
pub struct GopherItem {
    /// Item type: `0` text, `1` menu, `7` search, `9` binary, `I` image, `h` html, `i` info, `3` error.
    pub kind: char,
    pub display: String,
    pub selector: String,
    pub host: String,
    pub port: u16,
}

impl GopherItem {
    pub fn info(text: impl Into<String>) -> Self {
        GopherItem { kind: 'i', display: text.into(), selector: String::new(), host: "error.host".into(), port: 1 }
    }
    pub fn link(kind: char, display: impl Into<String>, selector: impl Into<String>, host: &str, port: u16) -> Self {
        GopherItem { kind, display: display.into(), selector: selector.into(), host: host.into(), port }
    }
}

/// Render menu lines (the codec adds the terminating `.`).
pub fn menu(items: &[GopherItem]) -> Response {
    let mut s = String::new();
    for i in items {
        let clean = |t: &str| t.replace(['\t', '\r', '\n'], " ");
        s.push_str(&format!("{}{}\t{}\t{}\t{}\r\n", i.kind, clean(&i.display), clean(&i.selector), i.host, i.port));
    }
    Response::bytes(CT_GOPHER_MENU, s.into_bytes())
}

pub(crate) fn serve(conn: &mut Conn) {
    if !wait_for_data(conn, conn.limits.request_timeout) {
        return;
    }
    let deadline = Instant::now() + conn.limits.request_timeout;
    let Ok(Some(line)) = read_line(&mut conn.reader, 1024, deadline) else { return };
    let line = String::from_utf8_lossy(&line).into_owned();
    let (selector, search) = match line.split_once('\t') {
        Some((s, q)) => (s.to_string(), Some(q.to_string())),
        None => (line, None),
    };
    let selector = selector.trim().to_string();
    let path = if selector.starts_with('/') { selector.clone() } else { format!("/{selector}") };

    let mut req = Request::new(Method::Get, &path);
    // The selector is taken literally (no percent-decoding, no query string)
    req.path = path;
    req.query.clear();
    req.protocol = Protocol::Gopher;
    req.peer = conn.peer;
    req.local_port = conn.local_port;
    if let Some(q) = search {
        let q = q.split('\t').next().unwrap_or("").to_string();
        req.query.push(("q".into(), q));
    }
    let resp = conn.router.handle(req);
    let _ = conn.reader.get_mut().write_all(&encode(&resp));
    let _ = conn.reader.get_mut().flush();
}

fn encode(resp: &Response) -> Vec<u8> {
    if !resp.is_success() {
        let msg = error_message(resp);
        return format!("3{msg}\t\terror.host\t1\r\n.\r\n").into_bytes();
    }
    if resp.content_type.starts_with(CT_GOPHER_MENU) {
        let mut b = resp.body.clone();
        b.extend_from_slice(b".\r\n");
        return b;
    }
    if resp.content_type.starts_with("text/") {
        // text files: CRLF line endings, dot-stuffing and a terminating period
        let text = String::from_utf8_lossy(&resp.body);
        let mut out = String::new();
        for line in text.lines() {
            if line.starts_with('.') {
                out.push('.');
            }
            out.push_str(line);
            out.push_str("\r\n");
        }
        out.push_str(".\r\n");
        return out.into_bytes();
    }
    resp.body.clone() // binary: raw bytes until the connection closes
}

fn error_message(resp: &Response) -> String {
    let body = resp.body_str();
    let msg = utils::json::parse(&body)
        .ok()
        .and_then(|v| v.get("msg").and_then(|m| m.as_str().map(String::from)))
        .unwrap_or_else(|| crate::reason(resp.status).to_string());
    msg.replace(['\t', '\r', '\n'], " ")
}

pub(crate) fn error_text(resp: &Response) -> String {
    error_message(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_format() {
        let r = menu(&[GopherItem::info("Welcome"), GopherItem::link('1', "Docs", "/docs", "localhost", 70)]);
        assert_eq!(r.body_str(), "iWelcome\t\terror.host\t1\r\n1Docs\t/docs\tlocalhost\t70\r\n");
        assert!(encode(&r).ends_with(b".\r\n"));
    }

    #[test]
    fn text_is_dot_stuffed() {
        let r = Response::text("a\n.hidden\nb");
        assert_eq!(String::from_utf8(encode(&r)).unwrap(), "a\r\n..hidden\r\nb\r\n.\r\n");
    }

    #[test]
    fn errors_are_menu_errors() {
        let e = String::from_utf8(encode(&Response::error(404, "nothing here"))).unwrap();
        assert!(e.starts_with("3nothing here\t"));
    }
}
