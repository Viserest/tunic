use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use database::{Database, DbError, Table, Uuid, Value};
use p2p::Node;
use utils::{Shutdown, json_obj, lock, unix_now};

use crate::{Batch, Job, JobState, Reduce, ROLE_SCHEDULER, SERVICE};

#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    /// A claimed job returns to the queue if its worker has not reported for this long.
    pub lease_secs: u64,
    pub max_attempts: u32,
    /// Upper bound on jobs in one batch.
    pub max_batch_jobs: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        SchedulerConfig { lease_secs: 120, max_attempts: 3, max_batch_jobs: 100_000 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct WorkerInfo {
    pub name: String,
    pub threads: u64,
    pub last_seen: u64,
    pub active: usize,
    pub completed: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub queued: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
}

pub struct Scheduler {
    jobs: Table<Job>,
    batches: Table<Batch>,
    queue: Mutex<VecDeque<(Uuid, String)>>,
    workers: Mutex<HashMap<String, WorkerInfo>>,
    pub config: SchedulerConfig,
}

fn valid_kind(k: &str) -> bool {
    !k.is_empty() && k.len() <= 64 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn e(err: DbError) -> String {
    err.to_string()
}

impl Scheduler {
    /// `db` must be a scoped handle (typically `db.scope("hpc")`). Jobs already in the database
    /// are picked up again, so a scheduler restart loses nothing.
    pub fn new(db: Database, config: SchedulerConfig) -> Result<Arc<Scheduler>, DbError> {
        let jobs: Table<Job> = Table::new(db.clone())?;
        let batches: Table<Batch> = Table::new(db)?;
        let mut queued: Vec<Job> = jobs.find(|j| j.state == JobState::Queued)?;
        queued.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let queue = queued.into_iter().map(|j| (j.id, j.kind)).collect();
        Ok(Arc::new(Scheduler { jobs, batches, queue: Mutex::new(queue), workers: Default::default(), config }))
    }

    pub fn submit(&self, kind: &str, payload: Value) -> Result<Job, String> {
        self.submit_inner(kind, payload, None)
    }

    fn submit_inner(&self, kind: &str, payload: Value, batch: Option<Uuid>) -> Result<Job, String> {
        if !valid_kind(kind) {
            return Err("`kind` must be 1-64 characters of letters, digits, `_`, `-`".into());
        }
        let job = Job {
            id: Uuid::new_v4(),
            batch_id: batch,
            kind: kind.to_string(),
            payload,
            state: JobState::Queued,
            worker: None,
            attempts: 0,
            max_attempts: self.config.max_attempts,
            created_at: unix_now(),
            started_at: 0,
            finished_at: 0,
            lease_expires_at: 0,
            result: None,
            error: None,
        };
        self.jobs.insert(&job).map_err(e)?;
        lock(&self.queue).push_back((job.id, job.kind.clone()));
        Ok(job)
    }

    /// Many jobs of one kind whose results are combined with `reduce`.
    pub fn submit_batch(&self, kind: &str, reduce: Reduce, payloads: Vec<Value>) -> Result<Batch, String> {
        if payloads.is_empty() {
            return Err("a batch needs at least one job".into());
        }
        if payloads.len() > self.config.max_batch_jobs {
            return Err(format!("a batch is limited to {} jobs", self.config.max_batch_jobs));
        }
        let mut batch = Batch {
            id: Uuid::new_v4(),
            kind: kind.to_string(),
            reduce,
            job_ids: Vec::new(),
            state: JobState::Running,
            created_at: unix_now(),
            finished_at: 0,
            result: None,
            error: None,
        };
        // the batch row exists before its jobs so a fast worker can finalize it
        self.batches.insert(&batch).map_err(e)?;
        for p in payloads {
            batch.job_ids.push(self.submit_inner(kind, p, Some(batch.id))?.id);
        }
        self.batches.insert(&batch).map_err(e)?;
        Ok(batch)
    }

    /// Hand the oldest queued job (of one of `kinds`; empty = any) to `worker`.
    pub fn claim(&self, worker: &str, kinds: &[String]) -> Result<Option<Job>, String> {
        loop {
            let picked = {
                let mut q = lock(&self.queue);
                let pos = q.iter().position(|(_, k)| kinds.is_empty() || kinds.contains(k));
                pos.and_then(|i| q.remove(i))
            };
            let Some((id, _)) = picked else { return Ok(None) };
            let now = unix_now();
            let lease = self.config.lease_secs;
            let updated = self
                .jobs
                .update(&id, |j| {
                    if j.state == JobState::Queued {
                        j.state = JobState::Running;
                        j.worker = Some(worker.to_string());
                        j.attempts += 1;
                        j.started_at = now;
                        j.lease_expires_at = now + lease;
                    }
                })
                .map_err(e)?;
            match updated {
                Some(j) if j.state == JobState::Running && j.worker.as_deref() == Some(worker) && j.started_at == now => return Ok(Some(j)),
                _ => continue, // cancelled or vanished: try the next one
            }
        }
    }

    pub fn complete(&self, job_id: &Uuid, worker: &str, result: Value) -> Result<(), String> {
        let now = unix_now();
        let mut owned = false;
        let job = self
            .jobs
            .update(job_id, |j| {
                if j.state == JobState::Running && j.worker.as_deref() == Some(worker) {
                    owned = true;
                    j.state = JobState::Done;
                    j.finished_at = now;
                    j.result = Some(result.clone());
                    j.error = None;
                }
            })
            .map_err(e)?
            .ok_or("no such job")?;
        if !owned {
            return Err("job is not leased to this worker".into());
        }
        self.note(worker, |w| w.completed += 1);
        self.finish_batch_if_done(&job)
    }

    pub fn fail(&self, job_id: &Uuid, worker: &str, error: &str) -> Result<(), String> {
        let now = unix_now();
        let mut owned = false;
        let mut requeue = false;
        let job = self
            .jobs
            .update(job_id, |j| {
                if j.state == JobState::Running && j.worker.as_deref() == Some(worker) {
                    owned = true;
                    j.error = Some(error.chars().take(2000).collect());
                    if j.attempts >= j.max_attempts {
                        j.state = JobState::Failed;
                        j.finished_at = now;
                    } else {
                        j.state = JobState::Queued;
                        j.worker = None;
                        requeue = true;
                    }
                }
            })
            .map_err(e)?
            .ok_or("no such job")?;
        if !owned {
            return Err("job is not leased to this worker".into());
        }
        self.note(worker, |w| w.failed += 1);
        if requeue {
            lock(&self.queue).push_back((job.id, job.kind.clone()));
            Ok(())
        } else {
            self.finish_batch_if_done(&job)
        }
    }

    /// Worker liveness plus lease extension for the jobs it is still running.
    pub fn heartbeat(&self, worker: &str, threads: u64, active: &[Uuid]) {
        let now = unix_now();
        self.note(worker, |w| {
            w.threads = threads;
            w.active = active.len();
        });
        for id in active {
            let lease = self.config.lease_secs;
            let _ = self.jobs.update(id, |j| {
                if j.state == JobState::Running && j.worker.as_deref() == Some(worker) {
                    j.lease_expires_at = now + lease;
                }
            });
        }
    }

    fn note(&self, worker: &str, f: impl FnOnce(&mut WorkerInfo)) {
        let mut w = lock(&self.workers);
        let info = w.entry(worker.to_string()).or_insert_with(|| WorkerInfo { name: worker.to_string(), ..Default::default() });
        info.last_seen = unix_now();
        f(info);
    }

    pub fn cancel(&self, job_id: &Uuid) -> Result<bool, String> {
        let now = unix_now();
        let mut cancelled = false;
        let job = self
            .jobs
            .update(job_id, |j| {
                if j.state == JobState::Queued {
                    cancelled = true;
                    j.state = JobState::Failed;
                    j.error = Some("cancelled".into());
                    j.finished_at = now;
                }
            })
            .map_err(e)?;
        if cancelled {
            lock(&self.queue).retain(|(id, _)| id != job_id);
            if let Some(j) = job {
                self.finish_batch_if_done(&j)?;
            }
        }
        Ok(cancelled)
    }

    fn finish_batch_if_done(&self, job: &Job) -> Result<(), String> {
        let Some(bid) = job.batch_id else { return Ok(()) };
        let Some(batch) = self.batches.get(&bid).map_err(e)? else { return Ok(()) };
        if batch.state.is_terminal() || batch.job_ids.is_empty() {
            return Ok(());
        }
        let mut results = Vec::with_capacity(batch.job_ids.len());
        let mut failure: Option<String> = None;
        for id in &batch.job_ids {
            match self.jobs.get(id).map_err(e)? {
                Some(j) if j.state == JobState::Done => results.push(j.result.unwrap_or(Value::Null)),
                Some(j) if j.state == JobState::Failed => {
                    failure.get_or_insert(format!("job {} failed: {}", j.id, j.error.unwrap_or_default()));
                }
                _ => return Ok(()), // still in flight
            }
        }
        let now = unix_now();
        let (state, result, error) = match failure {
            Some(f) => (JobState::Failed, None, Some(f)),
            None => match batch.reduce.apply(&results) {
                Ok(v) => (JobState::Done, Some(v), None),
                Err(err) => (JobState::Failed, None, Some(err)),
            },
        };
        self.batches
            .update(&bid, |b| {
                if !b.state.is_terminal() {
                    b.state = state;
                    b.result = result.clone();
                    b.error = error.clone();
                    b.finished_at = now;
                }
            })
            .map_err(e)?;
        Ok(())
    }

    /// Return jobs whose lease ran out to the queue (or fail them after too many attempts).
    pub fn expire_leases(&self) -> usize {
        let now = unix_now();
        let expired = self.jobs.find(|j| j.state == JobState::Running && j.lease_expires_at < now).unwrap_or_default();
        let mut n = 0;
        for j in expired {
            let mut requeued = false;
            let updated = self.jobs.update(&j.id, |x| {
                if x.state == JobState::Running && x.lease_expires_at < now {
                    x.error = Some("worker lease expired".into());
                    if x.attempts >= x.max_attempts {
                        x.state = JobState::Failed;
                        x.finished_at = now;
                    } else {
                        x.state = JobState::Queued;
                        x.worker = None;
                        requeued = true;
                    }
                }
            });
            if let Ok(Some(x)) = updated {
                n += 1;
                if requeued {
                    lock(&self.queue).push_back((x.id, x.kind));
                } else if x.state == JobState::Failed {
                    let _ = self.finish_batch_if_done(&x);
                }
            }
        }
        n
    }

    pub fn get_job(&self, id: &Uuid) -> Option<Job> {
        self.jobs.get(id).ok().flatten()
    }
    pub fn get_batch(&self, id: &Uuid) -> Option<Batch> {
        self.batches.get(id).ok().flatten()
    }

    /// Newest first.
    pub fn list_jobs(&self, state: Option<JobState>, limit: usize) -> Vec<Job> {
        let mut v = self.jobs.find(|j| state.is_none_or(|s| j.state == s)).unwrap_or_default();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        v.truncate(limit);
        v
    }

    pub fn list_batches(&self, limit: usize) -> Vec<Batch> {
        let mut v = self.batches.all().unwrap_or_default();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v.truncate(limit);
        v
    }

    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        for j in self.jobs.all().unwrap_or_default() {
            match j.state {
                JobState::Queued => s.queued += 1,
                JobState::Running => s.running += 1,
                JobState::Done => s.done += 1,
                JobState::Failed => s.failed += 1,
            }
        }
        s
    }

    pub fn workers(&self) -> Vec<WorkerInfo> {
        let mut v: Vec<_> = lock(&self.workers).values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Serve the scheduler on the mesh: registers the `hpc` service and the `hpc-scheduler` role.
    pub fn attach(self: &Arc<Self>, node: &Arc<Node>) {
        let me = self.clone();
        node.register_service(SERVICE, move |method, p| me.rpc(method, p));
        node.add_role(ROLE_SCHEDULER);
    }

    /// Background thread requeueing jobs of dead workers.
    pub fn start_reaper(self: &Arc<Self>, shutdown: &Shutdown) -> JoinHandle<()> {
        let me = self.clone();
        let sd = shutdown.clone();
        let every = Duration::from_secs((self.config.lease_secs / 4).clamp(1, 15));
        std::thread::Builder::new()
            .name("hpc-reaper".into())
            .spawn(move || {
                while !sd.sleep(every) {
                    me.expire_leases();
                }
            })
            .expect("spawn reaper")
    }

    fn rpc(&self, method: &str, p: Value) -> Result<Value, String> {
        let s = |k: &str| p.get(k).and_then(|v| v.as_str()).map(String::from);
        let id = |k: &str| p.get(k).and_then(|v| v.as_str()).and_then(Uuid::parse);
        match method {
            "claim" => {
                let worker = s("worker").ok_or("`worker` is required")?;
                let kinds: Vec<String> = p.get("kinds").and_then(|k| k.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
                self.note(&worker, |w| {
                    if let Some(t) = p.get("threads").and_then(|t| t.as_u64()) {
                        w.threads = t;
                    }
                });
                Ok(match self.claim(&worker, &kinds)? {
                    Some(j) => json_obj! {"job" => json_obj! {"id" => j.id, "kind" => j.kind, "payload" => j.payload, "attempt" => j.attempts}},
                    None => json_obj! {"job" => Value::Null},
                })
            }
            "complete" => {
                self.complete(&id("job").ok_or("`job` is required")?, &s("worker").ok_or("`worker` is required")?, p.get("result").cloned().unwrap_or(Value::Null))?;
                Ok(Value::Null)
            }
            "fail" => {
                self.fail(&id("job").ok_or("`job` is required")?, &s("worker").ok_or("`worker` is required")?, &s("error").unwrap_or_default())?;
                Ok(Value::Null)
            }
            "heartbeat" => {
                let active: Vec<Uuid> = p.get("jobs").and_then(|j| j.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().and_then(Uuid::parse)).collect()).unwrap_or_default();
                self.heartbeat(&s("worker").ok_or("`worker` is required")?, p.get("threads").and_then(|t| t.as_u64()).unwrap_or(0), &active);
                Ok(Value::Null)
            }
            "submit" => {
                let job = self.submit(&s("kind").ok_or("`kind` is required")?, p.get("payload").cloned().unwrap_or(Value::Null))?;
                Ok(json_obj! {"id" => job.id})
            }
            "status" => {
                let job = self.get_job(&id("job").ok_or("`job` is required")?).ok_or("no such job")?;
                Ok(job.to_api())
            }
            other => Err(format!("unknown method `{other}`")),
        }
    }
}

/// Build batch payloads from a request spec:
///
/// * `{"payloads": [..]}` as-is
/// * `{"range": {"start": 0, "end": 1000000, "chunks": 8}}` -> `{start, end}` pieces
/// * `{"samples": 1000000, "chunks": 8, "seed": 1}` -> `{samples, seed}` pieces
pub fn split(spec: &Value) -> Result<Vec<Value>, String> {
    if let Some(p) = spec.get("payloads").and_then(|p| p.as_array()) {
        return Ok(p.clone());
    }
    if let Some(r) = spec.get("range") {
        let start = r.get("start").and_then(|v| v.as_u64()).ok_or("`range.start` is required")?;
        let end = r.get("end").and_then(|v| v.as_u64()).ok_or("`range.end` is required")?;
        let chunks = r.get("chunks").and_then(|v| v.as_u64()).unwrap_or(8).clamp(1, 100_000);
        if end <= start {
            return Err("`range.end` must be greater than `range.start`".into());
        }
        let span = end - start;
        let size = span.div_ceil(chunks).max(1);
        let mut out = Vec::new();
        let mut a = start;
        while a < end {
            let b = (a + size).min(end);
            out.push(json_obj! {"start" => a, "end" => b});
            a = b;
        }
        return Ok(out);
    }
    if let Some(samples) = spec.get("samples").and_then(|v| v.as_u64()) {
        let chunks = spec.get("chunks").and_then(|v| v.as_u64()).unwrap_or(8).clamp(1, 100_000);
        let seed = spec.get("seed").and_then(|v| v.as_u64()).unwrap_or(1);
        let per = samples / chunks;
        if per == 0 {
            return Err("`samples` must be at least `chunks`".into());
        }
        return Ok((0..chunks).map(|i| json_obj! {"samples" => per, "seed" => seed + i}).collect());
    }
    Err("give `payloads`, `range` or `samples`".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched() -> Arc<Scheduler> {
        Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig::default()).unwrap()
    }

    #[test]
    fn claim_complete_in_order_with_kind_filter() {
        let s = sched();
        let a = s.submit("echo", Value::Int(1)).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let b = s.submit("primes", Value::Int(2)).unwrap();
        // kind filter skips the head of the queue
        let got = s.claim("w1", &["primes".into()]).unwrap().unwrap();
        assert_eq!(got.id, b.id);
        let got_a = s.claim("w2", &[]).unwrap().unwrap();
        assert_eq!(got_a.id, a.id);
        assert!(s.claim("w2", &[]).unwrap().is_none());
        // only the leaseholder may complete
        assert!(s.complete(&a.id, "w1", Value::Null).is_err());
        s.complete(&a.id, "w2", Value::Int(9)).unwrap();
        assert!(s.complete(&a.id, "w2", Value::Int(9)).is_err(), "already done");
        assert_eq!(s.get_job(&a.id).unwrap().result, Some(Value::Int(9)));
        assert_eq!(s.stats(), Stats { queued: 0, running: 1, done: 1, failed: 0 });
    }

    #[test]
    fn retries_then_fails() {
        let s = Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig { max_attempts: 2, ..Default::default() }).unwrap();
        let j = s.submit("echo", Value::Null).unwrap();
        s.claim("w", &[]).unwrap().unwrap();
        s.fail(&j.id, "w", "bad").unwrap();
        assert_eq!(s.get_job(&j.id).unwrap().state, JobState::Queued);
        let again = s.claim("w", &[]).unwrap().unwrap();
        assert_eq!(again.attempts, 2);
        s.fail(&j.id, "w", "bad again").unwrap();
        let done = s.get_job(&j.id).unwrap();
        assert_eq!(done.state, JobState::Failed);
        assert_eq!(done.error.as_deref(), Some("bad again"));
    }

    #[test]
    fn lease_expiry_requeues() {
        let s = Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig { lease_secs: 0, ..Default::default() }).unwrap();
        let j = s.submit("echo", Value::Null).unwrap();
        s.claim("dead-worker", &[]).unwrap().unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(s.expire_leases(), 1);
        assert_eq!(s.get_job(&j.id).unwrap().state, JobState::Queued);
        assert!(s.complete(&j.id, "dead-worker", Value::Null).is_err(), "stale worker is rejected");
        let again = s.claim("w2", &[]).unwrap().unwrap();
        assert_eq!(again.id, j.id);
    }

    #[test]
    fn batches_reduce_and_fail() {
        let s = sched();
        let b = s.submit_batch("echo", Reduce::Sum, vec![Value::Int(1), Value::Int(2), Value::Int(3)]).unwrap();
        for _ in 0..3 {
            let j = s.claim("w", &[]).unwrap().unwrap();
            let v = j.payload.clone();
            s.complete(&j.id, "w", v).unwrap();
        }
        let b = s.get_batch(&b.id).unwrap();
        assert_eq!((b.state, b.result), (JobState::Done, Some(Value::Int(6))));

        let s2 = Scheduler::new(Database::memory().scope("hpc").unwrap(), SchedulerConfig { max_attempts: 1, ..Default::default() }).unwrap();
        let b = s2.submit_batch("echo", Reduce::Sum, vec![Value::Int(1), Value::Int(2)]).unwrap();
        let j1 = s2.claim("w", &[]).unwrap().unwrap();
        s2.complete(&j1.id, "w", Value::Int(1)).unwrap();
        let j2 = s2.claim("w", &[]).unwrap().unwrap();
        s2.fail(&j2.id, "w", "nope").unwrap();
        let b = s2.get_batch(&b.id).unwrap();
        assert_eq!(b.state, JobState::Failed);
        assert!(b.error.unwrap().contains("nope"));
    }

    #[test]
    fn cancel_and_validation() {
        let s = sched();
        let j = s.submit("echo", Value::Null).unwrap();
        assert!(s.cancel(&j.id).unwrap());
        assert!(!s.cancel(&j.id).unwrap());
        assert!(s.claim("w", &[]).unwrap().is_none());
        assert!(s.submit("bad kind!", Value::Null).is_err());
        assert!(s.submit_batch("echo", Reduce::Sum, vec![]).is_err());
    }

    #[test]
    fn restart_recovers_queue() {
        let db = Database::memory().scope("hpc").unwrap();
        let s = Scheduler::new(db.clone(), SchedulerConfig::default()).unwrap();
        let j = s.submit("echo", Value::Null).unwrap();
        drop(s);
        let s2 = Scheduler::new(db, SchedulerConfig::default()).unwrap();
        assert_eq!(s2.claim("w", &[]).unwrap().unwrap().id, j.id);
    }

    #[test]
    fn splitting() {
        let r = split(&json_obj! {"range" => json_obj!{"start" => 0, "end" => 10, "chunks" => 3}}).unwrap();
        let spans: Vec<(u64, u64)> = r.iter().map(|p| (p.get("start").unwrap().as_u64().unwrap(), p.get("end").unwrap().as_u64().unwrap())).collect();
        assert_eq!(spans, vec![(0, 4), (4, 8), (8, 10)]);
        assert_eq!(split(&json_obj! {"samples" => 100, "chunks" => 4}).unwrap().len(), 4);
        assert!(split(&json_obj! {"range" => json_obj!{"start" => 5, "end" => 5}}).is_err());
        assert!(split(&json_obj! {}).is_err());
        assert_eq!(split(&json_obj! {"payloads" => vec![1, 2]}).unwrap().len(), 2);
    }
}
