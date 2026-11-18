//! # Tunic router
//!
//! One router for **HTTP/1.x**, **Gopher** and **Gemini**.
//!
//! ## Concept
//! 1. A listener accepts a connection; the protocol codec maps the bytes to a [`Request`] that
//!    remembers its [`Protocol`].
//! 2. The [`Router`] matches the request (host, method, path pattern) and checks the route's *tag*
//!    against the [`Policy`] (is the tag enabled, is the client address allowed).
//! 3. The route's handler returns a [`Response`].
//! 4. The codec converts the response back into the protocol's wire format.
//!
//! Handlers can answer every protocol with the same code: [`Response::page`] renders an
//! [`element::Page`] as html, gemtext or plain text depending on `req.protocol`.
//!
//! ## Ports
//! Http defaults to 80, Gopher to 70, Gemini to 1965 (the application chooses the real ports).
//!
//! ## Scope of this build
//! The router is built on `std` only. HTTP/1.0/1.1 (keep-alive, chunked request bodies),
//! Gopher and Gemini are implemented. Gemini mandates TLS: plug a TLS implementation in through
//! the [`Acceptor`] trait (or terminate TLS in front, e.g. stunnel/haproxy). HTTP/2 and HTTP/3
//! need a TLS stack and QUIC and are likewise meant to be added as extra codecs behind
//! [`Acceptor`]/[`Protocol`].

mod client;
mod core;
mod gemini;
mod gopher;
mod http;
mod proxy;
mod route;
mod server;

pub use client::{ClientResponse, request as http_request};
pub use core::*;
pub use gopher::{GopherItem, menu as gopher_menu};
pub use proxy::proxy_handler;
pub use route::*;
pub use server::*;
