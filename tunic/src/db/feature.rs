use std::sync::Arc;

use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::broadcast::{Receiver, Sender},
    task::JoinHandle,
};

use crate::feature::{FeatureTrait, NetworkRule};

#[derive(Deserialize, Serialize)]
#[repr(u8)]
enum PacketType {
    Request = 0,
    Response = 1,
    Sync = 2,
}

impl TryFrom<u8> for PacketType {
    type Error = u8;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(PacketType::Request),
            1 => Ok(PacketType::Response),
            2 => Ok(PacketType::Sync),
            other => Err(other),
        }
    }
}

async fn handle_request(client: &mut TcpStream) {}

async fn run_database<S>(mut shutdown_rx: Receiver<()>, state: S) {
    tracing::info!("Database server started.");

    let listener = TcpListener::bind("127.0.0.1:34423").await.unwrap();
    loop {
        tokio::select! {
            res = listener.accept() => {
                let (mut client, _) = res.unwrap();
                let packet_type_byte = client.read_u8().await.unwrap();
                let packet_type = PacketType::try_from(packet_type_byte).unwrap();
                match packet_type {
                    PacketType::Request => {}
                    PacketType::Response => {}
                    PacketType::Sync => {}
                }
            }
            _ = shutdown_rx.recv() => break,
        }
    }
}

#[derive(Clone, Default)]
struct State {}
/// List of letter-only names which are mapped to network rules.
/// These names can be used in database sub-keys.
///
/// ```toml
/// [database]
/// # No database separation because all database sub-keys use this network rule.
/// # Database is only accessible from 10.2.0.0 to 10.2.255.255 at port 5400.
/// all = "10.2.0.0/16:5400"
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
        for (_, rule) in self.config.list.iter() {
            for port in &rule.ports {
                let shutdown_rx = shutdown_tx.subscribe();
                let state = self.state.clone();
                handles.push(tokio::spawn(async {
                    run_database::<Self::State>(shutdown_rx, state).await;
                }));
            }
        }
        handles
    }
}
