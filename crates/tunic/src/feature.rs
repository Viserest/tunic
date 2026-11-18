//! Features: switchable parts of an app.
//!
//! A feature is a name (also its config section: `drive` reads `drive.enabled`, `drive.scope`, ...),
//! a list of settings it understands, and a `start` function that gets a [`Context`] to mount
//! routes, open database scopes, talk to the p2p node and spawn background threads.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;

use database::Database;
use p2p::Node;
use router::Router;
use utils::Shutdown;

use crate::config::{Config, Opt};

pub trait Feature: Send + Sync {
    /// Config section and `--enable`/`--disable` name, e.g. `"drive"` or `"hpc.worker"`.
    fn name(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    /// Whether the feature runs when nothing says otherwise.
    fn default_enabled(&self) -> bool {
        false
    }
    /// Settings besides `<name>.enabled` (which is declared for you).
    fn options(&self) -> Vec<Opt> {
        vec![]
    }
    /// Features that must be enabled (and registered before this one).
    fn requires(&self) -> Vec<&str> {
        vec![]
    }
    /// Whether this feature needs the p2p node (turns p2p on automatically).
    fn needs_node(&self) -> bool {
        false
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String>;
}

/// Feature made from a closure; see `App::on_start`.
pub struct FnFeature<F> {
    pub name: String,
    pub enabled: bool,
    pub f: F,
}

impl<F: Fn(&mut Context) -> Result<(), String> + Send + Sync> Feature for FnFeature<F> {
    fn name(&self) -> &str {
        &self.name
    }
    fn default_enabled(&self) -> bool {
        self.enabled
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        (self.f)(ctx)
    }
}

/// Everything a feature may use while starting.
pub struct Context {
    /// Resolved settings.
    pub config: Config,
    /// Root database. Unscoped: open a scope with [`Context::scope`] before reading or writing.
    pub db: Database,
    pub shutdown: Shutdown,
    /// The mesh node, when p2p is on.
    pub node: Option<Arc<Node>>,
    pub(crate) routes: Router,
    pub(crate) links: Vec<(String, String)>,
    pub(crate) threads: Vec<JoinHandle<()>>,
    services: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    pub(crate) enabled: Vec<String>,
}

impl Context {
    pub(crate) fn new(config: Config, db: Database, shutdown: Shutdown, node: Option<Arc<Node>>) -> Context {
        Context { config, db, shutdown, node, routes: Router::new(), links: vec![], threads: vec![], services: HashMap::new(), enabled: vec![] }
    }

    /// Open a database scope (`"drive"`, `"team/chat"`). A scope is mandatory for all data access.
    pub fn scope(&self, name: &str) -> Result<Database, String> {
        self.db.scope(name).map_err(|e| e.to_string())
    }

    /// The scope a feature should use: its `<feature>.scope` setting, else `default`.
    pub fn scope_of(&self, feature: &str, default: &str) -> Result<Database, String> {
        let name = self.config.str(&format!("{feature}.scope")).unwrap_or_else(|| default.to_string());
        self.scope(&name)
    }

    /// Add routes. They are served when the router is enabled, otherwise they are ignored.
    pub fn mount(&mut self, r: Router) {
        let cur = std::mem::replace(&mut self.routes, Router::new());
        self.routes = cur.merge(r);
    }

    pub fn mount_at(&mut self, prefix: &str, r: Router) {
        let cur = std::mem::replace(&mut self.routes, Router::new());
        self.routes = cur.nest(prefix, r);
    }

    /// Show a link on the home page.
    pub fn link(&mut self, title: &str, path: &str) {
        self.links.push((title.to_string(), path.to_string()));
    }

    /// Keep a background thread; it is joined when the app stops (it must watch `shutdown`).
    pub fn spawn(&mut self, h: JoinHandle<()>) {
        self.threads.push(h);
    }

    pub fn node(&self) -> Result<Arc<Node>, String> {
        self.node.clone().ok_or_else(|| "this feature needs the p2p node: set p2p.port and/or p2p.join".to_string())
    }

    /// Share a service with features that start later (and with the running app handle).
    pub fn provide<T: Any + Send + Sync + Clone>(&mut self, v: T) {
        self.services.insert(TypeId::of::<T>(), Box::new(v));
    }

    pub fn get<T: Any + Send + Sync + Clone>(&self) -> Option<T> {
        self.services.get(&TypeId::of::<T>()).and_then(|b| b.downcast_ref::<T>()).cloned()
    }

    pub fn is_enabled(&self, feature: &str) -> bool {
        self.enabled.iter().any(|f| f == feature)
    }

    pub(crate) fn take_services(&mut self) -> HashMap<TypeId, Box<dyn Any + Send + Sync>> {
        std::mem::take(&mut self.services)
    }
}
