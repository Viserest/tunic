# Tunic

One executable, many optional parts, all of them nodes of a distributed mesh. Use it as a binary (`apps/tunic`) configured by command line, `tunic.toml` or environment variables, or as a Rust library where you add your own routes, tables, features and database scopes.

"Feature" in this project means a switchable part of the application (drive, calendar, hpc.worker, ...). It has nothing to do with cargo features.

```
cargo build --release -p tunic-app        # target/release/tunic
tunic --help                              # every setting, its default and its env name
tunic --list-features
tunic                                     # http on :8080 with the default features, data in ./data
```

On first start an administrator is created (`user.admin-name`, default `admin`). If `user.admin-password` / `TUNIC_USER_ADMIN_PASSWORD` is not set, a password is generated and printed once.

## Features

| Feature | Default | What it does |
|---|---|---|
| `user` | on | accounts, login sessions (cookie `session_id` or Bearer), PBKDF2-HMAC-SHA256 hashes, lockout after repeated failures, admin user management |
| `drive` | on | files on disk + metadata in the database; owners, folders, sharing (commenter < viewer < editor < owner, inherited down folders), quota, media playlists; downloads are sandboxed (`nosniff`, CSP sandbox, html/svg never inline) |
| `calendar` | on | events and contacts per user, range queries, iCalendar (`.ics`) export |
| `food` | on | ingredients, meals, daily intake, workouts, daily summary |
| `intelligence` | on | non-LLM maths: evaluate, differentiate (symbolic), integrate, limits |
| `messages` | on | group chat; `POST /message/ingest/:source` (bearer token) for bridges; line-based TCP relay for bots; `Bridge` trait for outgoing delivery |
| `hpc.scheduler` | off | job/batch queue with leases, retries and reduce (sum/min/max/concat), dashboard and job API (admin only) |
| `hpc.worker` | off | pulls jobs from a scheduler over the mesh; runs on nodes **without** an open port |

Besides features every executable has three core parts:

* **router** (`router.*`): Http/1.x, Gopher and Gemini on any port, route *tags* (`public`, `admin`, `internal`) with per-tag network access rules, virtual hosts and a reverse proxy.
* **database** (`database.*`): scoped storage (memory, filesystem, or a remote database server), optionally *served* to other nodes over TCP, with scope-limited tokens.
* **p2p** (`p2p.*`): the mesh. Every node has an **optional** port. At least one node of a mesh has a port; every other node needs `--join <address of any node>`. Nodes gossip peers and roles and call each other's services.

## Configuration

Sources, lowest to highest priority: defaults → environment (`TUNIC_ROUTER_PROTOCOLS_HTTP`, `.` and `-` become `_`) → `tunic.toml` (`--config file`, `TUNIC_CONFIG`, or `./tunic.toml`) → command line. Unknown keys and wrong types are errors, not silently ignored. `tunic --print-config` shows the result (secrets hidden).

Command line: `--router.protocols.http 9000`, `--enable drive`, `--disable food`, `--join host:7000` (repeatable), `--router.reverse-proxy.example.com localhost:2455`, `--set key=value`, `--print-config`, `--list-features`.

See [`examples/tunic.example.toml`](examples/tunic.example.toml) for a commented file:

```toml
[router]
enabled = true

[router.protocols]       # true = default port, false = off, number = that port
http = 80
gopher = 70

[router.tags]
all = true

[router.access]          # tag "public" from everywhere, tag "admin" only from 10.1.0.0 - 10.1.255.255
public = "0.0.0.0"
admin = "10.1.0.0/16"

[router.reverse-proxy]
"example.com" = "localhost:2455"

[database]
enabled = true           # serve this node's data on port 54000
port = 54000
token = "change-me"      # required when binding to anything but 127.0.0.1
```

Safe defaults: `admin`/`internal` routes only answer on loopback until `router.access.admin` says otherwise; the database server binds to 127.0.0.1 and refuses to listen publicly without a token.

## Database

### Modes

`database.mode = memory | fs | remote` (default `fs`, in `<data.dir>/db`). `remote` needs `database.connect` (and `database.token`). `database.route` sends scope prefixes to other servers (`hpc = "TOKEN@server6:54000"`), so one node can use several databases at once. Because this node's features use their own database directly, a node that also serves its database does not connect back to itself.

### Schema

#### Delegated
A scope belongs to 1 database.

```
Database 1: tunic
Database 2: app1
Database 3: app2, app3
```
* Database 3 holds two scopes but those scopes aren't at anyother database.

#### Synchronized
Each scope is a duplicate at all other databases.

```
Database 1: tunic, app1, app2
Database 2: tunic, app1, app2
Database 3: tunic, app1, app2
Database 4: app3
```
* Database 4 isn't part of the synchronized group.

#### Pooled
A group of databases hold a single scope.

```
Database 1: tunic, app1
Database 2: app1/users [50%]
Database 3: app1/users [50%]
Database 3: app2
```
* Database 2 and 3 are the group. The others are delegated.

## Distributed layouts

