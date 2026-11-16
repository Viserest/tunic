use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::{
    sync::Mutex,
    time::{Duration, sleep},
};
use uuid::Uuid;

use crate::unix_now;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, PartialOrd)]
pub enum JobResult {
    NotStarted,
    Running,
    Continue,
    Done,
    Error(String),
}

#[derive(Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub delay: u64,
    pub created_at: u64,
    pub last_run_at: u64,
    pub state: JobResult,
}

impl Job {
    pub async fn run_spawn<F, Fut>(job: Arc<Mutex<Job>>, callback: F)
    where
        F: FnOnce() -> Fut + Send + Copy + 'static,
        Fut: Future<Output = JobResult> + Send,
    {
        tokio::spawn(async move {
            loop {
                let mut guard = job.lock().await;
                guard.state = JobResult::Running;
                guard.last_run_at = unix_now();

                // Execute job payload run function
                guard.state = callback().await;

                match guard.state {
                    // Schedule rerun if Continue
                    JobResult::Continue => {
                        sleep(Duration::from_secs(guard.delay)).await;
                    }
                    // Stop loop if Done or Error
                    _ => break,
                }
            }
        });
    }
}
