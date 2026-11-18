use std::sync::Arc;

use database::*;
use utils::Shutdown;

#[derive(Clone, Debug, PartialEq, Record)]
struct Item {
    id: Uuid,
    name: String,
}

fn serve(tokens: Tokens) -> (DbServer, Arc<LocalBackend>, Shutdown) {
    let sd = Shutdown::new();
    let backend = Arc::new(LocalBackend::memory());
    let server = DbServer::start(backend.clone(), "127.0.0.1:0", tokens, &sd).unwrap();
    (server, backend, sd)
}

#[test]
fn remote_roundtrip() {
    let (server, backend, sd) = serve(Tokens::new().admin("secret"));
    let remote = RemoteBackend::connect(&server.addr.to_string(), Some("secret")).unwrap();
    let db = Database::new(Arc::new(remote));
    let t: Table<Item> = Table::new(db.scope("app").unwrap()).unwrap();
    let item = Item { id: Uuid::new_v4(), name: "x".into() };
    t.insert(&item).unwrap();
    assert_eq!(t.get(&item.id).unwrap(), Some(item.clone()));
    assert_eq!(t.count().unwrap(), 1);
    // data really lives on the server
    assert_eq!(backend.list("app", "item").unwrap().len(), 1);
    t.update(&item.id, |i| i.name = "y".into()).unwrap();
    assert_eq!(t.get(&item.id).unwrap().unwrap().name, "y");
    assert!(t.remove(&item.id).unwrap());
    assert_eq!(t.get(&item.id).unwrap(), None);
    sd.trigger();
    server.join();
}

#[test]
fn auth_and_scope_limits() {
    let (server, _b, sd) = serve(Tokens::new().admin("root").scoped("hpc-token", "hpc"));
    let addr = server.addr.to_string();
    assert!(RemoteBackend::connect(&addr, None).is_err(), "no token, not open");
    assert!(RemoteBackend::connect(&addr, Some("wrong")).is_err());

    let hpc = RemoteBackend::connect(&addr, Some("hpc-token")).unwrap();
    hpc.put("hpc", "jobs", "1", &Value::Int(1)).unwrap();
    hpc.put("hpc/child", "jobs", "1", &Value::Int(1)).unwrap();
    assert!(matches!(hpc.put("user", "u", "1", &Value::Int(1)), Err(DbError::Denied(_))));
    assert!(matches!(hpc.get("hpcx", "u", "1"), Err(DbError::Denied(_))));
    assert!(matches!(hpc.dump(""), Err(DbError::Denied(_))));
    assert_eq!(hpc.dump("hpc").unwrap().len(), 2);

    let root = RemoteBackend::connect(&addr, Some("root")).unwrap();
    root.put("user", "u", "1", &Value::Int(1)).unwrap();
    assert_eq!(root.dump("").unwrap().len(), 3);
    sd.trigger();
    server.join();
}

#[test]
fn open_server_and_reconnect() {
    let (server, _b, sd) = serve(Tokens::open());
    let remote = RemoteBackend::connect(&server.addr.to_string(), None).unwrap();
    remote.put("a", "c", "1", &Value::Int(1)).unwrap();
    // many threads share the pool
    let remote = Arc::new(remote);
    let hs: Vec<_> = (0..8)
        .map(|n| {
            let r = remote.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    r.put("a", "c", &format!("{n}-{i}"), &Value::Int(i)).unwrap();
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(remote.list("a", "c").unwrap().len(), 201);
    // invalid scope is reported, not a crash
    assert!(remote.put("../x", "c", "1", &Value::Null).is_err());
    sd.trigger();
    server.join();
    assert!(remote.get("a", "c", "1").is_err(), "server is gone");
}

#[test]
fn replicate_between_databases() {
    let (server, _b, sd) = serve(Tokens::open());
    let remote = RemoteBackend::connect(&server.addr.to_string(), None).unwrap();
    remote.put("s", "c", "1", &Value::Int(1)).unwrap();
    remote.put("s", "c", "2", &Value::Int(2)).unwrap();
    let local = LocalBackend::memory();
    assert_eq!(replicate(&remote, &local, "s").unwrap(), 2);
    assert_eq!(local.get("s", "c", "2").unwrap(), Some(Value::Int(2)));
    sd.trigger();
    server.join();
}
