use std::sync::Arc;
use std::time::Duration;

use commerce_store::Db;
use commerce_stripe::StripeClient;
use secrecy::SecretString;

use crate::auth::Authenticator;

/// Shared state for the public router. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub stripe: StripeClient,
    /// Stripe endpoint signing secrets; more than one during rotation.
    pub webhook_secrets: Arc<[SecretString]>,
    pub auth: Arc<Authenticator>,
    pub request_timeout: Duration,
}
