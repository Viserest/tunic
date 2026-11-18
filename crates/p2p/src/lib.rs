//! # P2P mesh
//!
//! Every tunic executable is a *node*. A node has an optional listening port and a list of nodes
//! to join. Nodes exchange their peer lists (gossip), advertise *roles* (`database`, `router`,
//! `hpc-scheduler`, `hpc-worker`, ...) and can call named *services* on each other.
//!
//! * A node **with** a port is reachable: others can call it.
//! * A node **without** a port is passive (e.g. behind NAT): it can call others, and it shows up
//!   in their peer tables for as long as it keeps contacting them. Pull-style services (the HPC
//!   workers) work this way.
//! * At least one node of a mesh needs a port. A node with neither a port nor anything to join has
//!   nothing to do and refuses to start.
//!
//! Wire format: one request per TCP connection, `[u32 big-endian length][json]`.
//! This is plain TCP with an optional shared-secret `token`: run it on a private network or
//! behind a VPN/tunnel; it is **not** encrypted.

mod dashboard;
mod node;

pub use dashboard::dashboard;
pub use node::{Node, NodeConfig, PeerInfo, ServiceFn};
