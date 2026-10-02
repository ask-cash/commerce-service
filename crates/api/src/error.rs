use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use commerce_app::AppError;
use serde::Serialize;

/// Stable, machine-readable error codes. Cash branches on these, so treat
/// adding a variant as an API change and never rename one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    Unauthenticated,
    Forbidden,
    NotFound,
    Conflict,
    IdempotencyKeyReused,
    ProviderRejected,
    ProviderUnavailable,
    Timeout,
    Internal,
}

/// Every error response has the same shape:
/// `{"error": {"code": "...", "message": "..."}}`. The request ID is in the
/// `x-request-id` response header.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthenticated(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, ErrorCode::Unauthenticated, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, ErrorCode::Forbidden, message)
    }
}

#[derive(Serialize)]
struct Body<'a> {
    error: Inner<'a>,
}

#[derive(Serialize)]
struct Inner<'a> {
    code: ErrorCode,
    message: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Body {
            error: Inner {
                code: self.code,
                message: &self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        match &e {
            AppError::InvalidInput(_) => Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::InvalidRequest,
                e.to_string(),
            ),
            AppError::NotFound(_) => Self::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, e.to_string()),
            AppError::Conflict(_) => Self::new(StatusCode::CONFLICT, ErrorCode::Conflict, e.to_string()),
            AppError::ProviderRejected(_) => {
                Self::new(StatusCode::PAYMENT_REQUIRED, ErrorCode::ProviderRejected, e.to_string())
            }
            AppError::ProviderUnavailable(source) => {
                tracing::warn!(error = %source, "payment provider unavailable");
                Self::new(
                    StatusCode::SERVICE_UNAVAILABLE,
                    ErrorCode::ProviderUnavailable,
                    e.to_string(),
                )
            }
            AppError::Internal(source) => {
                // Log the cause; never leak it to the caller.
                tracing::error!(error = %source, "internal error");
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal, "internal error")
            }
        }
    }
}
