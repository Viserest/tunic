//! # Drive
//!
//! Files with owners, folders, sharing permissions and media playlists.
//!
//! * File **bytes** live in a directory on disk (`<files_dir>/<id>`), **metadata**, folders,
//!   permissions and playlists live in the database scope you give [`Drive::new`].
//! * Access levels: [`Level::Commenter`] < [`Level::Viewer`] < [`Level::Editor`] < [`Level::Owner`].
//!   Owners can share a file or folder with other users; files inherit the permissions of the
//!   folders above them.
//!
//! Routes (see [`router`]): `POST /storage/files` (raw body; name in the `Og-Filename` header),
//! `GET|DELETE /storage/files/:id`, `GET /storage/files`, `POST /storage/files/:id/share`,
//! `/storage/dirs`, and `/media/playlists`.

mod mime;
mod model;
mod service;
mod web;

pub use mime::{MimeType, safe_to_inline, sanitize_filename};
pub use model::{Directory, FileMeta, Level, Permission, Playlist};
pub use service::{Drive, DriveConfig, DriveError};
pub use web::router;
