#![allow(clippy::expect_used, clippy::unwrap_used)]
//! End-to-end tests of the public router's authentication, using a
//! test-only Ed25519 key. No database or network is touched.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use commerce_api::{AppState, AuthConfig, Authenticator, PublicKey, public_router};
use commerce_store::{Db, DbConfig};
use commerce_stripe::{StripeClient, StripeConfig};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use secrecy::SecretString;
use serde_json::{Value, json};
use tower::ServiceExt;

// Test-only key pair. Never used anywhere but these tests.
const PRIVATE_PEM: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIDIFx0lAs0gnciuec74TizNNqXxaXi0a7SW9yis35bPc
-----END PRIVATE KEY-----";
const PUBLIC_X: &str = "686F8RNCBWbHlv_sI31S9AqrJ-ks4CaQAyMf_jPJNtE";
const KID: &str = "cash-test-1";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn app() -> axum::Router {
    let auth = Authenticator::new(&AuthConfig {
        issuer: "cash".into(),
        audience: "commerce-service".into(),
        public_keys: vec![PublicKey {
            kid: KID.into(),
            x: PUBLIC_X.into(),
        }],
    })
    .expect("valid auth config");
    let db = Db::connect_lazy(&DbConfig {
        url: SecretString::from("postgres://unused@localhost/unused"),
        max_connections: 1,
    })
    .expect("lazy pool");
    let stripe = StripeClient::new(StripeConfig {
        secret_key: SecretString::from("sk_test_unused"),
        api_version: "2025-09-30.clover".into(),
        base_url: Some("http://127.0.0.1:9".into()),
        timeout: Duration::from_secs(1),
    })
    .expect("stripe client");
    public_router(AppState {
        db,
        stripe,
        webhook_secrets: Arc::from([]),
        auth: Arc::new(auth),
        request_timeout: Duration::from_secs(5),
    })
}

fn token(kid: Option<&str>, claims: Value) -> String {
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = kid.map(str::to_owned);
    let key = EncodingKey::from_ed_pem(PRIVATE_PEM.as_bytes()).expect("test key");
    encode(&header, &claims, &key).expect("sign")
}

fn claims(overrides: Value) -> Value {
    let t = now();
    let mut c = json!({
        "iss": "cash", "aud": "commerce-service", "sub": "cash-gateway",
        "iat": t, "exp": t + 300, "scope": "billing:write billing:read",
    });
    if let (Some(base), Some(extra)) = (c.as_object_mut(), overrides.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    c
}

async fn get_caller(auth: Option<String>) -> (StatusCode, Value, bool) {
    let mut req = Request::get("/v1/caller");
    if let Some(a) = auth {
        req = req.header("authorization", a);
    }
    let resp = app()
        .oneshot(req.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let has_request_id = resp.headers().contains_key("x-request-id");
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        has_request_id,
    )
}

#[tokio::test]
async fn valid_token_identifies_caller() {
    let (status, body, has_request_id) =
        get_caller(Some(format!("Bearer {}", token(Some(KID), claims(json!({})))))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"subject": "cash-gateway", "scopes": ["billing:read", "billing:write"]})
    );
    assert!(has_request_id);
}

#[tokio::test]
async fn missing_token_is_401_with_error_shape() {
    let (status, body, has_request_id) = get_caller(None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthenticated");
    assert!(has_request_id);
}

#[tokio::test]
async fn rejects_bad_tokens() {
    let t = now();
    let cases = [
        ("unknown kid", token(Some("other"), claims(json!({})))),
        ("no kid", token(None, claims(json!({})))),
        (
            "wrong audience",
            token(Some(KID), claims(json!({"aud": "someone-else"}))),
        ),
        ("wrong issuer", token(Some(KID), claims(json!({"iss": "evil"})))),
        (
            "expired",
            token(Some(KID), claims(json!({"iat": t - 900, "exp": t - 600}))),
        ),
        ("too long-lived", token(Some(KID), claims(json!({"exp": t + 3600})))),
        ("garbage", "not-a-jwt".to_owned()),
    ];
    for (name, tok) in cases {
        let (status, body, _) = get_caller(Some(format!("Bearer {tok}"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{name}");
        assert_eq!(body["error"]["code"], "unauthenticated", "{name}");
    }
}

#[tokio::test]
async fn unknown_route_is_404() {
    let resp = app()
        .oneshot(Request::get("/v1/nope").body(Body::empty()).expect("request"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
