//! HTTP/1.0 and HTTP/1.1 codec.

use std::io::{self, Read, Write};
use std::time::Instant;

use crate::server::{Conn, read_exact_deadline, read_line, wait_for_data};
use crate::{Headers, Method, Protocol, Request, Response, normalize_host, reason, split_target};

const MAX_HEADERS: usize = 100;
const MAX_REQUESTS_PER_CONNECTION: usize = 10_000;

enum Parsed {
    Request { req: Request, keep_alive: bool },
    Reject(u16, &'static str),
    Closed,
}

pub(crate) fn serve(conn: &mut Conn) {
    for _ in 0..MAX_REQUESTS_PER_CONNECTION {
        if !wait_for_data(conn, conn.limits.keepalive_timeout) {
            return;
        }
        match read_request(conn) {
            Ok(Parsed::Request { req, keep_alive }) => {
                let head = req.method == Method::Head;
                let resp = conn.router.handle(req);
                if write_response(conn.reader.get_mut(), &resp, head, keep_alive).is_err() || !keep_alive {
                    return;
                }
            }
            Ok(Parsed::Reject(status, msg)) => {
                let resp = Response::error(status, msg);
                let _ = write_response(conn.reader.get_mut(), &resp, false, false);
                return;
            }
            Ok(Parsed::Closed) | Err(_) => return,
        }
    }
}

fn read_request(conn: &mut Conn) -> io::Result<Parsed> {
    let deadline = Instant::now() + conn.limits.request_timeout;
    let max = conn.limits.max_header_bytes;

    // request line (ignore leading empty lines, RFC 9112 2.2)
    let line = loop {
        match read_line(&mut conn.reader, max, deadline)? {
            None => return Ok(Parsed::Closed),
            Some(l) if l.is_empty() => continue,
            Some(l) => break l,
        }
    };
    let line = match String::from_utf8(line) {
        Ok(l) => l,
        Err(_) => return Ok(Parsed::Reject(400, "Bad Request")),
    };
    let mut parts = line.split(' ');
    let (Some(m), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Ok(Parsed::Reject(400, "Malformed request line"));
    };
    let http11 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Ok(Parsed::Reject(400, "Unsupported HTTP version")),
    };
    let Some(method) = Method::parse(m) else {
        return Ok(Parsed::Reject(501, "Method not implemented"));
    };
    if target.len() > 8192 {
        return Ok(Parsed::Reject(414, "URI too long"));
    }
    if !target.starts_with('/') && target != "*" {
        return Ok(Parsed::Reject(400, "Only origin-form request targets are supported"));
    }

    // headers
    let mut headers = Headers::new();
    let mut total = line.len();
    loop {
        let Some(l) = read_line(&mut conn.reader, max, deadline)? else {
            return Ok(Parsed::Closed);
        };
        if l.is_empty() {
            break;
        }
        total += l.len();
        if total > max || headers.0.len() >= MAX_HEADERS {
            return Ok(Parsed::Reject(431, "Request header fields too large"));
        }
        let Ok(l) = String::from_utf8(l) else {
            return Ok(Parsed::Reject(400, "Bad header encoding"));
        };
        let Some((k, v)) = l.split_once(':') else {
            return Ok(Parsed::Reject(400, "Malformed header"));
        };
        if k.is_empty() || k.contains(|c: char| c.is_whitespace() || c.is_control()) {
            return Ok(Parsed::Reject(400, "Malformed header name"));
        }
        headers.append(k, v.trim());
    }

