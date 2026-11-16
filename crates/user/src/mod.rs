use std::sync::Arc;

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::Router;
use serde::{Deserialize, Serialize};
use tunic_macro::TableValueCrud;
use uuid::Uuid;

mod session;
pub use session::{Session, SessionLoginPayload, SessionPasswordResetPayload};
mod router;

use crate::{
    Table,
    db::table::{TableGetKey, TableValueDeleted, TableValueUpdate},
    feature::FeatureTrait,
    user,
};

pub fn password_hash(password: impl Into<String>) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.into().as_bytes(), &salt)
        .expect("[ERROR] Hashing password")
        .to_string()
}

pub trait UserTrait {
    // These must be set here because id, username, and password can have any field name
    fn get_id(&self) -> &Uuid;
    fn get_username(&self) -> &String;
    fn set_username(&mut self, username: impl Into<String>);
    fn get_password(&self) -> &String;
    fn set_password(&mut self, password: impl Into<String>);
    // Normal user functions
    fn verify(&self, password: impl Into<String>) -> bool {
        let hash = PasswordHash::new(self.get_password()).unwrap();
        Argon2::default()
            .verify_password(password.into().as_bytes(), &hash)
            .is_ok()
    }
    fn reset_password(
        &mut self,
        old_password: impl Into<String>,
        new_password: impl Into<String>,
    ) -> bool {
        // Check if old password is valid
        if self.verify(old_password) {
            self.set_password(new_password);
            return true;
        }
        false
    }
}

#[derive(Serialize, Deserialize, TableValueCrud)]
pub struct User {
    #[tunic(id)]
    id: Uuid,
    #[tunic(create, username)]
    username: String,
    #[tunic(create, password)]
    password: String,

    permission_index: Vec<Uuid>,
    playlist_index: Vec<Uuid>,
    chatgroup_index: Vec<Uuid>,
    session_index: Vec<Uuid>,

    created_at: u64,
    #[tunic(delete)]
    deleted_at: u64,
}
impl User {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            username: username.into(),
            password: password.into(),
            permission_index: Vec::new(),
            playlist_index: Vec::new(),
            chatgroup_index: Vec::new(),
            session_index: Vec::new(),
            created_at: crate::unix_now(),
            deleted_at: 0,
        }
    }
}

#[derive(Clone, Default)]
struct State {
    users: Table<Uuid, User>,
    sessions: Table<Uuid, Session>,
}
#[derive(Default)]
struct Config {}
enum Scope {
    All,
    Console,
    Home,
    Settings,
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
        router::router_all(self.state)
    }
    fn router_crud(self) -> Router {
        router::router_crud(self.state)
    }
    fn run(
        self,
        shutdown_tx: tokio::sync::broadcast::Sender<()>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        Vec::new()
    }
}
