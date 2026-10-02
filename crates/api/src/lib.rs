//! HTTP adapter for commerce-service.
//!
//! Two routers on two ports:
//! - **public** (`:8080`): `/v1/*` for Cash (JWT-authenticated) and, later,
//!   `/webhooks/stripe`. Only the webhook path is exposed through the ingress.
//! - **admin** (`:9090`): `/healthz`, `/readyz`, `/metrics`. Cluster-internal.

pub mod admin;
pub mod auth;
pub mod error;
mod middleware;
mod routes;
mod state;

pub use admin::{AdminState, admin_router};
pub use auth::{AuthConfig, Authenticator, Caller, PublicKey};
pub use error::{ApiError, ErrorCode};
pub use routes::public_router;
pub use state::AppState;
