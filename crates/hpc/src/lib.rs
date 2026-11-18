//! # HPC
//!
//! A small high-performance-computing layer on top of the [p2p](p2p) mesh.
//!
//! ```text
//!  Server 1  Scheduler  (dashboard + job API, owns the job/batch tables in its database scope)
//!  Server 2  Worker  ─┐
//!  Server 3  Worker  ─┼─ pull jobs from the scheduler over the mesh
//!  Server 4  Worker  ─┘
//! ```
//!
//! * [`Scheduler`] keeps [`Job`]s and [`Batch`]es (many jobs reduced into one result) in a
//!   database scope, hands jobs out with a lease, requeues jobs whose worker vanished, and
//!   exposes an RPC service (`hpc`) on the node plus an http dashboard/API ([`router`]).
//! * [`Worker`] runs on any node, **even one without an open port**: it *pulls* work with
//!   `claim` calls, executes it on a pool of threads and reports the result.
//! * [`TaskRegistry`] maps a job `kind` to a Rust function. Built-ins: `echo`, `sleep`, `primes`
//!   (count primes in `[start, end)`), `pi` (Monte-Carlo hits), `sha256_chain`, `wordcount`. Add
//!   your own with [`TaskRegistry::register`].

mod job;
mod scheduler;
mod tasks;
mod web;
mod worker;

pub use job::{Batch, Job, JobState, Reduce};
pub use scheduler::{Scheduler, SchedulerConfig, Stats, WorkerInfo, split};
pub use tasks::{TaskFn, TaskRegistry};
pub use web::router;
pub use worker::{Worker, WorkerConfig};

/// Role advertised by nodes running a scheduler.
pub const ROLE_SCHEDULER: &str = "hpc-scheduler";
/// Role advertised by nodes running workers.
pub const ROLE_WORKER: &str = "hpc-worker";
/// Name of the mesh service the scheduler registers.
pub const SERVICE: &str = "hpc";
