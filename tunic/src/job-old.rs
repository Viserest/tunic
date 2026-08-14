use std::{sync::Arc, time::Duration};

use axum::{Router, routing::post};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{State, unix_now};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, PartialOrd)]
pub enum JobResult {
    NotStarted,
    Continue,
    Done,
    Error(String),
}

#[async_trait::async_trait]
#[typetag::serde(tag = "type")]
pub trait JobTrait: Send + Sync {
    fn clone_box(&self) -> Box<dyn JobTrait + Send + Sync>;
    fn allow_concurrent(&self) -> bool;
    async fn run(&mut self) -> JobResult;
}

#[derive(Serialize, Deserialize)]
#[tunic_macro::make_crud(methods = [post, get], state = State, table = state.jobs)]
pub struct Job {
    #[skip(post)]
    id: Uuid,
    delay: u64,
    payload: Box<dyn JobTrait + Send + Sync>,
    pub state: JobResult,
    #[skip(post)]
    created_at: u64,
    #[skip(post)]
    last_ran_at: u64,
}

impl Clone for Job {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            delay: self.delay,
            payload: self.payload.clone_box(),
            state: self.state.clone(),
            created_at: self.created_at,
            last_ran_at: self.last_ran_at,
        }
    }
}

impl Job {
    pub fn new(delay: u64, payload: Box<dyn JobTrait + Send + Sync>) -> Self {
        Self {
            id: Uuid::now_v7(),
            delay,
            payload: payload,
            state: JobResult::NotStarted,
            created_at: unix_now(),
            last_ran_at: 0,
        }
    }
    /// Add self to jobs table in database
    /// Returns error on any errors
    /// Returns ok with if job started (true) or not (false)
    pub async fn init(&mut self, state: Arc<State>) -> Result<bool, String> {
        // Can a job run concurrently
        println!("[INFO] Concurrent check...");
        if !self.payload.allow_concurrent() {
            // Check if job is already running
            for (_, job_iter) in state.jobs.lock().await.iter() {
                if self.payload.typetag_name() == job_iter.lock().await.payload.typetag_name() {
                    if self.state == JobResult::Continue {
                        // Its running
                        return Ok(false);
                    }
                }
            }
        }
        println!("[INFO] Job allowed to run. Saving job to database...");

        // Add job to persistent table
        state
            .jobs
            .lock()
            .await
            .insert(self.id, Arc::new(Mutex::new(self.clone())));
        println!("[INFO] Job saved. Starting job...");

        // Check if job needs to run
        self.run(state).await;

        Ok(true)
    }
    pub async fn run(&self, state: Arc<State>) {
        // Elapsed: gap between now and last ran
        // If gap is greater than delay: run
        // If gap is smaller than delay: find difference then sleep for difference
        let elapsed = unix_now().saturating_sub(self.last_ran_at);
        let job_cloned = state.jobs.lock().await.get(&self.id).unwrap().clone();

        // Spawn a seperate thread if it needs to wait or run now
        tokio::task::spawn(async move {
            println!("[INFO] Thread spawned for job...");
            let mut guard = job_cloned.lock().await;

            // Run job once its the right time
            if elapsed < guard.delay {
                let wait = guard.delay - elapsed;
                println!("[INFO] Job will run in {} seconds.", wait);
                // find difference then sleep for difference
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            println!("[INFO] Job running...");
            let ret = guard.payload.run().await;
            println!("[INFO] Job ran. Saving state...");

            // Handle resulting state
            let mut jobs = state.jobs.lock_mut().await;
            let mut job = match jobs.get_mut(&guard.id) {
                Some(job) => job.lock().await,
                None => {
                    println!("[WARN] While looking up on table: Nothing found");
                    return ();
                }
            };
            job.state = ret.clone();
            job.last_ran_at = unix_now();
            println!("[INFO] State saved.");

            // Restart countdown if it should continue
            match ret {
                JobResult::Continue => {
                    guard.payload.run().await;
                }
                _ => {}
            }
        });
    }
}

pub fn admin_router() -> Router {
    Router::new().route("/", post(Job::handle_post).get(Job::handle_get))
}
