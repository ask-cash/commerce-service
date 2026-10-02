use axum::Json;
use serde::Serialize;

use crate::auth::Caller;

#[derive(Serialize)]
pub struct CallerResponse {
    subject: String,
    scopes: Vec<String>,
}

/// `GET /v1/caller`: who the token says you are. Lets Cash verify its token
/// setup end to end before any billing call.
pub async fn get(caller: Caller) -> Json<CallerResponse> {
    let mut scopes: Vec<String> = caller.scopes().map(str::to_owned).collect();
    scopes.sort();
    Json(CallerResponse {
        subject: caller.subject,
        scopes,
    })
}
