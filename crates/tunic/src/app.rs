//! The [`App`] builder and the running app.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::thread::JoinHandle;

use database::{Backend, Database, DbServer, Record, RemoteBackend, RoutedBackend, Table, Tokens};
use p2p::{Node, NodeConfig};
use router::{Acceptor, Limits, Listener, Policy, Protocol, Router, Server};
use user::Owned;
use utils::{NetworkRule, Shutdown, Value};

use crate::builtin;
use crate::config::{Config, Kind, Opt};
use crate::feature::{Context, Feature, FnFeature};

fn list_opt(key: &str, default: &str, help: &str) -> Opt {
    Opt::new(key, Kind::List, Value::Array(vec![Value::Str(default.into())]), help)
}

/// Settings that belong to tunic itself rather than to a feature.
pub fn core_options() -> Vec<Opt> {
    vec![
        Opt::str("data.dir", Some("data"), "directory for the filesystem database and stored files"),
        // router
        Opt::bool("router.enabled", true, "serve http/gopher/gemini"),
        Opt::str("router.bind", Some("0.0.0.0"), "address the router listens on"),
        Opt::port("router.protocols.http", Value::Bool(true), "serve http: true (port 8080), false, or a port number"),
        Opt::port("router.protocols.https", Value::Bool(false), "serve http over TLS (default port 8443; needs a TLS acceptor, library use only)"),
        Opt::port("router.protocols.gopher", Value::Bool(false), "serve gopher (default port 7070)"),
        Opt::port("router.protocols.gemini", Value::Bool(false), "serve gemini (default port 1965; needs a TLS acceptor, library use only)"),
        Opt::bool("router.tags.all", true, "serve every route tag (public, admin, internal)"),
        Opt::bool("router.tags.public", true, "serve `public` routes when tags.all is false"),
        Opt::bool("router.tags.admin", false, "serve `admin` routes when tags.all is false"),
        Opt::bool("router.tags.internal", false, "serve `internal` routes when tags.all is false"),
        list_opt("router.access.public", "0.0.0.0", "who may use `public` routes (ip[/prefix][:port], comma separated)"),
        list_opt("router.access.admin", "127.0.0.0/8", "who may use `admin` routes"),
        list_opt("router.access.internal", "127.0.0.0/8", "who may use `internal` routes"),
        Opt::map("router.reverse-proxy", "forward a host name to an upstream: example.com=localhost:2455"),
        Opt::int("router.max-body-mb", Some(64), "largest accepted request body in MiB"),
        Opt::int("router.max-connections", Some(1024), "concurrent connections per listener"),
        // database
        Opt::bool("database.enabled", false, "serve this node's database to other nodes over TCP"),
        Opt::str("database.bind", Some("127.0.0.1"), "address the database server listens on"),
        Opt::int("database.port", Some(54000), "port of the database server"),
        Opt::str("database.mode", Some("fs"), "where this node keeps data: memory, fs or remote"),
        Opt::str("database.path", None, "directory of the fs database (default: <data.dir>/db)"),
        Opt::str("database.connect", None, "address of the database server to use when mode = remote"),
        Opt::str("database.token", None, "token: required from clients (server) / presented to the server (client)"),
        Opt::map("database.tokens", "extra server tokens limited to a scope: token=scope"),
        Opt::map("database.route", "send a scope prefix to another database server: scope=[token@]host:port"),
        // p2p
        Opt::new("p2p.enabled", Kind::Bool, Value::Null, "join the mesh (default: on when p2p.port, p2p.join or an hpc feature is set)"),
        Opt::int("p2p.port", None, "optional port other nodes can reach this node on"),
        Opt::str("p2p.bind", Some("0.0.0.0"), "address the mesh port listens on"),
        Opt::str("p2p.advertise", None, "address peers should use to reach this node"),
        Opt::list("p2p.join", "nodes to contact on start (any one node of the mesh is enough)"),
        Opt::str("p2p.token", None, "shared secret every node of the mesh must present"),
        Opt::str("p2p.name", None, "name shown to other nodes"),
    ]
}

