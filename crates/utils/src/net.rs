//! [`NetworkRule`]: `ip[/prefix]:port[/port...]` rules used for access control and listeners.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

/// `[Ip Address/CIDR prefix]:[Port1/Port2/PortN]`
///
/// * `0.0.0.0:80/443`        all addresses, ports 80 and 443
/// * `10.100.0.0/16:8080`    10.100.0.0 – 10.100.255.255, port 8080
/// * `127.0.0.1:8000`        localhost, port 8000
///
/// Ports are optional when only matching client addresses ([`NetworkRule::parse_addr_only`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkRule {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub ports: Vec<u16>,
}

impl NetworkRule {
    pub fn new(address: Ipv4Addr, prefix: u8, ports: Vec<u16>) -> Self {
        Self { address, prefix: prefix.min(32), ports }
    }

    /// Rule matching every IPv4 address.
    pub fn any() -> Self {
        Self { address: Ipv4Addr::UNSPECIFIED, prefix: 0, ports: vec![] }
    }

    /// Parse `ip[/prefix][:port[/port...]]`. A bare `0.0.0.0` means *any address*; any other bare
    /// ip means exactly that host.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty network rule".into());
        }
        let (addr_part, port_part) = match s.rsplit_once(':') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let (ip_str, prefix) = match addr_part.split_once('/') {
            Some((ip, pre)) => (ip, Some(pre.parse::<u8>().map_err(|_| format!("bad prefix in `{s}`"))?)),
            None => (addr_part, None),
        };
        if prefix.is_some_and(|p| p > 32) {
            return Err(format!("prefix out of range in `{s}`"));
        }
        let ip: Ipv4Addr = if ip_str == "localhost" {
            Ipv4Addr::LOCALHOST
        } else {
            ip_str.parse().map_err(|_| format!("bad ip `{ip_str}` in `{s}`"))?
        };
        let prefix = prefix.unwrap_or(if ip.is_unspecified() { 0 } else { 32 });
        let mut ports = Vec::new();
        if let Some(pp) = port_part {
            for p in pp.split('/') {
                ports.push(p.trim().parse::<u16>().map_err(|_| format!("bad port `{p}` in `{s}`"))?);
            }
        }
        Ok(Self { address: ip, prefix, ports })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let v4 = match ip {
            IpAddr::V4(v) => v,
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v) => v,
                None => return v6.is_loopback() && self.contains(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            },
        };
        if self.prefix == 0 {
            return true;
        }
        let mask = u32::MAX << (32 - self.prefix as u32);
        (u32::from(v4) & mask) == (u32::from(self.address) & mask)
    }

    pub fn contains_port(&self, port: u16) -> bool {
        self.ports.is_empty() || self.ports.contains(&port)
    }

    pub fn matches(&self, addr: &SocketAddr) -> bool {
        self.contains(addr.ip()) && self.contains_port(addr.port())
    }

    /// Socket addresses to bind for this rule (one per port).
    pub fn bind_addrs(&self) -> Vec<SocketAddr> {
        self.ports.iter().map(|p| SocketAddr::new(IpAddr::V4(self.address), *p)).collect()
    }
}

/// Resolve `host:port` (or an `ip:port`) to a socket address.
pub fn resolve(addr: &str) -> Result<SocketAddr, String> {
    addr.to_socket_addrs()
        .map_err(|e| format!("cannot resolve `{addr}`: {e}"))?
        .next()
        .ok_or_else(|| format!("no address for `{addr}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_variants() {
        let r = NetworkRule::parse("0.0.0.0:80/443").unwrap();
        assert_eq!(r.ports, vec![80, 443]);
        assert!(r.contains("8.8.8.8".parse().unwrap()));
        let r = NetworkRule::parse("10.100.0.0/16:8080").unwrap();
        assert!(r.contains("10.100.200.3".parse().unwrap()));
        assert!(!r.contains("10.101.0.1".parse().unwrap()));
        assert!(r.matches(&"10.100.0.9:8080".parse().unwrap()));
        assert!(!r.matches(&"10.100.0.9:8081".parse().unwrap()));
        let r = NetworkRule::parse("127.0.0.1:8000").unwrap();
        assert!(r.contains("127.0.0.1".parse().unwrap()));
        assert!(!r.contains("127.0.0.2".parse().unwrap()));
    }

    #[test]
    fn address_only() {
        assert!(NetworkRule::parse("0.0.0.0").unwrap().contains("1.2.3.4".parse().unwrap()));
        assert!(!NetworkRule::parse("10.1.0.0/16").unwrap().contains("10.2.0.1".parse().unwrap()));
        assert!(NetworkRule::parse("1.2.3.4").unwrap().contains("1.2.3.4".parse().unwrap()));
    }

    #[test]
    fn rejects_garbage() {
        assert!(NetworkRule::parse("300.1.1.1").is_err());
        assert!(NetworkRule::parse("10.0.0.0/33").is_err());
        assert!(NetworkRule::parse("1.1.1.1:99999").is_err());
        assert!(NetworkRule::parse("").is_err());
    }

    #[test]
    fn v6_loopback() {
        assert!(NetworkRule::parse("127.0.0.1").unwrap().contains("::1".parse().unwrap()));
    }
}
