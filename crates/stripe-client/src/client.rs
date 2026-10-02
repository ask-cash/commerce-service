use std::time::Duration;

use reqwest::{Method, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{StripeApiError, StripeError};

const DEFAULT_BASE_URL: &str = "https://api.stripe.com";
/// Reads are retried on transient failures; writes rely on the caller
/// retrying with the same idempotency key (usually via the job queue).
const MAX_READ_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub struct StripeConfig {
    pub secret_key: SecretString,
    /// Pinned API version, sent as `Stripe-Version` on every request.
    pub api_version: String,
    pub base_url: Option<String>,
    pub timeout: Duration,
}

#[derive(Clone)]
pub struct StripeClient {
    http: reqwest::Client,
    base_url: String,
    secret_key: SecretString,
    api_version: String,
}

impl StripeClient {
    pub fn new(config: StripeConfig) -> Result<Self, StripeError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .connect_timeout(Duration::from_secs(3))
            .user_agent(concat!("commerce-service/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| StripeError::Unavailable(e.to_string()))?;
        Ok(Self {
            http,
            base_url: config.base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
            secret_key: config.secret_key,
            api_version: config.api_version,
        })
    }

    /// GET with bounded retries on transient errors.
    pub async fn get<Q, T>(&self, path: &str, query: &Q) -> Result<T, StripeError>
    where
        Q: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let req = self.request(Method::GET, path).query(query);
            match self.send(req).await {
                Err(e) if e.is_retryable() && attempt < MAX_READ_ATTEMPTS => {
                    tracing::warn!(path, attempt, error = %e, "retrying stripe read");
                    tokio::time::sleep(backoff(attempt)).await;
                }
                other => return other,
            }
        }
    }

    /// POST (form-encoded, as Stripe expects). The idempotency key is
    /// mandatory: derive it from our own record ID so a retry of the same
    /// operation can never create a second object in Stripe.
    pub async fn post<B, T>(&self, path: &str, idempotency_key: &str, body: &B) -> Result<T, StripeError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let req = self
            .request(Method::POST, path)
            .header("Idempotency-Key", idempotency_key)
            .form(body);
        self.send(req).await
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(self.secret_key.expose_secret())
            .header("Stripe-Version", &self.api_version)
    }

    async fn send<T: DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> Result<T, StripeError> {
        let resp = req.send().await.map_err(|e| StripeError::Unavailable(e.to_string()))?;
        let status = resp.status();
        let request_id = resp
            .headers()
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| StripeError::Unavailable(e.to_string()))?;

        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|e| StripeError::Decode(e.to_string()));
        }
        if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(StripeError::Unavailable(format!(
                "status {status}, request-id {request_id:?}"
            )));
        }

        #[derive(serde::Deserialize)]
        struct Envelope {
            error: StripeApiError,
        }
        let error = serde_json::from_slice::<Envelope>(&bytes)
            .map(|e| e.error)
            .map_err(|e| StripeError::Decode(e.to_string()))?;
        Err(StripeError::Api {
            status: status.as_u16(),
            request_id,
            error: Box::new(error),
        })
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(200 * 2u64.pow(attempt.saturating_sub(1)))
}
