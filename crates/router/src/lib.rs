//! # Tunic router
//!
//! Tunic router for Http, Gopher, and Gemini
//!
//! ## Concept
//! 1. The initial request can be of any type. It's mapped to a [`Request`](crate::Request) and stores the protocol.
//! 2. The [`Request`](crate::Request) transverses the [`Router`](crate::Router) until it matches a [`Route`](crate::Route).
//! 3. The [`Route`](crate::Route) calls the callback function which returns a [`Response`](crate::Response).
//! 4. The [`Response`](crate::Response) is converted into it's initial protocol's response.
//! 5. The response is sent asynchronisly.
//!
//! ## Request mapping
//! * Additional mappings need to be easy to implement
//!
//! ### Http request
//! Transfers: the Version, URI, Method, Headers, Extension, and Body (as Vec<u8>)
//!
//! ### Gopher request
//! Transfers only the Selector. Method is always GET.
//!
//! ### Gemini request
//! Transfers only the URL. Method is always GET.
//!
//! ## Ports
//! - Http defaults to 80
//! - Gopher defaults to 70
//! - Gemini defaults to 1965

// Macros
#[macro_use]
mod macros;

// Core
mod core;
mod router;
mod serve;

pub use core::*;
pub use router::*;
pub use serve::*;

// Protocols
mod gemini;
mod gopher;
mod http;
