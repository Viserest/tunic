//! # Tunic
//!
//! One executable, many optional parts, all of them nodes of a distributed mesh.
//!
//! * **Binary**: `apps/tunic` is `tunic::cli::main_with(tunic::App::new())`. Command line options,
//!   `tunic.toml` and `TUNIC_*` environment variables decide which *features* run.
//! * **Library**: build your own [`App`]: add routes, [`App::table`]s, [`Feature`]s, change which
//!   built-in features are on, and scope where accounts and files live in the database.
//!
//! "Feature" here means a switchable part of the application (drive, calendar, hpc.worker, ...), not
//! a cargo feature.
//!
//! ```no_run
//! use tunic::{App, Value};
//! let running = App::new()
//!     .disable("food")
//!     .users("my-app/accounts")
//!     .files("my-app/files")
//!     .default_value("router.protocols.http", Value::Int(9000))
//!     .start()
//!     .unwrap();
//! running.wait();
//! ```

pub mod app;
pub mod builtin;
pub mod cli;
pub mod config;
pub mod feature;

pub use app::{App, Running, core_options};
pub use builtin::RelayAddr;
pub use config::{Config, Kind, Opt};
pub use feature::{Context, Feature, FnFeature};

// the parts, re-exported so a library user needs only this crate
pub use calendar;
pub use database;
pub use database as db;
pub use drive;
pub use element;
pub use food;
pub use hpc;
pub use intelligence;
pub use messages;
pub use p2p;
pub use router;
pub use user;
pub use utils;
pub use utils::{Shutdown, Value};
