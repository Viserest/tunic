//! # User
//!
//! Users as accounts with sessions. [`Users`] is the service object (create users, log in, look up
//! sessions from a request); [`router`] exposes login/home/admin pages over any protocol.
//!
//! Every `Users` lives in a database scope, so a library user can run several independent user
//! directories side by side: `Users::new(db.scope("myapp/users")?)`.
//!
//! Passwords are stored as salted PBKDF2-HMAC-SHA256 hashes ([`utils::password`]).

mod owned;
mod service;
mod web;

pub use owned::{Owned, owned_crud};
pub use service::{Session, User, Users, UsersConfig};
pub use web::router;
