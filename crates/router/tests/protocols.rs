use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use router::*;
use utils::Shutdown;

fn app() -> Router {
    Router::new()
        .get("/", |_| Response::text("home"))
        .get("/page", |req| {
            let page = element::Page::new("Page").add(element::heading(2, "Hello")).add(element::link("/", "home"));
            Response::page(req, &page)
        })
        .post("/echo", |req| Response::bytes("application/octet-stream", req.body.clone()))
        .get("/who", |req| Response::text(format!("{} {:?}", req.protocol.name(), req.query_param("q"))))
        .get("/menu", |_| gopher_menu(&[GopherItem::info("hi"), GopherItem::link('0', "doc", "/who", "localhost", 70)]))
        .get("/redir", |_| Response::redirect("/"))
}

fn raw(addr: std::net::SocketAddr, data: &[u8]) -> Vec<u8> {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(data).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    out
}

fn start(p: Protocol) -> (std::net::SocketAddr, Shutdown, Listener) {
    let sd = Shutdown::new();
    let l = Server::new(app()).listen(p, "127.0.0.1:0", &sd).unwrap();
    (l.addr, sd, l)
}

#[test]
fn http_basics() {
    let (addr, sd, l) = start(Protocol::Http);
    let r = String::from_utf8(raw(addr, b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")).unwrap();
    assert!(r.starts_with("HTTP/1.1 200 OK\r\n"), "{r}");
    assert!(r.ends_with("home"));
    assert!(r.contains("Content-Length: 4"));

    let r = String::from_utf8(raw(addr, b"GET /nope HTTP/1.0\r\n\r\n")).unwrap();
    assert!(r.starts_with("HTTP/1.1 404"), "{r}");

    // HEAD has headers but no body
    let r = String::from_utf8(raw(addr, b"HEAD / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")).unwrap();
    assert!(r.contains("Content-Length: 4") && r.ends_with("\r\n\r\n"));

    // html page for http
    let r = String::from_utf8(raw(addr, b"GET /page HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")).unwrap();
    assert!(r.contains("<h2>Hello</h2>"));
    sd.trigger();
    l.join();
}

#[test]
fn http_bodies_keepalive_and_errors() {
    let (addr, sd, l) = start(Protocol::Http);
    // content-length body
    let r = raw(addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
    assert!(r.ends_with(b"hello"));
    // chunked body
    let r = raw(addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n2;ext=1\r\nde\r\n0\r\n\r\n");
    assert!(r.ends_with(b"abcde"), "{}", String::from_utf8_lossy(&r));
    // two pipelined keep-alive requests
    let r = String::from_utf8(raw(
        addr,
        b"GET / HTTP/1.1\r\nHost: x\r\n\r\nGET /who?q=a%20b HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    ))
    .unwrap();
    assert_eq!(r.matches("HTTP/1.1 200 OK").count(), 2, "{r}");
    assert!(r.contains("http Some(\"a b\")"));
    // smuggling attempt is rejected
    let r = String::from_utf8(raw(addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n")).unwrap();
    assert!(r.starts_with("HTTP/1.1 400"), "{r}");
    // missing host on 1.1, garbage, unknown method, bad version
    assert!(String::from_utf8(raw(addr, b"GET / HTTP/1.1\r\n\r\n")).unwrap().starts_with("HTTP/1.1 400"));
    assert!(String::from_utf8(raw(addr, b"garbage\r\n\r\n")).unwrap().starts_with("HTTP/1.1 400"));
    assert!(String::from_utf8(raw(addr, b"BREW / HTTP/1.1\r\nHost: x\r\n\r\n")).unwrap().starts_with("HTTP/1.1 501"));
    assert!(String::from_utf8(raw(addr, b"GET / HTTP/2.0\r\nHost: x\r\n\r\n")).unwrap().starts_with("HTTP/1.1 400"));
    // wrong method -> 405 with Allow
    let r = String::from_utf8(raw(addr, b"DELETE /echo HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")).unwrap();
    assert!(r.starts_with("HTTP/1.1 405") && r.contains("Allow: POST"), "{r}");
    sd.trigger();
    l.join();
}

#[test]
fn http_limits() {
    let sd = Shutdown::new();
    let l = Server::new(app())
        .limits(Limits { max_body_bytes: 10, ..Limits::default() })
        .listen(Protocol::Http, "127.0.0.1:0", &sd)
        .unwrap();
    let r = String::from_utf8(raw(l.addr, b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 11\r\n\r\n")).unwrap();
    assert!(r.starts_with("HTTP/1.1 413"), "{r}");
    sd.trigger();
    l.join();
}

#[test]
fn http_client_roundtrip_and_reverse_proxy() {
    let (upstream, sd1, l1) = start(Protocol::Http);
    let proxy_router = Router::new().get("/local", |_| Response::text("local")).reverse_proxy("site.test", &upstream.to_string());
    let sd2 = Shutdown::new();
    let l2 = Server::new(proxy_router).listen(Protocol::Http, "127.0.0.1:0", &sd2).unwrap();

    let p = l2.addr.to_string();
    let r = http_request(&p, "GET", "/who?q=zz", &[("Host", "site.test")], b"", Duration::from_secs(5)).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body_str(), "http Some(\"zz\")");
    let r = http_request(&p, "POST", "/echo", &[("Host", "site.test")], b"proxied body", Duration::from_secs(5)).unwrap();
    assert_eq!(r.body_str(), "proxied body");
    // other hosts hit the local routes
    let r = http_request(&p, "GET", "/local", &[], b"", Duration::from_secs(5)).unwrap();
    assert_eq!(r.body_str(), "local");
    // dead upstream -> 502
    let dead = Router::new().reverse_proxy("d.test", "127.0.0.1:1");
    let sd3 = Shutdown::new();
    let l3 = Server::new(dead).listen(Protocol::Http, "127.0.0.1:0", &sd3).unwrap();
    let r = http_request(&l3.addr.to_string(), "GET", "/", &[("Host", "d.test")], b"", Duration::from_secs(5)).unwrap();
    assert_eq!(r.status, 502);
    for (s, l) in [(sd1, l1), (sd2, l2), (sd3, l3)] {
        s.trigger();
        l.join();
    }
}

#[test]
fn gopher() {
    let (addr, sd, l) = start(Protocol::Gopher);
    let r = String::from_utf8(raw(addr, b"/menu\r\n")).unwrap();
    assert_eq!(r, "ihi\t\terror.host\t1\r\n0doc\t/who\tlocalhost\t70\r\n.\r\n");
    let r = String::from_utf8(raw(addr, b"/who\tsearch term\r\n")).unwrap();
    assert_eq!(r, "gopher Some(\"search term\")\r\n.\r\n");
    let r = String::from_utf8(raw(addr, b"/page\r\n")).unwrap();
    assert!(r.contains("HELLO") && r.ends_with(".\r\n"), "{r}");
    let r = String::from_utf8(raw(addr, b"/missing\r\n")).unwrap();
    assert!(r.starts_with("3"), "{r}");
    // empty selector = root
    assert!(String::from_utf8(raw(addr, b"\r\n")).unwrap().starts_with("home"));
    sd.trigger();
    l.join();
}

#[test]
fn gemini() {
    let (addr, sd, l) = start(Protocol::Gemini);
    let r = String::from_utf8(raw(addr, b"gemini://localhost/page\r\n")).unwrap();
    assert!(r.starts_with("20 text/gemini"), "{r}");
    assert!(r.contains("## Hello") && r.contains("=> / home"), "{r}");
    let r = String::from_utf8(raw(addr, b"gemini://localhost/who?hello%20there\r\n")).unwrap();
    assert!(r.ends_with("gemini Some(\"hello there\")"), "{r}");
    assert!(String::from_utf8(raw(addr, b"gemini://localhost/redir\r\n")).unwrap().starts_with("30 /"));
    assert!(String::from_utf8(raw(addr, b"gemini://localhost/nope\r\n")).unwrap().starts_with("51 "));
    assert!(String::from_utf8(raw(addr, b"http://localhost/\r\n")).unwrap().starts_with("59 "));
    sd.trigger();
    l.join();
}

#[test]
fn shutdown_stops_accepting() {
    let (addr, sd, l) = start(Protocol::Http);
    sd.trigger();
    l.join();
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err());
}
