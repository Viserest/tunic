use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use database::Uuid;
use p2p::Node;
use utils::{Shutdown, Value, json_obj, lock};

use crate::{ROLE_SCHEDULER, ROLE_WORKER, SERVICE, TaskRegistry};

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub name: String,
    /// Jobs executed in parallel.
    pub threads: usize,
    /// Only claim these kinds (empty = everything the registry can run).
    pub kinds: Vec<String>,
    /// Pause between claims when the queue is empty.
    pub poll_interval: Duration,
    pub heartbeat_interval: Duration,
    /// Fixed scheduler address; otherwise any mesh peer with the `hpc-scheduler` role is used.
    pub scheduler: Option<String>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        WorkerConfig {
            name: String::new(),
            threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            kinds: Vec::new(),
            poll_interval: Duration::from_millis(500),
            heartbeat_interval: Duration::from_secs(10),
            scheduler: None,
        }
    }
}

/// Pulls jobs from a scheduler and runs them. Needs no open port.
pub struct Worker {
    node: Arc<Node>,
    tasks: TaskRegistry,
    cfg: WorkerConfig,
}

impl Worker {
    pub fn new(node: Arc<Node>, tasks: TaskRegistry, mut cfg: WorkerConfig) -> Worker {
        if cfg.name.is_empty() {
            cfg.name = format!("worker-{}", &node.id().to_string()[..8]);
        }
        cfg.threads = cfg.threads.clamp(1, 1024);
        Worker { node, tasks, cfg }
    }

    pub fn name(&self) -> &str {
        &self.cfg.name
    }

    fn scheduler_addr(node: &Node, cfg: &WorkerConfig) -> Option<String> {
        cfg.scheduler.clone().or_else(|| node.peers_with_role(ROLE_SCHEDULER).into_iter().find_map(|p| p.addr))
    }

    /// Start the worker threads (they stop when `shutdown` triggers).
    pub fn start(self, shutdown: &Shutdown) -> Vec<JoinHandle<()>> {
        self.node.add_role(ROLE_WORKER);
        let me = Arc::new(self);
        let active: Arc<Mutex<Vec<Uuid>>> = Arc::default();
        let mut handles = Vec::new();

        for i in 0..me.cfg.threads {
            let (me, sd, active) = (me.clone(), shutdown.clone(), active.clone());
            handles.push(std::thread::Builder::new().name(format!("hpc-worker-{i}")).spawn(move || me.run_loop(&sd, &active)).expect("spawn worker"));
        }

        // heartbeat keeps leases alive for long jobs and tells the dashboard we exist
        let (m, sd, act) = (me.clone(), shutdown.clone(), active);
        handles.push(
            std::thread::Builder::new()
                .name("hpc-heartbeat".into())
                .spawn(move || {
                    while !sd.sleep(m.cfg.heartbeat_interval) {
                        if let Some(addr) = Self::scheduler_addr(&m.node, &m.cfg) {
                            let jobs: Vec<Value> = lock(&act).iter().map(|u| Value::from(*u)).collect();
                            let _ = m.node.call_timeout(
                                &addr,
                                SERVICE,
                                "heartbeat",
                                json_obj! {"worker" => m.cfg.name.as_str(), "threads" => m.cfg.threads, "jobs" => jobs},
                                Duration::from_secs(5),
                            );
                        }
                    }
                })
                .expect("spawn heartbeat"),
        );
        handles
    }

    fn run_loop(&self, sd: &Shutdown, active: &Mutex<Vec<Uuid>>) {
        let kinds: Vec<String> = if self.cfg.kinds.is_empty() { self.tasks.kinds() } else { self.cfg.kinds.clone() };
        while !sd.is_triggered() {
            let Some(addr) = Self::scheduler_addr(&self.node, &self.cfg) else {
                sd.sleep(self.cfg.poll_interval);
                continue;
            };
            let claim = self.node.call_timeout(
                &addr,
                SERVICE,
                "claim",
                json_obj! {"worker" => self.cfg.name.as_str(), "kinds" => kinds.clone(), "threads" => self.cfg.threads},
                Duration::from_secs(10),
            );
            let job = match claim {
                Ok(v) => v.get("job").filter(|j| !j.is_null()).cloned(),
                Err(_) => None,
            };
            let Some(job) = job else {
                sd.sleep(self.cfg.poll_interval);
                continue;
            };
            let (Some(id), Some(kind)) = (job.get("id").and_then(|i| i.as_str()).and_then(Uuid::parse), job.get("kind").and_then(|k| k.as_str())) else {
                continue;
            };
            lock(active).push(id);
            let payload = job.get("payload").cloned().unwrap_or(Value::Null);
            let outcome = self.tasks.run(kind, &payload);
            lock(active).retain(|j| *j != id);
            let (method, body) = match outcome {
                Ok(r) => ("complete", json_obj! {"job" => id, "worker" => self.cfg.name.as_str(), "result" => r}),
                Err(e) => ("fail", json_obj! {"job" => id, "worker" => self.cfg.name.as_str(), "error" => e}),
            };
            // reporting must survive a brief scheduler outage
            for attempt in 0..5 {
                if self.node.call_timeout(&addr, SERVICE, method, body.clone(), Duration::from_secs(10)).is_ok() {
                    break;
                }
                if sd.sleep(Duration::from_millis(200 * (attempt + 1))) {
                    break;
                }
            }
        }
    }
}
