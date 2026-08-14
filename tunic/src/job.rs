use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{State, db::Table};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, PartialOrd)]
pub enum JobResult {
    NotStarted,
    Running,
    Continue,
    Done,
    Error(String),
}

#[async_trait::async_trait]
#[typetag::serde(tag = "type")]
pub trait JobTrait: Send + Sync {
    fn allow_concurrent(&self) -> bool;
    async fn run(&mut self) -> JobResult;
}

#[derive(Serialize, Deserialize)]
#[tunic_macro::make_crud(methods = [post, get], state = State, table = state.jobs)]
pub struct Job {
    pub id: Uuid,
    pub delay: u64,
    pub created_at: u64,
    pub last_run_at: u64,
    pub state: JobResult,
    pub payload: Box<dyn JobTrait>,
}

impl Job {
    pub async fn init(
        payload: Box<dyn JobTrait>,
        delay: u64,
        jobs: Table<Uuid, Job>,
    ) -> Option<Uuid> {
        let allow_conc = payload.allow_concurrent();

        // Check if a job of the same type already exists and handle concurrency rules
        if !allow_conc {
            // Check if any active/running job has the same type name
            let payload_type = std::any::type_name_of_val(&*payload);
            let mut already_running = false;
            for j in jobs.values().await {
                let j = j.lock().await;
                if std::any::type_name_of_val(&*j.payload) == payload_type
                    && j.state == JobResult::Running
                {
                    already_running = true;
                    break;
                }
            }
            if already_running {
                // Cannot initialize due to concurrency constraints
                return None;
            }
        }

        let id = Uuid::new_v4();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let job = Job {
            id,
            delay,
            created_at: now,
            last_run_at: 0,
            state: JobResult::Continue,
            payload,
        };

        jobs.insert(id, job).await;

        //Self::save_to_disk(&jobs, storage_path).await;

        Self::run_job_task(id, jobs.clone()).await;

        Some(id)
    }

    pub async fn run_job_task(id: Uuid, jobs: Table<Uuid, Job>) {
        tokio::spawn(async move {
            loop {
                // Update state to Running
                let (delay, mut payload) = {
                    if let Some(job) = jobs.get_mut(&id).await {
                        let mut job = job.lock().await;
                        job.state = JobResult::Running;
                        job.last_run_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        (job.delay, clone_trait_box(&mut *job.payload))
                    } else {
                        break;
                    }
                };

                // Execute job payload run function
                let result = payload.run().await;

                // Update state post-run
                if let Some(job) = jobs.get_mut(&id).await {
                    let mut job = job.lock().await;
                    job.state = result.clone();
                }

                //Self::save_to_disk(&jobs, &storage_path).await;

                match result {
                    JobResult::Continue => {
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    }
                    _ => break, // Stop loop if Done or Error
                }
            }
        });
    }

    /*
    pub async fn save_to_disk(jobs: &Table<Uuid, Job>, path: &str) {
        let map = jobs.lock().await;
        if let Ok(serialized) = serde_json::to_string_pretty(&*map) {
            let _ = fs::write(path, serialized);
        }
    }

    pub async fn load_from_disk(jobs: &Table<Uuid, Job>, path: &str, storage_path: &str) {
        if let Ok(data) = fs::read_to_string(path) {
            if let Ok(loaded_map) = serde_json::from_str::<BTreeMap<Uuid, Arc<Mutex<Job>>>>(&data) {
                let mut map = jobs.lock().await;
                *map = loaded_map;

                // Restart looping jobs found in storage
                for (&id, job) in map.iter() {
                    if job.state == JobResult::Continue || job.state == JobResult::Running {
                        Self::run_job_task(id, jobs.clone(), storage_path.to_string()).await;
                    }
                }
            }
        }
    }
    */
}

// Helper to clone trait object boxes for async task mapping
fn clone_trait_box(t: &dyn JobTrait) -> Box<dyn JobTrait> {
    // Serialization roundtrip trick to clone dynamic trait objects safely in Rust
    let serialized = serde_json::to_string(t).unwrap();
    serde_json::from_str(&serialized).unwrap()
}
