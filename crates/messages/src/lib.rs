//!
//! Message
//! 	Implementation: Convert all message sources to base chatgroups and messages
//! 	mod feature: Config, Scope, router_all, daemon_run
//! 	Crud:
//! 	  - Self Config[Get, Set, List]
//! 		- Chatgroup[Create, Get, List, Update, Delete]   #UserTable <-> ChatgroupTable -> MessageList#
//! 				{timestamps:u64, id:Uuid, user_index:Vec<Uuid>, messages:Vec<Message>}
//!   		- UserIndex[Create, List, Delete] #Lookup users
//! 		  - Message[Create, Get, List, Update, Delete]
//! 				{timestamps:u64, id:Uuid, user_id:Uuid, thread_parent:Uuid, thread_children:Vec<Uuid>}
//! 	Dashboard: Http, Database
//! 	Xmpp: Socket
//! 	Signal: Socket
//! 	Simplex: Socket
//!
//!

use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{Router, routing::post};
use serde::{Deserialize, Serialize};
use tokio::{sync::broadcast::Receiver, time::sleep};
use uuid::Uuid;

use crate::{
    Table,
    feature::{FeatureTrait, NetworkRule},
};

pub struct ChatGroup {
    id: Uuid,
    user_index: HashSet<Uuid>,
    messages: Vec<Message>,
    // Timestamps
    created_at: u64,
    accessd_at: u64,
    updated_at: u64,
    deleted_at: u64,
}

/// Message was sent, delivered, or read by list of users
#[derive(Deserialize, Serialize)]
pub enum Status {
    Sent(Vec<Uuid>),
    Delivered(Vec<Uuid>),
    Read(Vec<Uuid>),
}

#[derive(Deserialize, Serialize)]
pub struct Message {
    id: Uuid,
    user_id: Uuid,
    thread_parent: Option<Uuid>,
    thread_children: Vec<Uuid>,
    text: String,
    status: Status,
    // Timestamps
    created_at: u64,
    accessd_at: u64,
    updated_at: u64,
    deleted_at: u64,
}

#[derive(Clone, Default)]
struct State {
    chatgroups: Table<Uuid, ChatGroup>,
}
/// Optional sub-keys: xmpp, signal, and simplex
/// Required sub-keys (if any optional sub-key is also provided): http and database
///
/// ```toml
/// [message]
/// # Creates the message page
/// http     = "pub/messages"
/// database = "all"
/// # All message types are where feature specific clients connect to
/// # XMPP clients send to this port
/// xmpp     = "0.0.0.0:32000"
/// # Signal clients send to this port
/// signal   = "0.0.0.0:32001"
/// # Simplex clients send to this port
/// simplex  = "0.0.0.0:32002"
/// ```
#[derive(Clone, Default)]
struct Config {
    http: NetworkRule,
    database: NetworkRule,
    pub xmpp: Option<NetworkRule>,
    pub signal: Option<NetworkRule>,
    pub simplex: Option<NetworkRule>,
}
enum Scope {
    All,
    Console,
    // What sources messages can come from
    Read,
    ReadXmpp,
    ReadSignal,
    ReadSimplex,
    Home,
}

async fn run_xmpp(mut shutdown_rx: Receiver<()>, state: State, config: Arc<Config>) {
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            _ = sleep(Duration::from_millis(200)) => {
                tracing::trace!("running xmpp daemon");
            }
        }
    }
}

async fn run_signal(mut shutdown_rx: Receiver<()>, state: State, config: Arc<Config>) {
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            _ = sleep(Duration::from_millis(200)) => {
                tracing::trace!("running signal daemon");
            }
        }
    }
}

async fn run_simplex(mut shutdown_rx: Receiver<()>, state: State, config: Arc<Config>) {
    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            _ = sleep(Duration::from_millis(200)) => {
                tracing::trace!("running simplex daemon");
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
    type Config = Arc<Config>;
    type Scope = Scope;
    fn router_all(self) -> axum::Router {
        Router::new().route(
            "/message",
            post(async || "message post").get(async || "message get"),
        )
    }
    fn router_crud(self) -> axum::Router {
        Router::new()
    }
    fn run(
        self,
        shutdown_tx: tokio::sync::broadcast::Sender<()>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::new();
        if self.config.xmpp.is_some() {
            let shutdown_rx = shutdown_tx.subscribe();
            let state = self.state.clone();
            let config = self.config.clone();
            handles.push(tokio::spawn(async {
                run_xmpp(shutdown_rx, state, config).await;
            }));
        }
        if self.config.signal.is_some() {
            let shutdown_rx = shutdown_tx.subscribe();
            let state = self.state.clone();
            let config = self.config.clone();
            handles.push(tokio::spawn(async {
                run_signal(shutdown_rx, state, config).await;
            }));
        }
        if self.config.simplex.is_some() {
            let shutdown_rx = shutdown_tx.subscribe();
            let state = self.state.clone();
            let config = self.config.clone();
            handles.push(tokio::spawn(async {
                run_simplex(shutdown_rx, state, config).await;
            }));
        }
        handles
    }
}
