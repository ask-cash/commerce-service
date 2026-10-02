//! Pure domain model for commerce-service.
//!
//! This crate does no I/O and knows nothing about Stripe, HTTP or Postgres.
//! Adapters translate their types into these at the edges.

pub mod ids;
pub mod money;

pub use ids::{CustomerId, EventId, TenantId};
pub use money::{Currency, CurrencyError, Money, MoneyError};
