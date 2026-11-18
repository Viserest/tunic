//! # Intelligence
//!
//! AI that is not an LLM: a small exact-rules maths engine.
//!
//! * [`parse`] text into an [`Expr`] tree (`2 + 3*x^2`, `sin(x)/x`, implicit `2x`)
//! * [`Expr::eval`] numerically with variables
//! * [`Expr::derivative`] symbolically, with simplification
//! * [`integrate`] numerically (adaptive Simpson), [`limit`] numerically from both sides
//!
//! Routes: `GET /intelligence/{calc,derive,integrate,limit}?expr=...` (json answers).

mod expr;
mod routes;

pub use expr::{Expr, MathError, integrate, limit, parse};
pub use routes::router;
