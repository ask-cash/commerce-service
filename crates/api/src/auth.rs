//! Service-to-service authentication.
//!
//! Cash signs short-lived JWTs with an Ed25519 private key; we hold only the
//! public keys, selected by the token's `kid` so keys can be rotated without
//! downtime. Tokens must carry our audience, Cash's issuer, and a lifetime of
//! at most [`MAX_TOKEN_LIFETIME_SECS`].

use std::collections::{HashMap, HashSet};

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;

pub const MAX_TOKEN_LIFETIME_SECS: u64 = 600;
const CLOCK_LEEWAY_SECS: u64 = 30;

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub issuer: String,
    pub audience: String,
    pub public_keys: Vec<PublicKey>,
}

/// An Ed25519 public key in JWK form: `x` is the base64url raw key.
#[derive(Debug, Clone, Deserialize)]
pub struct PublicKey {
    pub kid: String,
    pub x: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthConfigError {
    #[error("no public keys configured")]
    NoKeys,
    #[error("invalid public key {kid}: {source}")]
    InvalidKey {
        kid: String,
        source: jsonwebtoken::errors::Error,
    },
}

pub struct Authenticator {
    keys: HashMap<String, DecodingKey>,
    validation: Validation,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    iat: u64,
    exp: u64,
    #[serde(default)]
    scope: String,
}

/// The authenticated calling service. Add it as a handler argument to
/// require authentication; call [`Caller::require`] for a scope.
#[derive(Debug, Clone)]
pub struct Caller {
    pub subject: String,
    scopes: HashSet<String>,
}

impl Caller {
    pub fn require(&self, scope: &str) -> Result<(), ApiError> {
        if self.scopes.contains(scope) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!("missing scope {scope}")))
        }
    }

    pub fn scopes(&self) -> impl Iterator<Item = &str> {
        self.scopes.iter().map(String::as_str)
    }
}

impl Authenticator {
    pub fn new(config: &AuthConfig) -> Result<Self, AuthConfigError> {
        if config.public_keys.is_empty() {
            return Err(AuthConfigError::NoKeys);
        }
        let mut keys = HashMap::new();
        for key in &config.public_keys {
            let decoding = DecodingKey::from_ed_components(&key.x).map_err(|source| AuthConfigError::InvalidKey {
                kid: key.kid.clone(),
                source,
            })?;
            keys.insert(key.kid.clone(), decoding);
        }

        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[&config.issuer]);
        validation.set_audience(&[&config.audience]);
        validation.set_required_spec_claims(&["exp", "iat", "iss", "aud", "sub"]);
        validation.leeway = CLOCK_LEEWAY_SECS;
        Ok(Self { keys, validation })
    }

    pub fn verify(&self, token: &str) -> Result<Caller, ApiError> {
        let header = decode_header(token).map_err(|_| ApiError::unauthenticated("malformed token"))?;
        let kid = header
            .kid
            .ok_or_else(|| ApiError::unauthenticated("token has no kid"))?;
        let key = self
            .keys
            .get(&kid)
            .ok_or_else(|| ApiError::unauthenticated("unknown signing key"))?;

        let claims = decode::<Claims>(token, key, &self.validation)
            .map_err(|e| {
                tracing::debug!(error = %e, "token rejected");
                ApiError::unauthenticated("invalid token")
            })?
            .claims;
        if claims.exp.saturating_sub(claims.iat) > MAX_TOKEN_LIFETIME_SECS {
            return Err(ApiError::unauthenticated("token lifetime too long"));
        }

        Ok(Caller {
            subject: claims.sub,
            scopes: claims.scope.split_whitespace().map(str::to_owned).collect(),
        })
    }
}

impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError::unauthenticated("missing bearer token"))?;
        state.auth.verify(token)
    }
}
