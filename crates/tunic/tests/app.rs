//! Whole applications started from code, config files and command lines.

use std::time::{Duration, Instant};

use tunic::{App, Value, cli};

fn tmp(tag: &str) -> String {
    let d = std::env::temp_dir().join(format!("tunic-it-{tag}-{}", utils::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

/// Defaults suited for tests: memory database, free ports, cheap password hashing.
fn quiet(app: App, tag: &str) -> App {
    let has_user = app.feature_names().iter().any(|f| f.0 == "user");
    let app = app.default_value("database.mode", "memory".into()).default_value("router.protocols.http", Value::Int(0)).default_value("data.dir", tmp(tag).into());
    if has_user {
        app.default_value("user.password-iterations", Value::Int(50)).default_value("user.admin-password", "admin test password".into())
    } else {
        app
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn call(addr: std::net::SocketAddr, method: &str, target: &str, cookie: Option<&str>, body: &[u8]) -> router::ClientResponse {
    let mut h = vec![];
    if let Some(c) = cookie {
        h.push(("Cookie", c));
    }
    router::http_request(&addr.to_string(), method, target, &h, body, Duration::from_secs(10)).unwrap()
}

fn login(addr: std::net::SocketAddr, name: &str, pw: &str) -> String {
    let r = router::http_request(&addr.to_string(), "POST", "/user/login", &[("Content-Type", "application/json")], format!(r#"{{"username":"{name}","password":"{pw}"}}"#).as_bytes(), Duration::from_secs(10)).unwrap();
    assert_eq!(r.status, 200, "{}", r.body_str());
    let v = utils::json::parse(&r.body_str()).unwrap();
    format!("session_id={}", v.get("session_id").unwrap().as_str().unwrap())
}

fn wait<T>(what: &str, f: impl Fn() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(30);
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out: {what}");
}

fn json(r: &router::ClientResponse) -> Value {
    utils::json::parse(&r.body_str()).unwrap_or_else(|e| panic!("{e}: {}", r.body_str()))
}

#[test]
fn default_app_serves_every_feature_over_http() {
    let run = quiet(App::new(), "http").start().unwrap();
    let a = run.http_addr().unwrap();
    assert_eq!(call(a, "GET", "/", None, b"").status, 200);
    assert!(call(a, "GET", "/", None, b"").body_str().contains("drive"));
    let c = login(a, "admin", "admin test password");

    // drive
    let up = router::http_request(&a.to_string(), "POST", "/storage/files", &[("Cookie", &c), ("Og-Filename", "hello.txt")], b"hello world", Duration::from_secs(10)).unwrap();
    assert_eq!(up.status, 201, "{}", up.body_str());
    let id = json(&up).get("id").unwrap().as_str().unwrap().to_string();
    assert_eq!(call(a, "GET", &format!("/storage/files/{id}"), Some(&c), b"").body, b"hello world");
    assert_eq!(call(a, "GET", &format!("/storage/files/{id}"), None, b"").status, 401);

    // calendar, food, intelligence
    let ev = call(a, "POST", "/calendar/events/create", Some(&c), br#"{"title":"standup","start_at":1000,"end_at":2000,"people":[],"location":"","notes":""}"#);
    assert!(ev.status == 200 || ev.status == 201, "{}", ev.body_str());
    assert!(call(a, "GET", "/calendar/export.ics", Some(&c), b"").body_str().contains("SUMMARY:standup"));
    assert_eq!(call(a, "GET", "/food/summary?day=2026-01-01", Some(&c), b"").status, 200);
    let calc = call(a, "GET", "/intelligence/calc?expr=2%2B3*4", None, b"");
    assert_eq!(json(&calc).get("result").unwrap().as_f64(), Some(14.0));

    // messages
    let g = call(a, "POST", "/message/groups", Some(&c), br#"{"name":"general"}"#);
    assert_eq!(g.status, 201, "{}", g.body_str());

    // library access to the same data
    let users: tunic::user::Users = run.get().unwrap();
    assert_eq!(users.count(), 1);
    run.stop();
}

#[test]
fn features_switch_off_by_config_env_and_cli() {
    // env only
    let app = quiet(App::new(), "off");
    let env = |k: &str| (k == "TUNIC_DRIVE_ENABLED").then(|| "false".to_string());
    let (cfg, _) = cli::load(&app, &[], &env).unwrap();
    let run = app.start_with(cfg).unwrap();
    assert!(!run.features().contains(&"drive".to_string()));
    assert_eq!(call(run.http_addr().unwrap(), "GET", "/storage/files", None, b"").status, 404);
    run.stop();

    // cli re-enables what a toml file disabled, and disables what env left on
    let dir = tmp("toml");
    let file = format!("{dir}/tunic.toml");
    std::fs::write(&file, "[drive]\nenabled = false\n[calendar]\nenabled = false\n").unwrap();
    let app = quiet(App::new(), "off2");
    let env = |k: &str| (k == "TUNIC_FOOD_ENABLED").then(|| "true".to_string());
    let args: Vec<String> = ["--config", &file, "--enable", "drive", "--disable", "food"].iter().map(|s| s.to_string()).collect();
    let (cfg, rest) = cli::load(&app, &args, &env).unwrap();
    assert_eq!(rest, ["--config", file.as_str()]);
    let run = app.start_with(cfg).unwrap();
    let f = run.features().to_vec();
    assert!(f.contains(&"drive".into()) && !f.contains(&"calendar".into()) && !f.contains(&"food".into()), "{f:?}");
    run.stop();
}

#[test]
fn config_priority_cli_over_toml_over_env() {
    let app = App::new();
    let dir = tmp("prio");
    let file = format!("{dir}/tunic.toml");
    std::fs::write(&file, "[router.protocols]\nhttp = 2222\n[router.reverse-proxy]\n\"example.com\" = \"localhost:2455\"\n[router.access]\nadmin = \"10.1.0.0/16\"\n").unwrap();
    let env = |k: &str| match k {
        "TUNIC_ROUTER_PROTOCOLS_HTTP" => Some("1111".to_string()),
        "TUNIC_ROUTER_PROTOCOLS_GOPHER" => Some("1112".to_string()),
        _ => None,
    };
    let (c, _) = cli::load(&app, &["--config".into(), file.clone()], &env).unwrap();
    assert_eq!(c.port("router.protocols.http", 8080), Some(2222), "toml over env");
    assert_eq!(c.port("router.protocols.gopher", 7070), Some(1112), "env still fills gaps");
    assert_eq!(c.map("router.reverse-proxy")["example.com"], "localhost:2455");
    let (c, _) = cli::load(&app, &["--config".into(), file, "--router.protocols.http".into(), "3333".into()], &env).unwrap();
    assert_eq!(c.port("router.protocols.http", 8080), Some(3333), "cli over toml");
}

#[test]
fn reverse_proxy_and_access_rules() {
    let upstream = quiet(App::empty().router(router::Router::new().get("/hi", |_: &router::Request| router::Response::text("from upstream")).tagged("public")), "up").start().unwrap();
    let up_addr = upstream.http_addr().unwrap().to_string();

    let app = quiet(App::new(), "proxy").default_value("router.protocols.gopher", Value::Int(0));
    let args: Vec<String> = ["--router.reverse-proxy.example.com".to_string(), up_addr.clone(), "--router.access.admin".into(), "10.9.0.0/16".into()].to_vec();
    let (cfg, _) = cli::load(&app, &args, &no_env).unwrap();
    let run = app.start_with(cfg).unwrap();
    let a = run.http_addr().unwrap();
    let r = router::http_request(&a.to_string(), "GET", "/hi", &[("Host", "example.com")], b"", Duration::from_secs(10)).unwrap();
    assert_eq!((r.status, r.body_str().as_str()), (200, "from upstream"));
    assert_eq!(call(a, "GET", "/hi", None, b"").status, 404, "other hosts do not reach the upstream");
    // admin routes are limited to 10.9.0.0/16, we connect from 127.0.0.1
    assert_eq!(call(a, "GET", "/user/users", None, b"").status, 403);
    assert_eq!(call(a, "POST", "/user/login", None, b"").status != 403, true, "public routes stay open");
    // gopher answers too
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(run.addr("gopher").unwrap()).unwrap();
    s.write_all(b"/\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.contains("Tunic") || out.contains("tunic"), "{out}");
    run.stop();
    upstream.stop();
}

#[test]
fn tags_can_be_switched_off() {
    let app = quiet(App::new(), "tags");
    let args: Vec<String> = ["--router.tags.all", "false", "--router.access.admin", "0.0.0.0"].iter().map(|s| s.to_string()).collect();
    let (cfg, _) = cli::load(&app, &args, &no_env).unwrap();
    let run = app.start_with(cfg).unwrap();
    let a = run.http_addr().unwrap();
    assert_eq!(call(a, "GET", "/user/users", None, b"").status, 404, "admin tag is off");
    assert_ne!(call(a, "GET", "/", None, b"").status, 404);
    run.stop();
}

#[test]
fn startup_errors_are_clear() {
    let e = |r: Result<tunic::Running, String>| r.err().expect("should fail");
    assert!(e(quiet(App::new(), "e1").disable("user").start()).contains("needs feature `user`"));
    assert!(e(quiet(App::new(), "e2").default_value("p2p.enabled", true.into()).start()).contains("p2p.port"));
    assert!(e(quiet(App::new(), "e3").default_value("router.protocols.gemini", true.into()).start()).contains("TLS"));
    assert!(e(quiet(App::new(), "e4").enable("hpc.worker").start()).contains("p2p"));
    assert!(e(quiet(App::new(), "e5").default_value("database.enabled", true.into()).default_value("database.bind", "0.0.0.0".into()).start()).contains("database.token"));
    assert!(e(quiet(App::new(), "e6").default_value("database.mode", "remote".into()).start()).contains("database.connect"));
}

#[test]
fn library_users_add_tables_features_and_scopes() {
    use tunic::database::{Record, Uuid};
    #[derive(Clone, Debug, PartialEq, Record)]
    struct Note {
        id: Uuid,
        owner: Uuid,
        text: String,
    }
    impl tunic::user::Owned for Note {
        fn owner(&self) -> Uuid {
            self.owner
        }
    }
    let run = quiet(App::new(), "lib")
        .users("my-app/accounts")
        .files("my-app/files")
        .table::<Note>("my-app/notes", "/notes")
        .on_start("ping", |ctx| {
            ctx.mount(router::Router::new().get("/ping", |_: &router::Request| router::Response::text("pong")).tagged("public"));
            ctx.link("Ping", "/ping");
            Ok(())
        })
        .start()
        .unwrap();
    let a = run.http_addr().unwrap();
    assert_eq!(call(a, "GET", "/ping", None, b"").body, b"pong");
    let c = login(a, "admin", "admin test password");
    let made = call(a, "POST", "/notes/create", Some(&c), br#"{"text":"remember"}"#);
    assert!(made.status < 300, "{}", made.body_str());
    assert_eq!(call(a, "POST", "/notes/create", None, br#"{"text":"x"}"#).status, 401);
    // data landed in the scopes the program chose
    let db = run.database();
    assert_eq!(db.scope("my-app/notes").unwrap().list("note").unwrap().len(), 1);
    assert_eq!(db.scope("my-app/accounts").unwrap().list("user").unwrap().len(), 1);
    assert!(db.list("user").is_err(), "the unscoped root refuses data access");
    run.stop();
}

#[test]
fn database_server_and_remote_clients_with_scoped_tokens() {
    // node A holds the data and serves it
    let a = quiet(App::empty(), "dbA")
        .default_value("database.enabled", true.into())
        .default_value("database.port", Value::Int(0))
        .default_value("database.token", "root-token".into())
        .default_value("database.tokens", {
            let mut m = Value::object();
            m.set("team-token", "team");
            m
        })
        .default_value("router.enabled", false.into())
        .start()
        .unwrap();
    let addr = a.addr("database").unwrap().to_string();
    let team = tunic::database::Database::new(std::sync::Arc::new(tunic::database::RemoteBackend::connect(&addr, Some("team-token")).unwrap()));
    team.scope("team/notes").unwrap().put("n", "1", &Value::Str("hi".into())).unwrap();
    assert!(team.scope("other").unwrap().put("n", "1", &Value::Null).is_err(), "scoped token cannot leave its scope");
    assert!(tunic::database::RemoteBackend::connect(&addr, Some("wrong")).is_err());

    // node B is a full app that keeps all its data on node A
    let b = quiet(App::new(), "dbB").default_value("database.mode", "remote".into()).default_value("database.connect", addr.clone().into()).default_value("database.token", "root-token".into()).start().unwrap();
    let c = login(b.http_addr().unwrap(), "admin", "admin test password");
    assert_eq!(call(b.http_addr().unwrap(), "POST", "/message/groups", Some(&c), br#"{"name":"remote"}"#).status, 201);
    assert!(a.database().scope("messages").unwrap().list("chat_group").map(|l| !l.is_empty()).unwrap_or(false) || !a.database().scope("messages").unwrap().collections().unwrap().is_empty());
    b.stop();
    a.stop();
}

#[test]
fn routed_databases_split_scopes_across_servers() {
    let shared = quiet(App::empty(), "shared")
        .default_value("database.enabled", true.into())
        .default_value("database.port", Value::Int(0))
        .default_value("router.enabled", false.into())
        .start()
        .unwrap();
    let addr = shared.addr("database").unwrap().to_string();
    // accounts stay local, files metadata goes to the shared database
    let app = quiet(App::new(), "routed").default_value("database.route", {
        let mut m = Value::object();
        m.set("drive", addr.as_str());
        m
    });
    let run = app.start().unwrap();
    let a = run.http_addr().unwrap();
    let c = login(a, "admin", "admin test password");
    let up = router::http_request(&a.to_string(), "POST", "/storage/files", &[("Cookie", &c), ("Og-Filename", "x.txt")], b"data", Duration::from_secs(10)).unwrap();
    assert_eq!(up.status, 201, "{}", up.body_str());
    assert_eq!(shared.database().scope("drive").unwrap().list("file_meta").unwrap().len(), 1, "metadata is on the shared server");
    assert!(run.database().scope("user").unwrap().list("user").unwrap().len() == 1);
    run.stop();
    shared.stop();
}

#[test]
fn hpc_cluster_from_three_apps() {
    // Server 1: scheduler with an open mesh port
    let sched = quiet(App::empty().feature(tunic::builtin::UserFeature).feature(tunic::builtin::SchedulerFeature), "hpc1")
        .default_value("p2p.port", Value::Int(0))
        .default_value("p2p.bind", "127.0.0.1".into())
        .default_value("hpc.scheduler.enabled", true.into())
        .start()
        .unwrap();
    let mesh = sched.addr("p2p").unwrap().to_string();

    // Servers 2 and 3: workers on passive nodes (no port), they only know Server 1
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let app = quiet(App::empty().feature(tunic::builtin::WorkerFeature), &format!("hpcw{i}"))
                .default_value("router.enabled", false.into())
                .default_value("hpc.worker.enabled", true.into())
                .default_value("hpc.worker.threads", Value::Int(2))
                .default_value("hpc.worker.poll-ms", Value::Int(20))
                .default_value("p2p.join", Value::Array(vec![Value::Str(mesh.clone())]));
            app.start().unwrap()
        })
        .collect();

    // a user submits a batch over http (admin only)
    let a = sched.http_addr().unwrap();
    assert_eq!(call(a, "GET", "/hpc/stats", None, b"").status, 401);
    let c = login(a, "admin", "admin test password");
    let sub = call(a, "POST", "/hpc/batches", Some(&c), br#"{"kind":"primes","reduce":"sum","range":{"start":0,"end":300000,"chunks":12}}"#);
    assert!(sub.status < 300, "{}", sub.body_str());
    let id = json(&sub).get("content").unwrap().as_str().unwrap().to_string();
    let done = wait("batch result", || {
        let v = json(&call(a, "GET", &format!("/hpc/batches/{id}"), Some(&c), b"")).get("content").unwrap().clone();
        (v.get("state").and_then(|s| s.as_str()) == Some("done")).then_some(v)
    });
    assert_eq!(done.get("result").unwrap().as_i64(), Some(25997), "pi(300000) = 25997");
    let w = json(&call(a, "GET", "/hpc/workers", Some(&c), b""));
    assert_eq!(w.get("workers").unwrap().as_array().unwrap().len(), 2, "both workers took part: {w:?}");

    for w in workers {
        w.stop();
    }
    sched.stop();
}

#[test]
fn mesh_needs_a_port_or_a_node_to_join() {
    let e = quiet(App::empty(), "mesh").default_value("p2p.enabled", true.into()).start().err().unwrap();
    assert!(e.contains("p2p.port") && e.contains("p2p.join"), "{e}");
    // two reachable nodes find each other and advertise their roles
    let a = quiet(App::empty(), "meshA").default_value("p2p.port", Value::Int(0)).default_value("p2p.bind", "127.0.0.1".into()).start().unwrap();
    let b = quiet(App::empty(), "meshB").default_value("p2p.join", Value::Array(vec![Value::Str(a.addr("p2p").unwrap().to_string())])).start().unwrap();
    wait("b sees a", || b.node().unwrap().peers().iter().any(|p| p.roles.contains(&"router".to_string())).then_some(()));
    a.stop();
    b.stop();
}
