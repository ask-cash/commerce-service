use serde::Deserialize;

/// The `error` object Stripe returns on non-2xx responses.
#[derive(Debug, Clone, Deserialize)]
pub struct StripeApiError {
    #[serde(rename = "type")]
    pub kind: String,
    pub code: Option<String>,
    pub decline_code: Option<String>,
    pub message: Option<String>,
    pub param: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    /// Stripe answered with a 4xx: the request itself is wrong or declined.
    /// Never retried.
    #[error("stripe rejected request ({status}): {}", .error.message.as_deref().unwrap_or(&.error.kind))]
    Api {
        status: u16,
        request_id: Option<String>,
        error: Box<StripeApiError>,
    },
    /// Rate limited, 5xx, timeout or connection failure. Safe to retry with
    /// the same idempotency key.
    #[error("stripe unavailable: {0}")]
    Unavailable(String),
    /// The response did not match the type we expected.
    #[error("unexpected stripe response: {0}")]
    Decode(String),
}

impl StripeError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, StripeError::Unavailable(_))
    }
}