    // body framing
    let te = headers.get("transfer-encoding").map(|s| s.to_ascii_lowercase());
    let cls: Vec<&str> = headers.get_all("content-length").collect();
    if te.is_some() && !cls.is_empty() {
        return Ok(Parsed::Reject(400, "Both Content-Length and Transfer-Encoding"));
    }
    if cls.windows(2).any(|w| w[0] != w[1]) {
        return Ok(Parsed::Reject(400, "Conflicting Content-Length"));
    }
    if http11 && headers.get("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
        conn.reader.get_mut().write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let mut body = Vec::new();
    if let Some(te) = te {
        if te != "chunked" {
            return Ok(Parsed::Reject(501, "Unsupported Transfer-Encoding"));
        }
        loop {
            let Some(sz) = read_line(&mut conn.reader, 256, deadline)? else {
                return Ok(Parsed::Closed);
            };
            let sz = String::from_utf8_lossy(&sz).into_owned();
            let Ok(n) = usize::from_str_radix(sz.split(';').next().unwrap_or("").trim(), 16) else {
                return Ok(Parsed::Reject(400, "Bad chunk size"));
            };
            if n == 0 {
                // trailers
                while let Some(t) = read_line(&mut conn.reader, max, deadline)? {
                    if t.is_empty() {
                        break;
                    }
                }
                break;
            }
            if body.len() + n > conn.limits.max_body_bytes {
                return Ok(Parsed::Reject(413, "Payload too large"));
            }
            body.extend(read_exact_deadline(&mut conn.reader, n, deadline)?);
            // chunk terminator
            let _ = read_line(&mut conn.reader, 4, deadline)?;
        }
    } else if let Some(cl) = cls.first() {
        let Ok(n) = cl.trim().parse::<usize>() else {
            return Ok(Parsed::Reject(400, "Bad Content-Length"));
        };
        if n > conn.limits.max_body_bytes {
            return Ok(Parsed::Reject(413, "Payload too large"));
        }
        body = read_exact_deadline(&mut conn.reader, n, deadline)?;
    }

    let conn_hdr = headers.get("connection").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let keep_alive = if http11 { !conn_hdr.contains("close") } else { conn_hdr.contains("keep-alive") };
    if http11 && headers.get("host").is_none() {
        return Ok(Parsed::Reject(400, "Missing Host header"));
    }

    let (path, query) = split_target(target);
    let host = headers.get("host").map(normalize_host);
    let req = Request {
        protocol: Protocol::Http,
        method,
        path,
        target: target.to_string(),
        query,
        headers,
        body,
        host,
        peer: conn.peer,
        local_port: conn.local_port,
        params: Default::default(),
        extensions: Default::default(),
    };
    Ok(Parsed::Request { req, keep_alive })
}

pub(crate) fn write_response(w: &mut dyn Write, resp: &Response, head_only: bool, keep_alive: bool) -> io::Result<()> {
    let mut out = Vec::with_capacity(256 + resp.body.len());
    out.extend_from_slice(format!("HTTP/1.1 {} {}\r\n", resp.status, reason(resp.status)).as_bytes());
    for (k, v) in resp.headers.iter() {
        if matches!(k.to_ascii_lowercase().as_str(), "content-length" | "connection" | "transfer-encoding" | "content-type") {
            continue;
        }
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            continue; // header injection guard
        }
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    let no_body = resp.status == 204 || resp.status == 304 || (100..200).contains(&resp.status);
    if !no_body {
        out.extend_from_slice(format!("Content-Type: {}\r\n", resp.content_type.replace(['\r', '\n'], "")).as_bytes());
        out.extend_from_slice(format!("Content-Length: {}\r\n", resp.body.len()).as_bytes());
    }
    out.extend_from_slice(b"X-Content-Type-Options: nosniff\r\nServer: tunic\r\n");
    out.extend_from_slice(if keep_alive { b"Connection: keep-alive\r\n" } else { b"Connection: close\r\n" });
    out.extend_from_slice(b"\r\n");
    if !head_only && !no_body {
        out.extend_from_slice(&resp.body);
    }
    w.write_all(&out)?;
    w.flush()
}

#[allow(dead_code)]
pub(crate) fn drain(r: &mut impl Read) {
    let mut buf = [0u8; 1024];
    while matches!(r.read(&mut buf), Ok(n) if n > 0) {}
}
