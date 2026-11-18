use std::sync::Arc;

use element::{Page, heading, table, text};
use router::{Request, Response, Router};
use utils::{Uuid, Value, json_obj, unix_now};

use crate::{Job, JobState, Reduce, Scheduler, split};

type Guard = Arc<dyn Fn(&Request) -> Option<Response> + Send + Sync>;

fn short(id: &Uuid) -> String {
    id.to_string()[..8].to_string()
}

/// Scheduler dashboard and job API. All routes carry the tag `admin`.
///
/// | route | |
/// |---|---|
/// | `GET /hpc` | dashboard page (html, gemtext or text) |
/// | `GET /hpc/stats`, `GET /hpc/workers`, `GET /hpc/jobs?state=&limit=`, `GET /hpc/batches` | json |
/// | `POST /hpc/jobs` `{"kind","payload"}` | submit one job |
/// | `GET /hpc/jobs/:id`, `POST /hpc/jobs/:id/cancel` | |
/// | `POST /hpc/batches` `{"kind","reduce","range"｜"samples"｜"payloads"}` | submit a batch |
/// | `GET /hpc/batches/:id` | batch state and reduced result |
///
/// `guard` runs before every route (use it to require a login).
pub fn router(s: Arc<Scheduler>, guard: Option<Guard>) -> Router {
    let g = move |f: Arc<dyn Fn(&Request) -> Response + Send + Sync>| {
        let guard = guard.clone();
        move |req: &Request| {
            if let Some(g) = &guard {
                if let Some(deny) = g(req) {
                    return deny;
                }
            }
            f(req)
        }
    };
    let arc = |f: Box<dyn Fn(&Request) -> Response + Send + Sync>| -> Arc<dyn Fn(&Request) -> Response + Send + Sync> { Arc::from(f) };

    let sc = s.clone();
    let dashboard = arc(Box::new(move |req| {
        let st = sc.stats();
        let now = unix_now();
        let workers: Vec<Vec<String>> = sc
            .workers()
            .into_iter()
            .map(|w| vec![w.name, w.threads.to_string(), w.active.to_string(), w.completed.to_string(), w.failed.to_string(), format!("{}s ago", now.saturating_sub(w.last_seen))])
            .collect();
        let jobs: Vec<Vec<String>> = sc
            .list_jobs(None, 25)
            .into_iter()
            .map(|j| vec![short(&j.id), j.kind, j.state.as_str().to_string(), j.worker.unwrap_or_default(), j.attempts.to_string(), j.result.map(|r| r.to_json()).or(j.error).unwrap_or_default()])
            .collect();
        let batches: Vec<Vec<String>> = sc
            .list_batches(10)
            .into_iter()
            .map(|b| vec![short(&b.id), b.kind, b.job_ids.len().to_string(), b.state.as_str().to_string(), b.result.map(|r| r.to_json()).or(b.error).unwrap_or_default()])
            .collect();
        let page = Page::new("HPC scheduler")
            .add(heading(1, "HPC scheduler"))
            .add(text(format!("Queued {} / running {} / done {} / failed {}", st.queued, st.running, st.done, st.failed)))
            .add(heading(2, "Workers"))
            .add(table(["Name", "Threads", "Active", "Completed", "Failed", "Last seen"], workers))
            .add(heading(2, "Recent jobs"))
            .add(table(["Id", "Kind", "State", "Worker", "Attempts", "Result"], jobs))
            .add(heading(2, "Recent batches"))
            .add(table(["Id", "Kind", "Jobs", "State", "Result"], batches))
            .add(text("Submit with POST /hpc/jobs or POST /hpc/batches (json)."));
        Response::page(req, &page)
    }));

    let sc = s.clone();
    let stats = arc(Box::new(move |_| {
        let st = sc.stats();
        Response::json(&json_obj! {"queued" => st.queued, "running" => st.running, "done" => st.done, "failed" => st.failed})
    }));

    let sc = s.clone();
    let workers = arc(Box::new(move |_| {
        let w: Vec<Value> = sc
            .workers()
            .into_iter()
            .map(|w| json_obj! {"name" => w.name, "threads" => w.threads, "active" => w.active, "completed" => w.completed, "failed" => w.failed, "last_seen" => w.last_seen})
            .collect();
        Response::json(&json_obj! {"workers" => w})
    }));

    let sc = s.clone();
    let jobs = arc(Box::new(move |req| {
        let state = req.query_param("state").and_then(JobState::parse);
        let limit = req.query_param("limit").and_then(|l| l.parse().ok()).unwrap_or(50usize).clamp(1, 500);
        let list: Vec<Value> = sc.list_jobs(state, limit).iter().map(Job::to_api).collect();
        Response::json(&json_obj! {"jobs" => list})
    }));

    let sc = s.clone();
    let submit = arc(Box::new(move |req| {
        let body = match req.json() {
            Ok(b) => b,
            Err(e) => return Response::bad_request(e),
        };
        let Some(kind) = body.get("kind").and_then(|k| k.as_str()) else { return Response::bad_request("`kind` is required") };
        match sc.submit(kind, body.get("payload").cloned().unwrap_or(Value::Null)) {
            Ok(j) => Response::json(&json_obj! {"msg" => "Successfully created", "content" => j.id}).status(201),
            Err(e) => Response::bad_request(e),
        }
    }));

    let sc = s.clone();
    let job = arc(Box::new(move |req| match req.param("id").and_then(Uuid::parse).and_then(|id| sc.get_job(&id)) {
        Some(j) => Response::json(&json_obj! {"content" => j.to_api()}),
        None => Response::error(404, "Nothing found"),
    }));

    let sc = s.clone();
    let cancel = arc(Box::new(move |req| match req.param("id").and_then(Uuid::parse) {
        Some(id) => match sc.cancel(&id) {
            Ok(true) => Response::ok_msg("Cancelled"),
            Ok(false) => Response::error(409, "Only queued jobs can be cancelled"),
            Err(e) => Response::internal(e),
        },
        None => Response::bad_request("bad id"),
    }));

    let sc = s.clone();
    let submit_batch = arc(Box::new(move |req| {
        let body = match req.json() {
            Ok(b) => b,
            Err(e) => return Response::bad_request(e),
        };
        let Some(kind) = body.get("kind").and_then(|k| k.as_str()) else { return Response::bad_request("`kind` is required") };
        let reduce = match body.get("reduce").and_then(|r| r.as_str()) {
            None => Reduce::Sum,
            Some(r) => match Reduce::parse(r) {
                Some(r) => r,
                None => return Response::bad_request("`reduce` must be sum, max, min or concat"),
            },
        };
        let payloads = match split(&body) {
            Ok(p) => p,
            Err(e) => return Response::bad_request(e),
        };
        match sc.submit_batch(kind, reduce, payloads) {
            Ok(b) => Response::json(&json_obj! {"msg" => "Successfully created", "content" => b.id, "jobs" => b.job_ids.len()}).status(201),
            Err(e) => Response::bad_request(e),
        }
    }));

    let sc = s.clone();
    let batch = arc(Box::new(move |req| match req.param("id").and_then(Uuid::parse).and_then(|id| sc.get_batch(&id)) {
        Some(b) => {
            let (mut done, mut failed) = (0, 0);
            for id in &b.job_ids {
                match sc.get_job(id).map(|j| j.state) {
                    Some(JobState::Done) => done += 1,
                    Some(JobState::Failed) => failed += 1,
                    _ => {}
                }
            }
            Response::json(&json_obj! {"content" => json_obj! {
                "id" => b.id, "kind" => b.kind, "state" => b.state.as_str(), "jobs" => b.job_ids.len(),
                "jobs_done" => done, "jobs_failed" => failed, "result" => b.result, "error" => b.error,
            }})
        }
        None => Response::error(404, "Nothing found"),
    }));

    let sc = s;
    let batches = arc(Box::new(move |_| {
        let list: Vec<Value> = sc
            .list_batches(50)
            .into_iter()
            .map(|b| json_obj! {"id" => b.id, "kind" => b.kind, "state" => b.state.as_str(), "jobs" => b.job_ids.len(), "result" => b.result})
            .collect();
        Response::json(&json_obj! {"batches" => list})
    }));

    Router::new()
        .get("/hpc", g(dashboard))
        .get("/hpc/stats", g(stats))
        .get("/hpc/workers", g(workers))
        .get("/hpc/jobs", g(jobs))
        .post("/hpc/jobs", g(submit))
        .get("/hpc/jobs/:id", g(job))
        .post("/hpc/jobs/:id/cancel", g(cancel))
        .get("/hpc/batches", g(batches))
        .post("/hpc/batches", g(submit_batch))
        .get("/hpc/batches/:id", g(batch))
        .tagged("admin")
}
