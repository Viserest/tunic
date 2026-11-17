use std::sync::Arc;

use axum::Router;
use tokio::{
    sync::broadcast::{self, Sender},
    task::JoinHandle,
};
use uuid::Uuid;

mod directory;
mod filemeta;
pub mod media;
mod mime;
mod permission;
pub mod storage;

pub use directory::Directory;
pub use filemeta::Filemeta;
pub use mime::{IMAGE_MIME_TYPES, MimeType};
pub use permission::{Permission, Principal};

use crate::{Table, feature::FeatureTrait};

#[derive(Clone, Default)]
struct State {
    permissions: Table<Uuid, Permission>,
    directories: Table<Uuid, Directory>,
    files: Table<Uuid, Filemeta>,
}
#[derive(Default)]
struct Config {}
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
    fn router_all(self) -> axum::Router {
        Router::new()
    }
    fn router_crud(self) -> Router {
        Router::new()
            .nest(
                "/permissions",
                self.state
                    .permissions
                    .router::<permission::CreatePayload, permission::UpdatePayload>(),
            )
            .nest(
                "/directories",
                self.state
                    .directories
                    .router::<directory::CreatePayload, directory::UpdatePayload>(),
            )
            .nest(
                "/files",
                self.state
                    .files
                    .router::<filemeta::CreatePayload, filemeta::UpdatePayload>(),
            )
    }
    fn run(self, shutdown_tx: Sender<()>) -> Vec<JoinHandle<()>> {
        Vec::new()
    }
}
