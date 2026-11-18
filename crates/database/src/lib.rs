//! # Database
//!
//! Storage built from Rust structs. Derive [`Record`] on a struct and keep it in a [`Table`].
//!
//! ```
//! use database::{Database, Record, Table, Uuid};
//!
//! #[derive(Clone, Debug, PartialEq, Record)]
//! struct Song { id: Uuid, title: String, #[record(default)] plays: u64 }
//!
//! let db = Database::memory().scope("music").unwrap();   // a scope is always required
//! let songs: Table<Song> = Table::new(db).unwrap();
//! let s = Song { id: Uuid::new_v4(), title: "Suffocate".into(), plays: 0 };
//! songs.insert(&s).unwrap();
//! songs.update(&s.id, |s| s.plays += 1).unwrap();
//! assert_eq!(songs.get(&s.id).unwrap().unwrap().plays, 1);
//! ```
//!
//! ## Where data lives
//! A [`Backend`] stores JSON documents addressed by `scope / collection / id`.
//! * [`LocalBackend`]: in memory, or in a directory on the filesystem (append-only log per
//!   collection, compacted on open)
//! * [`RemoteBackend`]: another process's database, over TCP, spoken to a [`DbServer`]
//! * [`RoutedBackend`]: sends different scopes to different backends (multiple databases)
//!
//! ## Scopes
//! Every access goes through a [`Database`] that has a [`Scope`]: a `/`-separated name such as
//! `user`, `drive` or `myapp/uploads`. Code holding a scoped handle can only reach that scope and
//! its children, and a [`DbServer`] can restrict each access token to one scope.
//!
//! ## Wire protocol
//! `[u8 frame type][u32 big-endian length][json payload]`, frame types
//! `0 Request`, `1 Response`, `2 Sync`, `3 Hello`.

extern crate self as database;

mod backend;
mod crud;
mod error;
mod field;
mod record;
mod remote;
mod scope;
mod server;
mod table;
mod wire;

pub use backend::{Backend, Entry, LocalBackend, RoutedBackend, replicate};
pub use crud::{CrudOptions, crud};
pub use error::DbError;
pub use field::FieldValue;
pub use proc_macros::Record;
pub use record::Record;
pub use remote::RemoteBackend;
pub use scope::{Database, Scope};
pub use server::{DbServer, Tokens};
pub use table::Table;
pub use utils::{Uuid, Value};
