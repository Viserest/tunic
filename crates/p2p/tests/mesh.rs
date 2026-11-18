use std::sync::Arc;
use std::time::{Duration, Instant};

use p2p::*;
use utils::{Shutdown, Value, json_obj};

fn cfg(name: &str, listen: bool, join: &[String]) -> NodeConfig {
    NodeConfig {
        name: name.into(),
        listen: listen.then(|| "127.0.0.1:0".to_string()),
        join: join.to_vec(),
        gossip_interval: Duration::from_millis(100),
        peer_timeout: Duration::from_secs(3),
        ..Default::default()
    }
}

fn eventually(what: &str, f: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for: {what}");
}

fn start(c: NodeConfig, sd: &Shutdown) -> Arc<Node> {
    let n = Node::new(c);
    n.start(sd).unwrap();
    n
}

#[test]
fn three_nodes_discover_each_other_and_call_services() {
    let sd = Shutdown::new();
    let a = start(NodeConfig { roles: vec!["database".into()], ..cfg("a", true, &[]) }, &sd);
    let a_addr = a.local_addr().unwrap().to_string();
    let b = start(cfg("b", true, &[a_addr.clone()]), &sd);
    let b_addr = b.local_addr().unwrap().to_string();
    // c only knows b
    let c = start(cfg("c", true, &[b_addr]), &sd);

    eventually("a learns about c via gossip", || a.peers().iter().any(|p| p.name == "c"));
    eventually("c learns about a via gossip", || c.peers().iter().any(|p| p.name == "a"));
    assert_eq!(c.peers_with_role("database").len(), 1);

    a.register_service("echo", |m, p| Ok(json_obj! {"method" => m, "got" => p}));
    a.register_service("fail", |_, _| Err("nope".to_string()));
    a.register_service("panic", |_, _| panic!("boom"));
    let r = c.call_role("database", "echo", "hi", Value::Int(7)).unwrap();
    assert_eq!(r.get("method").unwrap().as_str(), Some("hi"));
    assert_eq!(r.get("got").unwrap().as_i64(), Some(7));
    assert_eq!(c.call(&a_addr, "fail", "x", Value::Null).unwrap_err(), "nope");
    assert!(c.call(&a_addr, "panic", "x", Value::Null).unwrap_err().contains("panicked"));
    assert!(c.call(&a_addr, "missing", "x", Value::Null).unwrap_err().contains("unknown service"));
    assert!(c.call_role("nothing", "echo", "x", Value::Null).is_err());

    // roles added later spread
    b.add_role("hpc-worker");
    eventually("role spreads", || a.peers_with_role("hpc-worker").len() == 1);
    sd.trigger();
    for n in [a, b, c] {
        n.join();
    }
}

#[test]
fn passive_nodes_can_call_but_are_not_callable() {
    let sd = Shutdown::new();
    let seed = start(cfg("seed", true, &[]), &sd);
    let addr = seed.local_addr().unwrap().to_string();
    seed.register_service("ping", |_, _| Ok(Value::from("pong")));
    let passive = start(NodeConfig { roles: vec!["hpc-worker".into()], ..cfg("passive", false, &[addr.clone()]) }, &sd);
    eventually("seed sees passive node", || seed.peers().iter().any(|p| p.name == "passive"));
    let p = seed.peers().into_iter().find(|p| p.name == "passive").unwrap();
    assert!(p.addr.is_none());
    assert!(seed.peers_with_role("hpc-worker").is_empty(), "passive peers cannot be called");
    assert_eq!(passive.call(&addr, "ping", "", Value::Null).unwrap().as_str(), Some("pong"));
    sd.trigger();
    seed.join();
    passive.join();
}

#[test]
fn node_needs_port_or_join() {
    let n = Node::new(cfg("lonely", false, &[]));
    assert!(n.start(&Shutdown::new()).is_err());
}

#[test]
fn token_is_enforced() {
    let sd = Shutdown::new();
    let a = start(NodeConfig { token: Some("s3".into()), ..cfg("a", true, &[]) }, &sd);
    let addr = a.local_addr().unwrap().to_string();
    a.register_service("x", |_, _| Ok(Value::Null));
    let good = start(NodeConfig { token: Some("s3".into()), ..cfg("good", false, &[addr.clone()]) }, &sd);
    let bad = start(NodeConfig { token: Some("wrong".into()), ..cfg("bad", false, &[addr.clone()]) }, &sd);
    assert!(good.call(&addr, "x", "", Value::Null).is_ok());
    assert!(bad.call(&addr, "x", "", Value::Null).is_err());
    assert!(a.peers().iter().all(|p| p.name != "bad"));
    sd.trigger();
}

#[test]
fn dead_peers_are_evicted() {
    let sd_a = Shutdown::new();
    let a = start(
        NodeConfig { peer_timeout: Duration::from_secs(2), ..cfg("a", true, &[]) },
        &sd_a,
    );
    let sd_b = Shutdown::new();
    let b = start(cfg("b", false, &[a.local_addr().unwrap().to_string()]), &sd_b);
    eventually("a sees b", || a.peers().iter().any(|p| p.name == "b"));
    sd_b.trigger();
    b.join();
    eventually("b is evicted", || a.peers().is_empty());
    sd_a.trigger();
}

#[test]
fn dashboard_renders() {
    let sd = Shutdown::new();
    let a = start(cfg("a", true, &[]), &sd);
    let r = dashboard(a);
    let page = r.handle(router::Request::get("/p2p"));
    assert_eq!(page.status, 200);
    assert!(page.body_str().contains("P2P mesh"));
    assert_eq!(r.handle(router::Request::get("/p2p/peers")).status, 200);
    sd.trigger();
}