/// A tunic application: built-in features plus your own.
///
/// ```no_run
/// let app = tunic::App::new().disable("food").default_value("router.protocols.http", 9000.into());
/// let running = app.start().unwrap();
/// running.wait();
/// ```
pub struct App {
    features: Vec<Box<dyn Feature>>,
    custom: Router,
    defaults: Vec<(String, Value)>,
    database: Option<Database>,
    acceptors: HashMap<String, Arc<dyn Acceptor>>,
    tasks: Option<hpc::TaskRegistry>,
    extra_options: Vec<Opt>,
}

impl App {
    /// All built-in features (see `tunic --list-features`).
    pub fn new() -> App {
        let mut a = App::empty();
        a.features = builtin::all();
        a
    }

    /// No features: only the router, database and p2p node. Add your own with [`App::feature`].
    pub fn empty() -> App {
        App { features: vec![], custom: Router::new(), defaults: vec![], database: None, acceptors: HashMap::new(), tasks: None, extra_options: vec![] }
    }

    /// Add (or replace, when the name is the same) a feature.
    pub fn feature(mut self, f: impl Feature + 'static) -> App {
        self.features.retain(|x| x.name() != f.name());
        self.features.push(Box::new(f));
        self
    }

    /// Add a feature written as a closure, enabled by default.
    pub fn on_start(self, name: &str, f: impl Fn(&mut Context) -> Result<(), String> + Send + Sync + 'static) -> App {
        self.feature(FnFeature { name: name.to_string(), enabled: true, f })
    }

    /// Remove a built-in feature entirely (it cannot be re-enabled by config).
    pub fn remove(mut self, name: &str) -> App {
        self.features.retain(|f| f.name() != name);
        self
    }

    /// Change whether a feature is on by default (config, env and cli can still override it).
    pub fn enable(self, name: &str) -> App {
        self.default_value(&format!("{name}.enabled"), Value::Bool(true))
    }
    pub fn disable(self, name: &str) -> App {
        self.default_value(&format!("{name}.enabled"), Value::Bool(false))
    }

    /// Change the default of any setting.
    pub fn default_value(mut self, key: &str, v: Value) -> App {
        self.defaults.push((key.to_string(), v));
        self
    }

    /// Declare a setting for your own features.
    pub fn option(mut self, o: Opt) -> App {
        self.extra_options.push(o);
        self
    }

    /// Routes of your own, served before the built-in ones (so they can override them).
    pub fn router(mut self, r: Router) -> App {
        let cur = std::mem::replace(&mut self.custom, Router::new());
        self.custom = cur.merge(r);
        self
    }

    /// Use this database instead of one built from the `database.*` settings.
    pub fn database(mut self, db: Database) -> App {
        self.database = Some(db);
        self
    }

    /// Database scope that holds accounts and sessions.
    pub fn users(self, scope: &str) -> App {
        self.default_value("user.scope", Value::Str(scope.into()))
    }

    /// Database scope for file metadata (file contents go to `drive.files-dir`).
    pub fn files(self, scope: &str) -> App {
        self.default_value("drive.scope", Value::Str(scope.into()))
    }

