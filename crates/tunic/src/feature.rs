use std::net::Ipv4Addr;

use axum::Router;
use tokio::{sync::broadcast::Sender, task::JoinHandle};

pub trait FeatureTrait: Clone + Default {
    type State;
    type Config;
    type Scope;
    fn router_all(self) -> Router;
    fn router_crud(self) -> Router;
    fn run(self, shutdown_tx: Sender<()>) -> Vec<JoinHandle<()>>;
}

/// [Ip Address/CIDR prefix]:[Port1/Port2/PortN]
/// The Ip Address and at least 1 port are required in a network rule.
/// CIDR prefix, extra ports, and brackets are optional.
///
/// Example usage:
/// - ```0.0.0.0:80/443```
/// All Ip Addresses at Port 80 and 443
///
/// - ```10.100.0.0/16:8080```
/// All Ip Addresses from range 10.100.0.0 to 10.100.255.255 at Port 8080
///
/// - ```127.0.0.1:8000```
/// Localhost Ip Address at Port 8000
#[derive(Clone)]
pub struct NetworkRule {
    pub address: Ipv4Addr,
    prefix: u8,
    pub ports: Vec<u16>,
}

impl NetworkRule {
    pub fn new(address: impl Into<String>, port: u16) -> Self {
        Self {
            address: address
                .into()
                .parse::<Ipv4Addr>()
                .expect("Invalid ip address"),
            prefix: 32,
            ports: vec![port],
        }
    }
    pub fn ip_in_range(&self, ip: Ipv4Addr) -> bool {
        todo!("network rule fn ip_in_range");
    }
}

impl Default for NetworkRule {
    fn default() -> Self {
        Self {
            address: Ipv4Addr::new(0, 0, 0, 0),
            prefix: 32,
            ports: Vec::new(),
        }
    }
}
