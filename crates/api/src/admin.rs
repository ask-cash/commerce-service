use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use commerce_store::Db;
use metrics_exporter_prometheus::PrometheusHandle;

/// State for the admin port, served by both the server and worker processes.
#[derive(Clone)]
pub struct AdminState {
    pub db: Db,
    pub metrics: PrometheusHandle,
}

pub fn admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .with_state(state)
}

/// Liveness: the process is up. Deliberately checks nothing external.
async fn healthz() -> &'static str {
    "ok"
}

/// Readiness: Postgres is reachable and fully migrated. Stripe is not
/// checked, so a Stripe outage never takes pods out of rotation.
async fn readyz(State(state): State<AdminState>) -> impl IntoResponse {
    match state.db.ready().await {
        Ok(()) => (StatusCode::OK, "ready".to_owned()),
        Err(e) => {
            tracing::warn!(error = %e, "not ready");
            (StatusCode::SERVICE_UNAVAILABLE, "not ready".to_owned())
        }
    }
}

async fn metrics(State(state): State<AdminState>) -> String {
    state.metrics.render()
}
