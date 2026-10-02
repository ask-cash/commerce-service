use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, StatusCode, header};
use axum::routing::get;
use tower::ServiceBuilder;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::sensitive_headers::SetSensitiveRequestHeadersLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::middleware::track_metrics;
use crate::state::AppState;

mod caller;

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
const MAX_BODY_BYTES: usize = 256 * 1024;

/// The router served on the public port. Each feature adds its routes under
/// `/v1`; the Stripe webhook route is added alongside.
pub fn public_router(state: AppState) -> Router {
    let timeout = state.request_timeout;
    let v1 = Router::new().route("/caller", get(caller::get));

    Router::new()
        .nest("/v1", v1)
        .route_layer(axum::middleware::from_fn(track_metrics))
        .layer(
            // Outermost first: id → sensitive headers → trace → panic → timeout.
            ServiceBuilder::new()
                .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuid))
                .layer(PropagateRequestIdLayer::new(REQUEST_ID))
                .layer(SetSensitiveRequestHeadersLayer::new([
                    header::AUTHORIZATION,
                    HeaderName::from_static("stripe-signature"),
                ]))
                .layer(
                    TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
                        let request_id = req
                            .headers()
                            .get(REQUEST_ID)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default();
                        tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id)
                    }),
                )
                .layer(CatchPanicLayer::new())
                .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, timeout))
                .layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
        )
        .with_state(state)
}
