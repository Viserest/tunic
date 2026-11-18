use std::io::{self, BufReader};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

use utils::{Value, json_obj, lock};

use crate::wire::{self, HELLO, MAGIC, REQUEST, RESPONSE, SYNC};
use crate::{Backend, DbError, Entry};

struct Conn {
    r: BufReader<TcpStream>,
    w: TcpStream,
}

/// Uses a database running in another process (a [`crate::DbServer`]) as a [`Backend`].
///
/// Keeps a small pool of connections and transparently reconnects once if one has gone stale.
pub struct RemoteBackend {
    addr: String,
    token: Option<String>,
    timeout: Duration,
    pool: Mutex<Vec<Conn>>,
}

const POOL_MAX: usize = 8;

impl RemoteBackend {
    /// Connects once to verify the address and token.
    pub fn connect(addr: &str, token: Option<&str>) -> Result<RemoteBackend, DbError> {
        let b = RemoteBackend { addr: addr.to_string(), token: token.map(String::from), timeout: Duration::from_secs(30), pool: Mutex::new(Vec::new()) };
        let c = b.dial().map_err(|e| DbError::Remote(format!("cannot connect to database at {addr}: {e}")))?;
        lock(&b.pool).push(c);
        Ok(b)
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    fn dial(&self) -> io::Result<Conn> {
        let sock = utils::net::resolve(&self.addr).map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
        let s = TcpStream::connect_timeout(&sock, self.timeout)?;
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(self.timeout))?;
        s.set_write_timeout(Some(self.timeout))?;
        let mut w = s.try_clone()?;
        let mut r = BufReader::new(s);
        let mut hello = json_obj! {"magic" => MAGIC};
        if let Some(t) = &self.token {
            hello.set("token", t.as_str());
        }
        wire::write_frame(&mut w, HELLO, &hello)?;
        match wire::read_frame(&mut r, None)? {
            Some((RESPONSE, v)) if v.get("ok").and_then(|o| o.as_bool()) == Some(true) => Ok(Conn { r, w }),
            Some((_, v)) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                v.get("error").and_then(|e| e.as_str()).unwrap_or("handshake rejected").to_string(),
            )),
            None => Err(io::ErrorKind::UnexpectedEof.into()),
        }
    }

    fn exchange(c: &mut Conn, ty: u8, req: &Value) -> io::Result<Value> {
        wire::write_frame(&mut c.w, ty, req)?;
        match wire::read_frame(&mut c.r, None)? {
            Some((RESPONSE, v)) => Ok(v),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected reply")),
        }
    }

    fn call(&self, ty: u8, req: &Value) -> Result<Value, DbError> {
        let mut last = None;
        for attempt in 0..2 {
            let pooled = if attempt == 0 { lock(&self.pool).pop() } else { None };
            let mut c = match pooled {
                Some(c) => c,
                None => self.dial().map_err(|e| DbError::Remote(format!("{}: {e}", self.addr)))?,
            };
            match Self::exchange(&mut c, ty, req) {
                Ok(v) => {
                    let mut pool = lock(&self.pool);
                    if pool.len() < POOL_MAX {
                        pool.push(c);
                    }
                    drop(pool);
                    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
                        return Ok(v);
                    }
                    let msg = v.get("error").and_then(|e| e.as_str()).unwrap_or("error").to_string();
                    return Err(if msg.starts_with("access denied") { DbError::Denied(msg) } else { DbError::Remote(msg) });
                }
                Err(e) => last = Some(e), // stale connection: retry once on a fresh one
            }
        }
        Err(DbError::Remote(format!("{}: {}", self.addr, last.map(|e| e.to_string()).unwrap_or_default())))
    }

    fn op(&self, op: &str, ns: &str, col: &str, id: &str, value: Option<&Value>) -> Result<Value, DbError> {
        let mut req = json_obj! {"op" => op, "ns" => ns, "col" => col, "id" => id};
        if let Some(v) = value {
            req.set("value", v.clone());
        }
        self.call(REQUEST, &req)
    }
}

impl Backend for RemoteBackend {
    fn get(&self, ns: &str, col: &str, id: &str) -> Result<Option<Value>, DbError> {
        let r = self.op("get", ns, col, id, None)?;
        Ok(r.get("value").filter(|v| !v.is_null()).cloned())
    }
    fn put(&self, ns: &str, col: &str, id: &str, value: &Value) -> Result<(), DbError> {
        self.op("put", ns, col, id, Some(value)).map(|_| ())
    }
    fn delete(&self, ns: &str, col: &str, id: &str) -> Result<bool, DbError> {
        Ok(self.op("delete", ns, col, id, None)?.get("deleted").and_then(|d| d.as_bool()).unwrap_or(false))
    }
    fn list(&self, ns: &str, col: &str) -> Result<Vec<(String, Value)>, DbError> {
        let r = self.op("list", ns, col, "", None)?;
        Ok(r.get("items")
            .and_then(|i| i.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.get("value")?.clone())))
                    .collect()
            })
            .unwrap_or_default())
    }
    fn collections(&self, ns: &str) -> Result<Vec<String>, DbError> {
        let r = self.op("collections", ns, "", "", None)?;
        Ok(r.get("collections").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default())
    }
    fn dump(&self, prefix: &str) -> Result<Vec<Entry>, DbError> {
        let r = self.call(SYNC, &json_obj! {"prefix" => prefix})?;
        Ok(r.get("entries")
            .and_then(|e| e.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some(Entry {
                            ns: e.get("ns")?.as_str()?.to_string(),
                            col: e.get("col")?.as_str()?.to_string(),
                            id: e.get("id")?.as_str()?.to_string(),
                            value: e.get("value")?.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}
