use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use utils::{Shutdown, Uuid, Value, json_obj, lock, unix_now};

const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub id: Uuid,
    pub name: String,
    /// Address to listen on (`0.0.0.0:7000`). `None` = passive node.
    pub listen: Option<String>,
    /// Address other nodes should use to reach this one. Defaults to the bound address; an
    /// unspecified IP (`0.0.0.0`) is replaced by the address peers see us connecting from.
    pub advertise: Option<String>,
    /// Nodes to contact on start (and to keep contacting).
    pub join: Vec<String>,
    pub roles: Vec<String>,
    /// Shared secret every node of the mesh must present.
    pub token: Option<String>,
    pub max_peers: usize,
    pub gossip_interval: Duration,
    pub peer_timeout: Duration,
}

impl Default for NodeConfig {
    fn default() -> Self {
        NodeConfig {
            id: Uuid::new_v4(),
            name: String::new(),
            listen: None,
            advertise: None,
            join: Vec::new(),
            roles: Vec::new(),
            token: None,
            max_peers: 128,
            gossip_interval: Duration::from_secs(5),
            peer_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeerInfo {
    pub id: Uuid,
    pub name: String,
    /// `None` for passive peers (cannot be called).
    pub addr: Option<String>,
    pub roles: Vec<String>,
    pub last_seen: u64,
}

impl PeerInfo {
    fn to_value(&self) -> Value {
        json_obj! {
            "id" => self.id, "name" => self.name.as_str(), "addr" => self.addr.clone(),
            "roles" => self.roles.clone(), "last_seen" => self.last_seen,
        }
    }
    fn from_value(v: &Value) -> Option<PeerInfo> {
        Some(PeerInfo {
            id: Uuid::parse(v.get("id")?.as_str()?)?,
            name: v.get("name").and_then(|n| n.as_str()).unwrap_or("").chars().take(128).collect(),
            addr: v.get("addr").and_then(|a| a.as_str()).map(String::from),
            roles: v
                .get("roles")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.chars().take(64).collect())).take(32).collect())
                .unwrap_or_default(),
            last_seen: v.get("last_seen").and_then(|l| l.as_u64()).unwrap_or(0),
        })
    }
}

/// A service: `(method, payload) -> result`.
pub type ServiceFn = Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;

pub struct Node {
    cfg: NodeConfig,
    roles: RwLock<Vec<String>>,
    peers: Mutex<HashMap<Uuid, PeerInfo>>,
    services: RwLock<HashMap<String, ServiceFn>>,
    local: Mutex<Option<SocketAddr>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

fn write_msg(s: &mut TcpStream, v: &Value) -> io::Result<()> {
    let p = v.to_json().into_bytes();
    let mut buf = (p.len() as u32).to_be_bytes().to_vec();
    buf.extend_from_slice(&p);
    s.write_all(&buf)?;
    s.flush()
}

fn read_msg(s: &mut TcpStream) -> io::Result<Value> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    let t = String::from_utf8(buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not utf-8"))?;
    utils::json::parse(&t).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

impl Node {
    pub fn new(cfg: NodeConfig) -> Arc<Node> {
        let roles = RwLock::new(cfg.roles.clone());
        Arc::new(Node {
            cfg,
            roles,
            peers: Mutex::new(HashMap::new()),
            services: RwLock::new(HashMap::new()),
            local: Mutex::new(None),
            threads: Mutex::new(Vec::new()),
        })
    }

    pub fn id(&self) -> Uuid {
        self.cfg.id
    }
    pub fn config(&self) -> &NodeConfig {
        &self.cfg
    }

    /// The address the node is listening on (after [`Node::start`]).
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *lock(&self.local)
    }

    pub fn roles(&self) -> Vec<String> {
        self.roles.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Advertise an additional role (takes effect on the next gossip round).
    pub fn add_role(&self, role: &str) {
        let mut r = self.roles.write().unwrap_or_else(|e| e.into_inner());
        if !r.iter().any(|x| x == role) {
            r.push(role.to_string());
        }
    }

    pub fn register_service(&self, name: &str, f: impl Fn(&str, Value) -> Result<Value, String> + Send + Sync + 'static) {
        self.services.write().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), Arc::new(f));
    }

    fn advertised(&self) -> Option<String> {
        self.cfg.advertise.clone().or_else(|| self.local_addr().map(|a| a.to_string()))
    }

    pub fn self_info(&self) -> PeerInfo {
        PeerInfo { id: self.cfg.id, name: self.cfg.name.clone(), addr: self.advertised(), roles: self.roles(), last_seen: unix_now() }
    }

    pub fn peers(&self) -> Vec<PeerInfo> {
        let mut v: Vec<PeerInfo> = lock(&self.peers).values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }

    /// Reachable peers advertising `role`.
    pub fn peers_with_role(&self, role: &str) -> Vec<PeerInfo> {
        self.peers().into_iter().filter(|p| p.addr.is_some() && p.roles.iter().any(|r| r == role)).collect()
    }

