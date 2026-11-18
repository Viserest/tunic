//!
//! P2p
//! 	Implementation: Open sockets for peer-to-peer connections
//! 	mod feature: Config, Scope, router_all, daemon_run
//! 	Crud:
//! 	  - Self Config[Get, Set, List]
//! 	Dashboard: Http
//! 	Tor: Socket
//! 	I2p: Socket
//!
//!
//!

use std::sync::Arc;

use axum::{Router, routing::get};
use tokio::{
    sync::broadcast::{Receiver, Sender},
    task::JoinHandle,
};

use crate::feature::{FeatureTrait, NetworkRule};

#[derive(Clone, Default)]
struct State;
/// Required sub-keys: relay, socks1, socks2, control
///
/// ```toml
/// [p2p.tor]
/// # Tor node relay traffic. Relay should be global, port doesn't need to be default.
/// relay   = "0.0.0.0:9001"
/// # Tor browser SOCKS proxy
/// socks1  = "127.0.0.1:9150"
/// # System SOCKS proxy
/// socks2  = "127.0.0.1:9050"
/// # Optional local control
/// control = "127.0.0.1:9051"
/// ```
struct Tor {
    relay: NetworkRule,
    socks1: NetworkRule,
    socks2: NetworkRule,
    control: NetworkRule,
}
/// Required sub-keys: router, console, http, socks, i2cp, samv3
///
/// ```toml
/// [p2p.i2p]
/// # Router should be global, port doesn't need to be default.
/// router = "0.0.0.0:12345"
/// # Router console, this is placed in tunic router.
/// console = "127.0.0.1:7657"
/// # HTTP proxy for web browsing
/// http = "127.0.0.1:4444"
/// # SOCKS interface for i2p clients
/// socks = "127.0.0.1:7656"
/// # I2CP interface for i2p clients which are java
/// i2cp = "127.0.0.1:7654"
/// # SAMv3 interface for i2p clients which are non-java
/// samv3 = "127.0.0.1:XXXX"
/// ```
struct I2p {
    router: NetworkRule,
    console: NetworkRule,
    http: NetworkRule,
    socks: NetworkRule,
    i2cp: NetworkRule,
    samv3: NetworkRule,
}
/// Optional sub-keys: tor and i2p
/// Required sub-keys (if any optional sub-key is also provided): http and database
///
/// - Http: Where to place the config page
/// - Database: Where to connect to the database
#[derive(Default)]
struct Config {
    http: NetworkRule,
    database: NetworkRule,
    tor: Option<Tor>,
    i2p: Option<I2p>,
}
enum Scope {
    All,
    Console,
}

async fn run_tor(mut shutdown_rx: Receiver<()>, state: State, config: Arc<Config>) {
    use std::time::Duration;

    use tokio::time::sleep;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            _ = sleep(Duration::from_millis(200)) => {
                tracing::trace!("running tor daemon");
            }
        }
    }
}

async fn run_i2p(mut shutdown_rx: Receiver<()>, state: State, config: Arc<Config>) {
    use std::time::Duration;

    use tokio::time::sleep;

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            _ = sleep(Duration::from_millis(200)) => {
                tracing::trace!("running i2p daemon");
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct Feature {
    state: State,
    config: Arc<Config>,
}
impl FeatureTrait for Feature {
    type State = State;
    type Config = Config;
    type Scope = Scope;
    fn router_all(self) -> Router {
        Router::new().route("/p2p_config", get(async || "p2p config"))
    }
    fn router_crud(self) -> Router {
        Router::new()
    }
    fn run(self, shutdown_tx: Sender<()>) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        if self.config.tor.is_some() {
            let shutdown_rx = shutdown_tx.subscribe();
            let state = self.state.clone();
            let config = self.config.clone();
            handles.push(tokio::spawn(async {
                run_tor(shutdown_rx, state, config).await;
            }));
        }
        if self.config.i2p.is_some() {
            let shutdown_rx = shutdown_tx.subscribe();
            let state = self.state.clone();
            let config = self.config.clone();
            handles.push(tokio::spawn(async {
                run_i2p(shutdown_rx, state, config).await;
            }));
        }
        handles
    }
}
