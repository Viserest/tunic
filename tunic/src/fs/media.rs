use std::sync::Arc;

use axum::{Router, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Table,
    feature::{FeatureTrait, NetworkRule},
};

#[derive(Deserialize, Serialize)]
pub struct Playlist {
    id: Uuid,
    user_index: Vec<Uuid>,
    file_index: Vec<Uuid>,
    // Timestamps
    created_at: u64,
    accessd_at: u64,
    updated_at: u64,
    deleted_at: u64,
}

#[derive(Clone, Default)]
struct State {
    playlists: Table<Uuid, Playlist>,
}
/// Requires sub-keys: http and database
///
/// ```toml
/// [media]
/// http     = "pub/media"
/// database = "all"
/// ```
#[derive(Default)]
pub struct Config {
    http: NetworkRule,
    database: NetworkRule,
}
pub enum Scope {
    All,
    Console,
    // Manage users
    ConsoleUser,
    // Manage media sources
    ConsoleSource,
    Home,
    // Which libraries user has access to
    HomeLibrary(String),
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
            "/media",
            post(async || "media post").get(async || "media get"),
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