    /// Per-user records of your own type: `T`'s table lives in `scope` and is served as json CRUD
    /// under `path` (`/list`, `/create`, `/read`, `/update`, `/delete`). Needs the `user` feature;
    /// every user only sees records they own.
    pub fn table<T: Record + Owned + 'static>(self, scope: &str, path: &str) -> App {
        let (scope, path) = (scope.to_string(), path.to_string());
        self.feature(FnFeature {
            name: format!("table{}", path.replace('/', ".")),
            enabled: true,
            f: move |ctx: &mut Context| {
                let users = ctx.get::<user::Users>().ok_or("App::table needs the `user` feature")?;
                let table = Table::<T>::new(ctx.scope(&scope)?).map_err(|e| e.to_string())?;
                ctx.mount_at(&path, user::owned_crud(users, table).tagged("public"));
                Ok(())
            },
        })
    }

    /// TLS for `"https"` or `"gemini"`. Tunic ships no TLS implementation of its own.
    pub fn acceptor(mut self, which: &str, a: Arc<dyn Acceptor>) -> App {
        self.acceptors.insert(which.to_string(), a);
        self
    }

    /// Job kinds HPC workers of this app can run (default: the built-ins).
    pub fn tasks(mut self, t: hpc::TaskRegistry) -> App {
        self.tasks = Some(t);
        self
    }

    pub fn feature_names(&self) -> Vec<(String, String, bool)> {
        self.features.iter().map(|f| (f.name().to_string(), f.description().to_string(), f.default_enabled())).collect()
    }

    /// An empty config with every setting declared and this app's defaults applied.
    pub fn config(&self) -> Result<Config, String> {
        let mut c = Config::new(core_options());
        for f in &self.features {
            c.declare([Opt::bool(&format!("{}.enabled", f.name()), f.default_enabled(), &format!("run the {} feature: {}", f.name(), f.description()))]);
            c.declare(f.options());
        }
        c.declare(self.extra_options.clone());
        for (k, v) in &self.defaults {
            c.set_default(k, v.clone())?;
        }
        Ok(c)
    }

    /// Start with defaults only (no env, file or command line): the library entry point.
    pub fn start(self) -> Result<Running, String> {
        let c = self.config()?;
        self.start_with(c)
    }

    pub fn start_with(self, config: Config) -> Result<Running, String> {
        let shutdown = Shutdown::new();
        let r = self.boot(config, &shutdown);
        if r.is_err() {
            shutdown.trigger();
        }
        r
    }

    fn boot(mut self, config: Config, shutdown: &Shutdown) -> Result<Running, String> {
        // ---- which features run
        let enabled: Vec<usize> = (0..self.features.len()).filter(|i| config.bool(&format!("{}.enabled", self.features[*i].name()))).collect();
        let names: Vec<String> = enabled.iter().map(|i| self.features[*i].name().to_string()).collect();
        for (pos, i) in enabled.iter().enumerate() {
            for dep in self.features[*i].requires() {
                match names.iter().position(|n| n == dep) {
                    Some(p) if p < pos => {}
                    Some(_) => return Err(format!("feature `{}` must be registered after `{dep}`", names[pos])),
                    None => return Err(format!("feature `{}` needs feature `{dep}`: enable it (--enable {dep}) or disable `{}`", names[pos], names[pos])),
                }
            }
        }

        // ---- database
        let db = match self.database.take() {
            Some(db) => db,
            None => build_database(&config)?,
        };

        // ---- p2p node
        let wants_node = enabled.iter().any(|i| self.features[*i].needs_node());
        let p2p_on = if config.get("p2p.enabled").is_some() { config.bool("p2p.enabled") } else { config.int("p2p.port").is_some() || !config.list("p2p.join").is_empty() || wants_node };
        let router_on = config.bool("router.enabled");
        let db_serves = config.bool("database.enabled");
        let node = if p2p_on {
            let port = config.int("p2p.port");
            let join = config.list("p2p.join");
            if port.is_none() && join.is_empty() {
                return Err("p2p is on but has nothing to do: set p2p.port (be reachable) and/or p2p.join (contact an existing node)".into());
            }
            let mut roles = vec![];
            if router_on {
                roles.push("router".to_string());
            }
            if db_serves {
                roles.push("database".to_string());
            }
            let nc = NodeConfig {
                name: config.str("p2p.name").unwrap_or_default(),
                listen: port.map(|p| format!("{}:{}", config.str("p2p.bind").unwrap_or_else(|| "0.0.0.0".into()), p)),
                advertise: config.str("p2p.advertise"),
                join,
                roles,
                token: config.str("p2p.token"),
                ..NodeConfig::default()
            };
            Some(Node::new(nc))
        } else {
            None
        };

        // ---- features
        let mut ctx = Context::new(config.clone(), db.clone(), shutdown.clone(), node.clone());
        ctx.enabled = names.clone();
        if let Some(t) = self.tasks.take() {
            ctx.provide(t);
        }
        for i in &enabled {
            let f = &self.features[*i];
            f.start(&mut ctx).map_err(|e| format!("feature `{}`: {e}", f.name()))?;
        }

        // ---- database server
        let mut addrs: BTreeMap<String, SocketAddr> = BTreeMap::new();
        let mut db_server = None;
        if db_serves {
            let bind = config.str("database.bind").unwrap_or_else(|| "127.0.0.1".into());
            let token = config.str("database.token");
            let mut tokens = Tokens::new();
            for (t, scope) in config.map("database.tokens") {
                tokens = tokens.scoped(&t, &scope);
            }
            tokens = match &token {
                Some(t) => tokens.admin(t),
                None if is_loopback(&bind) && config.map("database.tokens").is_empty() => Tokens::open(),
                None if is_loopback(&bind) => tokens,
                None => return Err(format!("database.bind = {bind} is reachable from other machines: set database.token (or bind to 127.0.0.1)")),
            };
            let addr = format!("{bind}:{}", config.int("database.port").unwrap_or(54000));
            let s = DbServer::start(db.backend(), &addr, tokens, shutdown).map_err(|e| format!("database server on {addr}: {e}"))?;
            addrs.insert("database".into(), s.addr);
            db_server = Some(s);
        }

        // ---- mesh
        if let Some(n) = &node {
            if router_on {
                ctx.mount(p2p::dashboard(n.clone()).tagged("admin"));
                ctx.link("Mesh", "/p2p");
            }
            n.start(shutdown).map_err(|e| format!("p2p: {e}"))?;
            if let Some(a) = n.local_addr() {
                addrs.insert("p2p".into(), a);
            }
        }

        // ---- router
        let mut listeners = vec![];
        if router_on {
            let links = ctx.links.clone();
            let node_for_home = node.clone();
            let home_names = names.clone();
            let home = move |req: &router::Request| {
                use element::{Page, heading, link, list, text};
                let mut p = Page::new("Tunic").add(heading(1, "Tunic"));
                p = p.add(text(format!("Running: {}", if home_names.is_empty() { "(no features)".to_string() } else { home_names.join(", ") })));
                if let Some(n) = &node_for_home {
                    p = p.add(text(format!("Mesh: {} peer(s)", n.peers().len())));
                }
                if !links.is_empty() {
                    p = p.add(list(links.iter().map(|(t, path)| link(path.clone(), t.clone()))));
                }
                router::Response::page(req, &p)
            };
            let mut routes = std::mem::replace(&mut self.custom, Router::new());
            routes = routes.merge(std::mem::replace(&mut ctx.routes, Router::new()));
            routes = routes.merge(Router::new().get("/", home).tagged("public"));
            for (host, upstream) in config.map("router.reverse-proxy") {
                routes = routes.reverse_proxy(&host, &upstream);
            }
            routes = routes.with_policy(policy_from(&config)?);

            let limits = Limits {
                max_body_bytes: (config.int("router.max-body-mb").unwrap_or(64).max(1) as usize) * 1024 * 1024,
                max_connections: config.int("router.max-connections").unwrap_or(1024).max(1) as usize,
                ..Limits::default()
            };
            let bind = config.str("router.bind").unwrap_or_else(|| "0.0.0.0".into());
            for (name, proto, tls, default_port) in [("http", Protocol::Http, false, 8080), ("https", Protocol::Http, true, 8443), ("gopher", Protocol::Gopher, false, 7070), ("gemini", Protocol::Gemini, true, 1965)] {
                let Some(port) = config.port(&format!("router.protocols.{name}"), default_port) else { continue };
                let mut server = Server::new(routes.clone()).limits(limits.clone());
                if tls || name == "https" {
                    let acc = self.acceptors.get(name).cloned().ok_or_else(|| format!("router.protocols.{name} needs TLS, and tunic has no TLS built in: use the library and pass `App::acceptor(\"{name}\", ...)`, or terminate TLS in a reverse proxy and serve plain http"))?;
                    server = server.acceptor(proto, acc);
                }
                let addr = format!("{bind}:{port}");
                let l = server.listen(proto, &addr, shutdown).map_err(|e| format!("{name} on {addr}: {e}"))?;
                addrs.insert(name.to_string(), l.addr);
                listeners.push(l);
            }
        }

        let services = ctx.take_services();
        if let Some(a) = services.get(&TypeId::of::<builtin::RelayAddr>()).and_then(|b| b.downcast_ref::<builtin::RelayAddr>()) {
            addrs.insert("messages-relay".into(), a.0);
        }
        Ok(Running { shutdown: shutdown.clone(), addrs, listeners, db_server, node, database: db, threads: std::mem::take(&mut ctx.threads), services, features: names, config })
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

fn is_loopback(bind: &str) -> bool {
    bind == "localhost" || bind.parse::<IpAddr>().map(|i| i.is_loopback()).unwrap_or(false)
}

fn policy_from(c: &Config) -> Result<Policy, String> {
    let mut p = Policy::default();
    p.all_tags = c.bool("router.tags.all");
    for t in ["public", "admin", "internal"] {
        if c.bool(&format!("router.tags.{t}")) {
            p.tags.insert(t.to_string());
        }
        for rule in c.list(&format!("router.access.{t}")) {
            p.allow(t, NetworkRule::parse(&rule).map_err(|e| format!("router.access.{t}: {e}"))?);
        }
    }
    Ok(p)
}

fn build_database(c: &Config) -> Result<Database, String> {
    let token = c.str("database.token");
    let remote = |addr: &str| -> Result<Arc<dyn Backend>, String> {
        let (tok, addr) = match addr.split_once('@') {
            Some((t, a)) => (Some(t.to_string()), a),
            None => (token.clone(), addr),
        };
        Ok(Arc::new(RemoteBackend::connect(addr, tok.as_deref()).map_err(|e| e.to_string())?))
    };
    let base: Arc<dyn Backend> = match c.str("database.mode").as_deref().unwrap_or("fs") {
        "memory" => Database::memory().backend(),
        "fs" => {
            let dir = c.str("database.path").unwrap_or_else(|| format!("{}/db", c.str("data.dir").unwrap_or_else(|| "data".into()).trim_end_matches('/')));
            Database::open(&dir).map_err(|e| format!("database at {dir}: {e}"))?.backend()
        }
        "remote" => remote(&c.str("database.connect").ok_or("database.mode = remote needs database.connect")?)?,
        other => return Err(format!("database.mode must be memory, fs or remote, not `{other}`")),
    };
    let routes = c.map("database.route");
    if routes.is_empty() {
        return Ok(Database::new(base));
    }
    let mut rb = RoutedBackend::new(base);
    for (scope, addr) in routes {
        rb = rb.route(&scope, remote(&addr)?);
    }
    Ok(Database::new(Arc::new(rb)))
}

/// A started app. Dropping it does not stop it: call [`Running::stop`] or [`Running::wait`].
pub struct Running {
    shutdown: Shutdown,
    addrs: BTreeMap<String, SocketAddr>,
    listeners: Vec<Listener>,
    db_server: Option<DbServer>,
    node: Option<Arc<Node>>,
    database: Database,
    threads: Vec<JoinHandle<()>>,
    services: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    features: Vec<String>,
    config: Config,
}

impl Running {
    /// Where a part is listening: `http`, `https`, `gopher`, `gemini`, `p2p`, `database`, `messages-relay`.
    pub fn addr(&self, what: &str) -> Option<SocketAddr> {
        self.addrs.get(what).copied()
    }
    pub fn addrs(&self) -> &BTreeMap<String, SocketAddr> {
        &self.addrs
    }
    pub fn http_addr(&self) -> Option<SocketAddr> {
        self.addr("http")
    }
    pub fn node(&self) -> Option<&Arc<Node>> {
        self.node.as_ref()
    }
    /// The root database; open a scope before using it.
    pub fn database(&self) -> &Database {
        &self.database
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn features(&self) -> &[String] {
        &self.features
    }
    pub fn shutdown_handle(&self) -> Shutdown {
        self.shutdown.clone()
    }
    /// A service features published (`Users`, `Drive`, `Calendar`, `Scheduler` handle, ...).
    pub fn get<T: Any + Clone>(&self) -> Option<T> {
        self.services.get(&TypeId::of::<T>()).and_then(|b| (&**b as &dyn Any).downcast_ref::<T>()).cloned()
    }

    /// Stop everything and wait for it.
    pub fn stop(self) {
        self.shutdown.trigger();
        self.finish();
    }

    /// Block until something triggers shutdown (Ctrl-C when installed), then stop.
    pub fn wait(self) {
        self.shutdown.wait();
        self.finish();
    }

    fn finish(self) {
        for l in self.listeners {
            l.join();
        }
        if let Some(s) = self.db_server {
            s.join();
        }
        if let Some(n) = &self.node {
            n.join();
        }
        for t in self.threads {
            let _ = t.join();
        }
    }
}