    /// Wait until a reachable peer with `role` is known.
    pub fn wait_for_role(&self, role: &str, timeout: Duration, shutdown: &Shutdown) -> Option<PeerInfo> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(p) = self.peers_with_role(role).into_iter().next() {
                return Some(p);
            }
            if Instant::now() >= until || shutdown.sleep(Duration::from_millis(50)) {
                return None;
            }
        }
    }

    fn token_ok(&self, msg: &Value) -> bool {
        match &self.cfg.token {
            None => true,
            Some(t) => msg.get("token").and_then(|v| v.as_str()).is_some_and(|got| utils::password::ct_eq(got.as_bytes(), t.as_bytes())),
        }
    }

    fn envelope(&self, mut v: Value) -> Value {
        if let Some(t) = &self.cfg.token {
            v.set("token", t.as_str());
        }
        v
    }

    fn connect(addr: &str, timeout: Duration) -> io::Result<TcpStream> {
        let sock = utils::net::resolve(addr).map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
        let s = TcpStream::connect_timeout(&sock, timeout)?;
        s.set_nodelay(true)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        Ok(s)
    }

    /// Call `service.method(payload)` on the node at `addr`.
    pub fn call(&self, addr: &str, service: &str, method: &str, payload: Value) -> Result<Value, String> {
        self.call_timeout(addr, service, method, payload, Duration::from_secs(30))
    }

    pub fn call_timeout(&self, addr: &str, service: &str, method: &str, payload: Value, timeout: Duration) -> Result<Value, String> {
        let mut s = Self::connect(addr, timeout).map_err(|e| format!("{addr}: {e}"))?;
        let req = self.envelope(json_obj! {"t" => "rpc", "service" => service, "method" => method, "payload" => payload});
        write_msg(&mut s, &req).map_err(|e| format!("{addr}: {e}"))?;
        let resp = read_msg(&mut s).map_err(|e| format!("{addr}: {e}"))?;
        match resp.get("t").and_then(|t| t.as_str()) {
            Some("rpc_ok") => Ok(resp.get("payload").cloned().unwrap_or(Value::Null)),
            Some("rpc_err") => Err(resp.get("error").and_then(|e| e.as_str()).unwrap_or("error").to_string()),
            _ => Err("unexpected reply".into()),
        }
    }

    /// Call a service on the first reachable peer with `role` that answers.
    pub fn call_role(&self, role: &str, service: &str, method: &str, payload: Value) -> Result<Value, String> {
        let peers = self.peers_with_role(role);
        if peers.is_empty() {
            return Err(format!("no peer with role `{role}`"));
        }
        let mut last = String::new();
        for p in peers {
            match self.call(p.addr.as_deref().unwrap(), service, method, payload.clone()) {
                Ok(v) => return Ok(v),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn register_peer(&self, mut info: PeerInfo, seen_ip: Option<std::net::IpAddr>, direct: bool) {
        if info.id == self.cfg.id {
            return;
        }
        // an advertised unspecified ip means "the address you reached me from"
        if let (Some(a), Some(ip)) = (&info.addr, seen_ip) {
            if let Some((host, port)) = a.rsplit_once(':') {
                if host == "0.0.0.0" || host == "::" || host == "[::]" || host.is_empty() {
                    info.addr = Some(SocketAddr::new(ip, port.parse().unwrap_or(0)).to_string());
                }
            }
        }
        if direct {
            info.last_seen = unix_now();
        }
        let mut peers = lock(&self.peers);
        if !peers.contains_key(&info.id) && peers.len() >= self.cfg.max_peers {
            return;
        }
        match peers.get_mut(&info.id) {
            Some(existing) => {
                let seen = existing.last_seen.max(info.last_seen);
                // fresher information about the peer itself wins; gossip only refreshes timestamps
                if direct || info.last_seen >= existing.last_seen {
                    *existing = info;
                }
                existing.last_seen = seen;
            }
            None => {
                peers.insert(info.id, info);
            }
        }
    }

    /// Introduce ourselves to `addr`, learn about its peers.
    fn exchange(&self, addr: &str) -> Result<(), String> {
        let mut s = Self::connect(addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
        let req = self.envelope(json_obj! {"t" => "hello", "from" => self.self_info().to_value()});
        write_msg(&mut s, &req).map_err(|e| e.to_string())?;
        let resp = read_msg(&mut s).map_err(|e| e.to_string())?;
        if resp.get("t").and_then(|t| t.as_str()) != Some("hello") {
            return Err(resp.get("error").and_then(|e| e.as_str()).unwrap_or("rejected").to_string());
        }
        let ip = s.peer_addr().ok().map(|a| a.ip());
        if let Some(mut from) = resp.get("from").and_then(PeerInfo::from_value) {
            if from.addr.is_none() {
                from.addr = Some(addr.to_string());
            }
            self.register_peer(from, ip, true);
        }
        if let Some(list) = resp.get("peers").and_then(|p| p.as_array()) {
            for p in list.iter().filter_map(PeerInfo::from_value) {
                self.register_peer(p, None, false);
            }
        }
        Ok(())
    }

    fn handle(self: &Arc<Self>, mut s: TcpStream) {
        let peer_ip = s.peer_addr().ok().map(|a| a.ip());
        let Ok(msg) = read_msg(&mut s) else { return };
        let reply = if !self.token_ok(&msg) {
            json_obj! {"t" => "error", "error" => "bad token"}
        } else {
            match msg.get("t").and_then(|t| t.as_str()) {
                Some("hello") => {
                    if let Some(from) = msg.get("from").and_then(PeerInfo::from_value) {
                        self.register_peer(from, peer_ip, true);
                    }
                    let peers: Vec<Value> = self.peers().iter().map(PeerInfo::to_value).collect();
                    json_obj! {"t" => "hello", "from" => self.self_info().to_value(), "peers" => peers}
                }
                Some("ping") => json_obj! {"t" => "pong"},
                Some("rpc") => {
                    let svc = msg.get("service").and_then(|v| v.as_str()).unwrap_or("");
                    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
                    let payload = msg.get("payload").cloned().unwrap_or(Value::Null);
                    let f = self.services.read().unwrap_or_else(|e| e.into_inner()).get(svc).cloned();
                    match f {
                        None => json_obj! {"t" => "rpc_err", "error" => format!("unknown service `{svc}`")},
                        Some(f) => {
                            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(method, payload)));
                            match r {
                                Ok(Ok(v)) => json_obj! {"t" => "rpc_ok", "payload" => v},
                                Ok(Err(e)) => json_obj! {"t" => "rpc_err", "error" => e},
                                Err(_) => json_obj! {"t" => "rpc_err", "error" => "service panicked"},
                            }
                        }
                    }
                }
                _ => json_obj! {"t" => "error", "error" => "unknown message"},
            }
        };
        let _ = write_msg(&mut s, &reply);
    }

    /// Start listening (if configured), join the mesh and keep gossiping until `shutdown`.
    pub fn start(self: &Arc<Self>, shutdown: &Shutdown) -> io::Result<()> {
        if self.cfg.listen.is_none() && self.cfg.join.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a node needs a listening port or at least one node to join (p2p.port / p2p.join)",
            ));
        }
        if let Some(addr) = &self.cfg.listen {
            let l = TcpListener::bind(addr)?;
            l.set_nonblocking(true)?;
            *lock(&self.local) = Some(l.local_addr()?);
            let me = self.clone();
            let sd = shutdown.clone();
            let active = Arc::new(AtomicUsize::new(0));
            let h = std::thread::Builder::new().name("p2p-accept".into()).spawn(move || {
                while !sd.is_triggered() {
                    match l.accept() {
                        Ok((s, _)) => {
                            if active.load(Ordering::SeqCst) >= 256 {
                                continue;
                            }
                            let _ = s.set_nonblocking(false);
                            let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
                            let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
                            let me = me.clone();
                            let active = active.clone();
                            active.fetch_add(1, Ordering::SeqCst);
                            let a2 = active.clone();
                            if std::thread::Builder::new()
                                .name("p2p-conn".into())
                                .spawn(move || {
                                    me.handle(s);
                                    active.fetch_sub(1, Ordering::SeqCst);
                                })
                                .is_err()
                            {
                                a2.fetch_sub(1, Ordering::SeqCst);
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            sd.sleep(Duration::from_millis(10));
                        }
                        Err(_) => {
                            sd.sleep(Duration::from_millis(50));
                        }
                    }
                }
            })?;
            lock(&self.threads).push(h);
        }

        // first contact is attempted synchronously so callers can rely on it when it works
        for addr in &self.cfg.join {
            let _ = self.exchange(addr);
        }

        let me = self.clone();
        let sd = shutdown.clone();
        let h = std::thread::Builder::new().name("p2p-gossip".into()).spawn(move || {
            while !sd.sleep(me.cfg.gossip_interval) {
                let mut targets: Vec<String> = me.cfg.join.clone();
                for p in me.peers() {
                    if let Some(a) = p.addr {
                        if !targets.contains(&a) {
                            targets.push(a);
                        }
                    }
                }
                for t in targets {
                    if sd.is_triggered() {
                        return;
                    }
                    let _ = me.exchange(&t);
                }
                // evict peers we have not heard of (directly or by gossip) for too long
                let cutoff = unix_now().saturating_sub(me.cfg.peer_timeout.as_secs().max(1));
                lock(&me.peers).retain(|_, p| p.last_seen >= cutoff);
            }
        })?;
        lock(&self.threads).push(h);
        Ok(())
    }

    /// Wait for the node's threads (after the shutdown signal).
    pub fn join(&self) {
        let hs: Vec<_> = lock(&self.threads).drain(..).collect();
        for h in hs {
            let _ = h.join();
        }
    }
}
