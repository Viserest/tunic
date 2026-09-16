use std::sync::Arc;

use axum::Router;
use tokio::{net::TcpListener, sync::broadcast::Sender, task::JoinHandle};

use crate::feature::{FeatureTrait, NetworkRule};

#[derive(Clone, Default)]
struct State {
    router: Router,
}
/// List of letter-only names which are mapped to network rules.
/// These names can be used in http sub-keys.
///
/// ```toml
/// [http]
/// # Public http pages, like media
/// pub = "0.0.0.0:80/443"
/// # Secret http pages, like backend storage
/// sec = "10.1.0.0/16:8000"
/// ```
#[derive(Default)]
struct Config {
    list: Vec<(String, NetworkRule)>,
}
enum Scope {}

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
        Router::new()
    }
    fn router_crud(self) -> Router {
        Router::new()
    }
    fn run(self, shutdown_tx: Sender<()>) -> Vec<JoinHandle<()>> {
        let mut handles = Vec::new();
        tracing::info!("Starting tcp listeners and axum router.");
        {
            let mut shutdown_rx = shutdown_tx.subscribe();
            let rule = self.config.list.first().unwrap().1.clone();
            let listener = TcpListener::bind((rule.address, *rule.ports.first().unwrap()));
            // Add to tokio tasks
            handles.push(tokio::spawn(async {
                axum::serve(listener.await.unwrap(), self.state.router)
                    .with_graceful_shutdown(async move {
                        let _ = shutdown_rx.recv().await;
                    })
                    .await
                    .unwrap();
            }));
        }
        tracing::info!("Done setting up tcp listeners and axum router.");
        handles
    }
}
