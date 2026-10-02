//! A deliberately small Stripe client.
//!
//! We wrap only the endpoints we use, pin the API version on every request,
//! and require an idempotency key on every write. Stripe types stay in this
//! crate; the store/app layers map them into domain types.

mod client;
mod error;
pub mod webhook;

pub use client::{StripeClient, StripeConfig};
pub use error::{StripeApiError, StripeError};
