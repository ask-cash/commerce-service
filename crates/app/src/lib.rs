//! Application layer: use cases and the ports they depend on.
//!
//! Use cases take ports as traits (a repository, a payment provider, an event
//! publisher) so adapters can be swapped for in-memory fakes in tests. Each
//! feature adds its use case module and the ports it needs here.

pub mod error;

pub use error::AppError;
