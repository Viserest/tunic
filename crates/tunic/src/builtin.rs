//! The built-in features.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hpc::{Scheduler, SchedulerConfig, TaskRegistry, Worker, WorkerConfig};
use messages::Messages;
use router::{Request, Response};
use user::{Users, UsersConfig};

use crate::config::Opt;
use crate::feature::{Context, Feature};

fn scope_opt(name: &str) -> Opt {
    Opt::str(&format!("{name}.scope"), Some(name), "database scope this feature stores its data in")
}

fn mb(v: Option<i64>) -> u64 {
    v.unwrap_or(0).max(0) as u64 * 1024 * 1024
}

pub fn all() -> Vec<Box<dyn Feature>> {
    vec![
        Box::new(UserFeature),
        Box::new(DriveFeature),
        Box::new(CalendarFeature),
        Box::new(FoodFeature),
        Box::new(IntelligenceFeature),
        Box::new(MessagesFeature),
        Box::new(SchedulerFeature),
        Box::new(WorkerFeature),
    ]
}

// ---------------------------------------------------------------- user

pub struct UserFeature;

impl Feature for UserFeature {
    fn name(&self) -> &str {
        "user"
    }
    fn description(&self) -> &str {
        "accounts, login sessions, user administration"
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn options(&self) -> Vec<Opt> {
        let d = UsersConfig::default();
        vec![
            scope_opt("user"),
            Opt::str("user.admin-name", Some("admin"), "name of the administrator created on first start"),
            Opt::str("user.admin-password", None, "password of that administrator (generated and printed once when unset)"),
            Opt::int("user.session-ttl", Some(d.session_ttl as i64), "login session lifetime in seconds"),
            Opt::int("user.min-password-len", Some(d.min_password_len as i64), "shortest accepted password"),
            Opt::int("user.password-iterations", Some(d.password_iterations as i64), "PBKDF2-HMAC-SHA256 iterations for new password hashes"),
        ]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let d = UsersConfig::default();
        let cfg = UsersConfig {
            session_ttl: ctx.config.int("user.session-ttl").map(|v| v.max(60) as u64).unwrap_or(d.session_ttl),
            min_password_len: ctx.config.int("user.min-password-len").map(|v| v.max(1) as usize).unwrap_or(d.min_password_len),
            password_iterations: ctx.config.int("user.password-iterations").map(|v| v.max(1) as u32).unwrap_or(d.password_iterations),
            ..d
        };
        let users = Users::with_config(ctx.scope_of("user", "user")?, cfg).map_err(|e| e.to_string())?;
        if users.count() == 0 {
            let name = ctx.config.str("user.admin-name").unwrap_or_else(|| "admin".into());
            let (pw, generated) = match ctx.config.str("user.admin-password") {
                Some(p) => (p, false),
                None => (utils::text::to_hex(&utils::random_bytes(12)), true),
            };
            users.create_user(&name, &pw, true).map_err(|e| format!("cannot create the first administrator: {e}"))?;
            if generated {
                eprintln!("tunic: created administrator `{name}` with generated password `{pw}` (change it; it is not shown again)");
            } else {
                eprintln!("tunic: created administrator `{name}`");
            }
        }
        ctx.mount_at("/user", user::router(users.clone(), "/user"));
        ctx.link("Sign in", "/user/login");
        ctx.provide(users);
        Ok(())
    }
}

fn users_of(ctx: &Context, who: &str) -> Result<Users, String> {
    ctx.get::<Users>().ok_or_else(|| format!("`{who}` needs the `user` feature"))
}

// ---------------------------------------------------------------- drive

pub struct DriveFeature;

impl Feature for DriveFeature {
    fn name(&self) -> &str {
        "drive"
    }
    fn description(&self) -> &str {
        "file storage with sharing permissions and media playlists"
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn requires(&self) -> Vec<&str> {
        vec!["user"]
    }
    fn options(&self) -> Vec<Opt> {
        vec![
            scope_opt("drive"),
            Opt::str("drive.files-dir", None, "where file contents are stored (default: <data.dir>/files)"),
            Opt::int("drive.max-file-mb", Some(64), "largest accepted upload in MiB"),
            Opt::int("drive.quota-mb", Some(0), "per-user storage quota in MiB (0 = unlimited)"),
        ]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let users = users_of(ctx, "drive")?;
        let dir = ctx.config.str("drive.files-dir").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(ctx.config.str("data.dir").unwrap_or_else(|| "data".into())).join("files"));
        let cfg = drive::DriveConfig { max_file_bytes: mb(ctx.config.int("drive.max-file-mb")).max(1), quota_bytes: mb(ctx.config.int("drive.quota-mb")) };
        let d = drive::Drive::new(ctx.scope_of("drive", "drive")?, dir, cfg).map_err(|e| e.to_string())?;
        ctx.mount(drive::router(d.clone(), users));
        ctx.link("Files", "/storage/files");
        ctx.provide(d);
        Ok(())
    }
}

// ---------------------------------------------------------------- calendar, food, intelligence

pub struct CalendarFeature;

impl Feature for CalendarFeature {
    fn name(&self) -> &str {
        "calendar"
    }
    fn description(&self) -> &str {
        "events, contacts, iCalendar export"
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn requires(&self) -> Vec<&str> {
        vec!["user"]
    }
    fn options(&self) -> Vec<Opt> {
        vec![scope_opt("calendar")]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let users = users_of(ctx, "calendar")?;
        let cal = calendar::Calendar::new(ctx.scope_of("calendar", "calendar")?).map_err(|e| e.to_string())?;
        ctx.mount(calendar::router(cal.clone(), users));
        ctx.link("Calendar", "/calendar");
        ctx.provide(cal);
        Ok(())
    }
}

pub struct FoodFeature;

impl Feature for FoodFeature {
    fn name(&self) -> &str {
        "food"
    }
    fn description(&self) -> &str {
        "ingredients, meals, daily intake and workouts"
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn requires(&self) -> Vec<&str> {
        vec!["user"]
    }
    fn options(&self) -> Vec<Opt> {
        vec![scope_opt("food")]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let users = users_of(ctx, "food")?;
        let f = food::Food::new(ctx.scope_of("food", "food")?).map_err(|e| e.to_string())?;
        ctx.mount(food::router(f.clone(), users));
        ctx.link("Food summary", "/food/summary");
        ctx.provide(f);
        Ok(())
    }
}

pub struct IntelligenceFeature;

impl Feature for IntelligenceFeature {
    fn name(&self) -> &str {
        "intelligence"
    }
    fn description(&self) -> &str {
        "calculator: evaluate, differentiate, integrate, limits"
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        ctx.mount(intelligence::router());
        ctx.link("Calculator", "/intelligence/calc?expr=2%2B2");
        Ok(())
    }
}

// ---------------------------------------------------------------- messages

pub struct MessagesFeature;

impl Feature for MessagesFeature {
    fn name(&self) -> &str {
        "messages"
    }
    fn description(&self) -> &str {
        "group chat, bridge ingest endpoint, line relay"
    }
    fn default_enabled(&self) -> bool {
        true
    }
    fn requires(&self) -> Vec<&str> {
        vec!["user"]
    }
    fn options(&self) -> Vec<Opt> {
        vec![
            scope_opt("messages"),
            Opt::str("messages.ingest-token", None, "bearer token for POST /message/ingest/:source (endpoint is off without it)"),
            Opt::int("messages.relay-port", None, "port of the line relay for bots (off without it; needs messages.relay-token)"),
            Opt::str("messages.relay-bind", Some("127.0.0.1"), "address of the line relay"),
            Opt::str("messages.relay-token", None, "token clients send with `AUTH <token>`"),
        ]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let users = users_of(ctx, "messages")?;
        let m = Messages::new(ctx.scope_of("messages", "messages")?).map_err(|e| e.to_string())?;
        ctx.mount(messages::router(m.clone(), users, ctx.config.str("messages.ingest-token")));
        ctx.link("Messages", "/message");
        if let Some(port) = ctx.config.int("messages.relay-port") {
            let token = ctx.config.str("messages.relay-token").ok_or("messages.relay-port needs messages.relay-token")?;
            let addr = format!("{}:{}", ctx.config.str("messages.relay-bind").unwrap_or_else(|| "127.0.0.1".into()), port);
            let relay = messages::relay::RelayServer::start(m.clone(), &addr, token, &ctx.shutdown).map_err(|e| format!("messages relay on {addr}: {e}"))?;
            ctx.provide(RelayAddr(relay.addr));
            ctx.spawn(std::thread::spawn(move || relay.join()));
        }
        ctx.provide(m);
        Ok(())
    }
}

/// Address the messages relay is listening on (available through `Running::get`).
#[derive(Clone, Copy, Debug)]
pub struct RelayAddr(pub std::net::SocketAddr);

// ---------------------------------------------------------------- hpc

pub struct SchedulerFeature;

impl Feature for SchedulerFeature {
    fn name(&self) -> &str {
        "hpc.scheduler"
    }
    fn description(&self) -> &str {
        "HPC job scheduler: queue, leases, retries, batches (workers join through the mesh)"
    }
    fn needs_node(&self) -> bool {
        true
    }
    fn options(&self) -> Vec<Opt> {
        let d = SchedulerConfig::default();
        vec![
            scope_opt("hpc"),
            Opt::int("hpc.scheduler.lease-secs", Some(d.lease_secs as i64), "a claimed job returns to the queue after this many silent seconds"),
            Opt::int("hpc.scheduler.max-attempts", Some(d.max_attempts as i64), "tries per job before it is marked failed"),
        ]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let node = ctx.node()?;
        let d = SchedulerConfig::default();
        let cfg = SchedulerConfig {
            lease_secs: ctx.config.int("hpc.scheduler.lease-secs").map(|v| v.max(1) as u64).unwrap_or(d.lease_secs),
            max_attempts: ctx.config.int("hpc.scheduler.max-attempts").map(|v| v.max(1) as u32).unwrap_or(d.max_attempts),
            ..d
        };
        let s = Scheduler::new(ctx.scope_of("hpc", "hpc")?, cfg).map_err(|e| e.to_string())?;
        s.attach(&node);
        let reaper = s.start_reaper(&ctx.shutdown);
        ctx.spawn(reaper);
        // with accounts available only administrators may use the dashboard and job API
        let guard: Option<Arc<dyn Fn(&Request) -> Option<Response> + Send + Sync>> = ctx.get::<Users>().map(|u| {
            Arc::new(move |req: &Request| match u.from_request(req) {
                None => Some(Response::unauthorized()),
                Some(me) if !me.is_admin => Some(Response::forbidden()),
                Some(_) => None,
            }) as Arc<dyn Fn(&Request) -> Option<Response> + Send + Sync>
        });
        ctx.mount(hpc::router(s.clone(), guard));
        ctx.link("HPC scheduler", "/hpc");
        ctx.provide(s);
        Ok(())
    }
}

pub struct WorkerFeature;

impl Feature for WorkerFeature {
    fn name(&self) -> &str {
        "hpc.worker"
    }
    fn description(&self) -> &str {
        "HPC worker: pulls jobs from a scheduler on the mesh; needs no open port"
    }
    fn needs_node(&self) -> bool {
        true
    }
    fn options(&self) -> Vec<Opt> {
        vec![
            Opt::str("hpc.worker.name", None, "worker name (default: worker-<node id>)"),
            Opt::int("hpc.worker.threads", Some(0), "parallel jobs (0 = number of CPUs)"),
            Opt::list("hpc.worker.kinds", "only run these job kinds (default: all registered tasks)"),
            Opt::str("hpc.worker.scheduler", None, "scheduler address (default: any mesh peer with the hpc-scheduler role)"),
            Opt::int("hpc.worker.poll-ms", Some(500), "pause between claims when the queue is empty"),
        ]
    }
    fn start(&self, ctx: &mut Context) -> Result<(), String> {
        let node = ctx.node()?;
        let tasks = ctx.get::<TaskRegistry>().unwrap_or_else(TaskRegistry::new);
        let d = WorkerConfig::default();
        let threads = ctx.config.int("hpc.worker.threads").filter(|t| *t > 0).map(|t| t as usize).unwrap_or(d.threads);
        let cfg = WorkerConfig {
            name: ctx.config.str("hpc.worker.name").unwrap_or_default(),
            threads,
            kinds: ctx.config.list("hpc.worker.kinds"),
            poll_interval: Duration::from_millis(ctx.config.int("hpc.worker.poll-ms").unwrap_or(500).max(10) as u64),
            scheduler: ctx.config.str("hpc.worker.scheduler"),
            ..d
        };
        let handles = Worker::new(node, tasks, cfg).start(&ctx.shutdown);
        for h in handles {
            ctx.spawn(h);
        }
        Ok(())
    }
}
