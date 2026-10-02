use commerce_domain::{CurrencyError, MoneyError};

/// Errors a use case can return. The API layer maps each variant to one HTTP
/// status and stable error code; adapters map their own errors into these.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Conflict(String),
    #[error("payment provider unavailable")]
    ProviderUnavailable(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("payment provider rejected the request: {0}")]
    ProviderRejected(String),
    #[error("internal error")]
    Internal(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<MoneyError> for AppError {
    fn from(e: MoneyError) -> Self {
        AppError::InvalidInput(e.to_string())
    }
}

impl From<CurrencyError> for AppError {
    fn from(e: CurrencyError) -> Self {
        AppError::InvalidInput(e.to_string())
    }
}
