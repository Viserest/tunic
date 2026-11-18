use std::collections::HashMap;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use utils::{Shutdown, Value, json_obj};

use crate::wire::{self, HELLO, MAGIC, REQUEST, RESPONSE, SYNC};
use crate::{Backend, Scope};

/// Access tokens for a [`DbServer`]: each token is limited to one scope (and its children).
/// An empty scope means "everything".
#[derive(Clone, Debug, Default)]
pub struct Tokens {
    map: HashMap<String, String>,
    /// Accept clients which present no token (full access). Only for trusted networks.
    pub open: bool,
}

impl Tokens {
    /// No authentication at all.
    pub fn open() -> Self {
        Tokens { map: HashMap::new(), open: true }
    }
    pub fn new() -> Self {
        Tokens::default()
    }
    /// Token with access to every scope.
    pub fn admin(self, token: &str) -> Self {
        self.scoped(token, "")
    }
    /// Token limited to `scope`.
    pub fn scoped(mut self, token: &str, scope: &str) -> Self {
        self.map.insert(token.to_string(), scope.trim_matches('/').to_string());
        self
    }
    fn authorize(&self, token: Option<&str>) -> Option<String> {
        match token {
            Some(t) => {
                // compare against every token without early exit
                let mut found = None;
                for (k, scope) in &self.map {
                    if utils::password::ct_eq(k.as_bytes(), t.as_bytes()) {
                        found = Some(scope.clone());
                    }
                }
                found.or_else(|| self.open.then(String::new))
            }
            None => self.open.then(String::new),
        }
    }
}

/// Serves a [`Backend`] over TCP so other nodes can use it as their database.
pub struct DbServer {
    pub addr: SocketAddr,
    handle: JoinHandle<()>,
}

impl DbServer {
    pub fn start(backend: Arc<dyn Backend>, addr: &str, tokens: Tokens, shutdown: &Shutdown) -> io::Result<DbServer> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        let tokens = Arc::new(tokens);
        let shutdown = shutdown.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let handle = std::thread::Builder::new().name(format!("db-accept-{}", local.port())).spawn(move || {
            while !shutdown.is_triggered() {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                        let (backend, tokens, sd, active) = (backend.clone(), tokens.clone(), shutdown.clone(), active.clone());
                        active.fetch_add(1, Ordering::SeqCst);
                        let spawned = std::thread::Builder::new().name("db-conn".into()).spawn({
                            let active = active.clone();
                            move || {
                                let _ = serve_conn(stream, peer, backend, &tokens, &sd);
                                active.fetch_sub(1, Ordering::SeqCst);
                            }
                        });
                        if spawned.is_err() {
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        shutdown.sleep(Duration::from_millis(10));
                    }
                    Err(_) => {
                        shutdown.sleep(Duration::from_millis(50));
                    }
                }
            }
            let until = Instant::now() + Duration::from_secs(3);
            while active.load(Ordering::SeqCst) > 0 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
        })?;
        Ok(DbServer { addr: local, handle })
    }

    pub fn join(self) {
        let _ = self.handle.join();
    }
}

fn fail(msg: impl Into<String>) -> Value {
    json_obj! {"ok" => false, "error" => msg.into()}
}

fn serve_conn(stream: TcpStream, _peer: SocketAddr, backend: Arc<dyn Backend>, tokens: &Tokens, sd: &Shutdown) -> io::Result<()> {
    let mut w = stream.try_clone()?;
    let mut r = BufReader::new(stream);

    // handshake
    let allowed = match wire::read_frame(&mut r, Some(sd))? {
        Some((HELLO, hello)) if hello.get("magic").and_then(|m| m.as_str()) == Some(MAGIC) => {
            match tokens.authorize(hello.get("token").and_then(|t| t.as_str())) {
                Some(scope) => {
                    wire::write_frame(&mut w, RESPONSE, &json_obj! {"ok" => true, "scope" => scope.as_str()})?;
                    scope
                }
                None => {
                    wire::write_frame(&mut w, RESPONSE, &fail("authentication failed"))?;
                    return Ok(());
                }
            }
        }
        _ => return Ok(()),
    };
    let allowed_scope = if allowed.is_empty() { None } else { Scope::new(&allowed).ok() };
    let permits = |ns: &str| match &allowed_scope {
        None => allowed.is_empty(),
        Some(s) => s.contains(ns),
    };

    while let Some((ty, req)) = wire::read_frame(&mut r, Some(sd))? {
        let resp = match ty {
            REQUEST => handle_request(&*backend, &req, &permits),
            SYNC => {
                let prefix = req.get("prefix").and_then(|p| p.as_str()).unwrap_or("");
                if (prefix.is_empty() && allowed.is_empty()) || (!prefix.is_empty() && permits(prefix)) {
                    match backend.dump(prefix) {
                        Ok(entries) => {
                            let arr: Vec<Value> = entries
                                .into_iter()
                                .map(|e| json_obj! {"ns" => e.ns, "col" => e.col, "id" => e.id, "value" => e.value})
                                .collect();
                            json_obj! {"ok" => true, "entries" => arr}
                        }
                        Err(e) => fail(e.to_string()),
                    }
                } else {
                    fail("access denied")
                }
            }
            _ => fail("unexpected frame type"),
        };
        wire::write_frame(&mut w, RESPONSE, &resp)?;
    }
    Ok(())
}

fn handle_request(backend: &dyn Backend, req: &Value, permits: &dyn Fn(&str) -> bool) -> Value {
    let s = |k: &str| req.get(k).and_then(|v| v.as_str());
    let (Some(op), Some(ns)) = (s("op"), s("ns")) else { return fail("malformed request") };
    if !permits(ns) {
        return fail(format!("access denied to scope `{ns}`"));
    }
    let col = s("col").unwrap_or("");
    let id = s("id").unwrap_or("");
    let result = match op {
        "get" => backend.get(ns, col, id).map(|v| json_obj! {"ok" => true, "value" => v}),
        "put" => match req.get("value") {
            Some(v) => backend.put(ns, col, id, v).map(|_| json_obj! {"ok" => true}),
            None => return fail("missing value"),
        },
        "delete" => backend.delete(ns, col, id).map(|d| json_obj! {"ok" => true, "deleted" => d}),
        "list" => backend.list(ns, col).map(|l| {
            let arr: Vec<Value> = l.into_iter().map(|(id, v)| json_obj! {"id" => id, "value" => v}).collect();
            json_obj! {"ok" => true, "items" => arr}
        }),
        "collections" => backend.collections(ns).map(|c| json_obj! {"ok" => true, "collections" => c}),
        _ => return fail("unknown op"),
    };
    result.unwrap_or_else(|e| fail(e.to_string()))
}