* [`examples/multi-app-one-database.md`](examples/multi-app-one-database.md): several applications and one database server.
* [`examples/hpc-cluster.md`](examples/hpc-cluster.md): a scheduler dashboard, workers and two databases. Workers are pull-based, so they need no open port; adding another worker is `tunic --enable hpc.worker --join <any node>`.

The HPC layer was verified end to end both in-process (three passive worker nodes counting primes below 2,000,000, = 148933) and as separate release-binary processes.

## As a library

```rust
use tunic::{App, Value};
use tunic::database::{Record, Uuid};
use tunic::router::{Request, Response, Router};

#[derive(Clone, Debug, PartialEq, Record)]
struct Note { id: Uuid, owner: Uuid, text: String }
impl tunic::user::Owned for Note { fn owner(&self) -> Uuid { self.owner } }

fn main() {
    let running = App::new()                                   // all built-in features
        .disable("food")                                       // change what is enabled
        .users("my-app/accounts")                              // scope that holds accounts
        .files("my-app/files")                                 // scope that holds file metadata
        .table::<Note>("my-app/notes", "/notes")               // your own per-user table + CRUD routes
        .router(Router::new()                                  // your own routes (served before built-ins)
            .get("/ping", |_: &Request| Response::text("pong"))
            .tagged("public"))
        .on_start("hello", |ctx| {                             // or a whole feature
            let db = ctx.scope("my-app/hello")?;               // a scope is always required
            ctx.link("Hello", "/hello");
            let _ = db;
            Ok(())
        })
        .default_value("router.protocols.http", Value::Int(9000))
        .start()
        .unwrap();
    running.wait();
}
```

This example is built from `crates/tunic/examples/library.rs`.

* The root `Database` has **no** data access: `db.scope("a/b")` is required for every read and write, and remote tokens can be limited to a scope.
* Implement `tunic::Feature` for reusable features: name (= config section), options, `requires()`, `start(ctx)`. `App::empty()` starts with none.
* `App::database(db)` supplies your own database, `App::tasks(registry)` your own HPC job kinds, `App::acceptor("https" | "gemini", acceptor)` a TLS implementation.
* `Running::get::<Users>()` etc. return the services features published; `Running::addr("http")` returns bound addresses.

## Crates

```
apps/tunic       Executable: tunic::cli::main_with(App::new())
crates/utils     uuid, sha256/hmac, PBKDF2 passwords, json + toml-subset parser, network rules, shutdown
crates/element   page building blocks (section, table, code, text, link), 12-column grid, html/gemtext/text output
crates/router    Http/1.x, Gopher, Gemini; routing, tags, access policy, virtual hosts, reverse proxy
crates/database  scoped storage, #[derive(Record)], tables, memory/fs/remote/routed backends, database server
crates/proc_macros  derive(Record) (hand-written, no syn/quote)
crates/user | drive | calendar | food | intelligence | messages
crates/p2p       mesh node: optional port, join list, gossip, roles, services, dashboard
crates/hpc       scheduler, worker, built-in tasks, dashboard/API
crates/tunic     configuration, features, App builder, CLI
```

## Not built (honest list)

* **No TLS.** `https` and Gemini (which requires TLS) are implemented as protocol codecs behind an `Acceptor` trait; starting them without an acceptor is an error that says so. Terminate TLS in front (a reverse proxy) or supply an acceptor.
* **HTTP/2 and HTTP/3** are not implemented; only HTTP/1.x.
* **P2P traffic is not encrypted**; it is plain TCP with an optional shared secret (`p2p.token`). Run it on a private network or tunnel. There is no Tor node or I2P router.
* **Messages**: no native XMPP, Signal, SimpleX, Teams, WhatsApp or Snapchat. What exists: the ingest endpoint, the relay protocol and the `Bridge` trait, which are the places such bridges plug in.
* **Database protocols**: only the custom binary protocol. No SQL, FTP, rsync, LDAP.
* **Passwords** use PBKDF2-HMAC-SHA256, not argon2, because the project is built with the standard library only.
* The README's `Artist/Album/Song` generated-lookups idea is not implemented; `#[derive(Record)]` gives typed records and tables, with relations expressed as ids.
* Everything is dependency-free (std only) because the build environment had no network access, so the original async/tokio stack was not used. Servers are thread-per-connection with limits, not an async runtime.

## Tests

`cargo test --workspace` (155 tests): unit tests per crate, protocol tests over real sockets, a multi-node HPC cluster, remote and routed databases, and whole-app tests for config priority, feature switching, reverse proxy, access rules and library use.

License: GPL-2.0-only (see LICENSE).

## Example on how to design

Application
  Server 1
    Tunic Router
    App1 Router
    App2 Router
    App3 Router
    App4 Router
Database
  Server 2
    Tunic Scope
  Server 3
    App1 Scope
    App3 Scope
    App4 Scope
  Server 4
    App2 Scope
HPC
  Server 5-9
    Any Scope
  Server 10-29
    App2/FreeTier Scope
  Server 30-99
    App2/PaidTier Scope
External
  I2p
  Tor
  XMPP
  Signal
  etc
