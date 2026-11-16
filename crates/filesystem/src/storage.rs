use std::sync::Arc;

use axum::{Router, routing::post};

use crate::feature::{FeatureTrait, NetworkRule};

#[derive(Clone, Default)]
struct State;
/// Requires sub-keys: http and database
///
/// ```toml
/// [storage]
/// http     = "sec/storage"
/// database = "all"
/// ```
#[derive(Default)]
struct Config {
    http: NetworkRule,
    database: NetworkRule,
}
enum Scope {
    All,
    Console,
    Home,
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
        Router::new().route(
            "/storage",
            post(async || "storage post").get(async || "storage get"),
        )
    }
    fn router_crud(self) -> Router {
        Router::new()
    }
    fn run(
        self,
        shutdown_tx: tokio::sync::broadcast::Sender<()>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        Vec::new()
    }
}
