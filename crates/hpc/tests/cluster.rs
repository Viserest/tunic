//! A whole cluster in one process: a scheduler node with an open port, workers on passive nodes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use database::{Database, Value};
use hpc::*;
use p2p::{Node, NodeConfig};
use router::Request;
use utils::{Shutdown, json_obj};

fn node(name: &str, listen: bool, join: &[String], sd: &Shutdown) -> Arc<Node> {
    let n = Node::new(NodeConfig {
        name: name.into(),
        listen: listen.then(|| "127.0.0.1:0".to_string()),
        join: join.to_vec(),
        gossip_interval: Duration::from_millis(100),
        ..Default::default()
    });
    n.start(sd).unwrap();
    n
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

fn worker_cfg(name: &str, threads: usize) -> WorkerConfig {
    WorkerConfig { name: name.into(), threads, poll_interval: Duration::from_millis(20), heartbeat_interval: Duration::from_millis(200), ..Default::default() }
}

#[test]
fn prime_counting_across_workers() {
    let sd = Shutdown::new();
    let sched_node = node("scheduler", true, &[], &sd);
    let sched = Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig::default()).unwrap();
    sched.attach(&sched_node);
    let addr = sched_node.local_addr().unwrap().to_string();

    // three passive worker nodes (no open port), they find the scheduler through the mesh
    let mut handles = Vec::new();
    for i in 0..3 {
        let n = node(&format!("w{i}"), false, &[addr.clone()], &sd);
        handles.extend(Worker::new(n, TaskRegistry::new(), worker_cfg(&format!("worker-{i}"), 2)).start(&sd));
    }

    let batch = sched.submit_batch("primes", Reduce::Sum, split(&json_obj! {"range" => json_obj!{"start" => 0, "end" => 2_000_000, "chunks" => 24}}).unwrap()).unwrap();
    let done = wait("batch", || sched.get_batch(&batch.id).filter(|b| b.state.is_terminal()));
    assert_eq!(done.state, JobState::Done, "{:?}", done.error);
    assert_eq!(done.result, Some(Value::Int(148_933)), "pi(2,000,000) = 148933");

    // the work was actually shared
    let workers = sched.workers();
    assert!(workers.len() >= 2, "{workers:?}");
    assert_eq!(workers.iter().map(|w| w.completed).sum::<u64>(), 24);
    assert_eq!(sched.stats(), Stats { queued: 0, running: 0, done: 24, failed: 0 });

    sd.trigger();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn custom_task_failures_and_pi() {
    let sd = Shutdown::new();
    let sched_node = node("scheduler", true, &[], &sd);
    let sched = Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig { max_attempts: 2, ..Default::default() }).unwrap();
    sched.attach(&sched_node);
    let addr = sched_node.local_addr().unwrap().to_string();

    let mut tasks = TaskRegistry::new();
    tasks.register("square", |p| {
        let n = p.as_i64().ok_or("not a number")?;
        Ok(Value::Int(n * n))
    });
    let n = node("w", false, &[addr.clone()], &sd);
    let handles = Worker::new(n, tasks, WorkerConfig { scheduler: Some(addr), ..worker_cfg("w", 2) }).start(&sd);

    let ok = sched.submit("square", Value::Int(12)).unwrap();
    let bad = sched.submit("square", Value::from("x")).unwrap();
    let unknown = sched.submit("no_such_task", Value::Null).unwrap();
    let j = wait("ok", || sched.get_job(&ok.id).filter(|j| j.state.is_terminal()));
    assert_eq!(j.result, Some(Value::Int(144)));
    let j = wait("bad", || sched.get_job(&bad.id).filter(|j| j.state.is_terminal()));
    assert_eq!((j.state, j.attempts), (JobState::Failed, 2));
    assert_eq!(j.error.as_deref(), Some("not a number"));
    // the worker only claims kinds it can run, so an unknown kind just waits in the queue
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(sched.get_job(&unknown.id).unwrap().state, JobState::Queued);

    let pi = sched.submit_batch("pi", Reduce::Sum, split(&json_obj! {"samples" => 2_000_000, "chunks" => 4}).unwrap()).unwrap();
    let b = wait("pi", || sched.get_batch(&pi.id).filter(|b| b.state.is_terminal()));
    let hits = b.result.unwrap().as_f64().unwrap();
    assert!((4.0 * hits / 2_000_000.0 - std::f64::consts::PI).abs() < 0.01);
    sd.trigger();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn http_api() {
    let sd = Shutdown::new();
    let sched_node = node("scheduler", true, &[], &sd);
    let sched = Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig::default()).unwrap();
    sched.attach(&sched_node);
    let addr = sched_node.local_addr().unwrap().to_string();
    let n = node("w", false, &[addr], &sd);
    let handles = Worker::new(n, TaskRegistry::new(), worker_cfg("w", 1)).start(&sd);

    let api = router(sched.clone(), Some(Arc::new(|req: &Request| (req.header("x-admin") != Some("1")).then(router::Response::unauthorized))));
    let call = |req: Request| api.handle(req.with_header("x-admin", "1"));

    assert_eq!(api.handle(Request::get("/hpc")).status, 401);
    let r = call(Request::post("/hpc/jobs", r#"{"kind":"echo","payload":{"a":1}}"#));
    assert_eq!(r.status, 201);
    let id = r.json_body().unwrap().get("content").unwrap().as_str().unwrap().to_string();
    let j = wait("job", || {
        let v = call(Request::get(&format!("/hpc/jobs/{id}"))).json_body().unwrap();
        (v.path("content.state").unwrap().as_str() == Some("done")).then_some(v)
    });
    assert_eq!(j.path("content.result.a").unwrap().as_i64(), Some(1));

    let r = call(Request::post("/hpc/batches", r#"{"kind":"primes","reduce":"sum","range":{"start":0,"end":100000,"chunks":4}}"#));
    assert_eq!(r.status, 201);
    let bid = r.json_body().unwrap().get("content").unwrap().as_str().unwrap().to_string();
    let b = wait("batch", || {
        let v = call(Request::get(&format!("/hpc/batches/{bid}"))).json_body().unwrap();
        (v.path("content.state").unwrap().as_str() == Some("done")).then_some(v)
    });
    assert_eq!(b.path("content.result").unwrap().as_i64(), Some(9592));

    assert_eq!(call(Request::post("/hpc/jobs", r#"{"payload":1}"#)).status, 400);
    assert_eq!(call(Request::post("/hpc/batches", r#"{"kind":"x","reduce":"avg","payloads":[1]}"#)).status, 400);
    assert_eq!(call(Request::get("/hpc/jobs/not-a-uuid")).status, 404);
    let page = call(Request::get("/hpc")).body_str();
    assert!(page.contains("HPC scheduler") && page.contains("primes"));
    assert_eq!(call(Request::get("/hpc/workers")).status, 200);
    assert_eq!(call(Request::get("/hpc/stats")).json_body().unwrap().get("done").unwrap().as_i64(), Some(5));
    sd.trigger();
    for h in handles {
        h.join().unwrap();
    }
}
