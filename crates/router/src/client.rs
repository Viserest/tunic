//! Tiny blocking HTTP/1.1 client (Connection: close). Used by the reverse proxy and by tests.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::Headers;

#[derive(Debug, Clone)]
pub struct ClientResponse {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl ClientResponse {
    pub fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

const MAX_RESPONSE: usize = 256 * 1024 * 1024;

/// Send one request to `addr` (`host:port`) and read the complete response.
pub fn request(
    addr: &str,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> io::Result<ClientResponse> {
    let sock = utils::net::resolve(addr).map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    let mut s = TcpStream::connect_timeout(&sock, timeout)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let mut head = format!("{method} {target} HTTP/1.1\r\n");
    let has = |n: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(n));
    if !has("host") {
        head.push_str(&format!("Host: {addr}\r\n"));
    }
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("connection") {
            continue;
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || matches!(method, "POST" | "PUT" | "PATCH") {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(body)?;
    s.flush()?;

    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let status: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad status line"))?;
    let mut hdrs = Headers::new();
    loop {
        line.clear();
        r.read_line(&mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            hdrs.append(k, v.trim());
        }
    }
    let mut body = Vec::new();
    let chunked = hdrs.get("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
    if method == "HEAD" || status == 204 || status == 304 {
        // no body
    } else if chunked {
        loop {
            line.clear();
            r.read_line(&mut line)?;
            let n = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk"))?;
            if n == 0 {
                break;
            }
            if body.len() + n > MAX_RESPONSE {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "response too large"));
            }
            let start = body.len();
            body.resize(start + n, 0);
            r.read_exact(&mut body[start..])?;
            line.clear();
            r.read_line(&mut line)?; // CRLF
        }
    } else if let Some(n) = hdrs.get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        if n > MAX_RESPONSE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "response too large"));
        }
        body.resize(n, 0);
        r.read_exact(&mut body)?;
    } else {
        r.take(MAX_RESPONSE as u64).read_to_end(&mut body)?;
    }
    Ok(ClientResponse { status, headers: hdrs, body })
}
