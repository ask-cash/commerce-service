//! Configuration, loaded once at startup.
//!
//! Sources, later ones winning: an optional TOML file named by
//! `COMMERCE_CONFIG`, then environment variables prefixed `COMMERCE_` with
//! `__` separating sections, e.g. `COMMERCE_DATABASE__URL`.

use std::net::SocketAddr;
use std::time::Duration;

use commerce_api::AuthConfig;
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Development,
    Test,
    Staging,
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub environment: Environment,
    #[serde(default = "default_log_format")]
    pub log_format: LogFormat,
    #[serde(default)]
    pub http: HttpConfig,
    pub database: DatabaseConfig,
    pub stripe: StripeConfig,
    pub auth: AuthConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    pub addr: SocketAddr,
    pub admin_addr: SocketAddr,
    pub request_timeout_secs: u64,
    pub shutdown_grace_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: SecretString,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Deserialize)]
pub struct StripeConfig {
    pub secret_key: SecretString,
    /// Pinned API version, e.g. "2025-09-30.clover". Upgrade deliberately.
    pub api_version: String,
    /// Endpoint signing secrets (`whsec_...`). Two during rotation.
    #[serde(default)]
    pub webhook_secrets: Vec<SecretString>,
    #[serde(default = "default_stripe_timeout_secs")]
    pub timeout_secs: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(transparent)]
    Load(#[from] Box<figment::Error>),
    #[error("live Stripe key used outside production")]
    LiveKeyOutsideProduction,
    #[error("test Stripe key used in production")]
    TestKeyInProduction,
    #[error("stripe.secret_key must start with sk_ or rk_")]
    UnrecognisedStripeKey,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            addr: SocketAddr::from(([0, 0, 0, 0], 8080)),
            admin_addr: SocketAddr::from(([0, 0, 0, 0], 9090)),
            request_timeout_secs: 10,
            shutdown_grace_secs: 20,
        }
    }
}

fn default_log_format() -> LogFormat {
    LogFormat::Json
}
fn default_max_connections() -> u32 {
    10
}
fn default_stripe_timeout_secs() -> u64 {
    8
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let mut figment = Figment::new();
        if let Ok(path) = std::env::var("COMMERCE_CONFIG") {
            figment = figment.merge(Toml::file_exact(path));
        }
        figment = figment.merge(Env::prefixed("COMMERCE_").split("__").ignore(&["config"]));
        Self::from_figment(&figment)
    }

    fn from_figment(figment: &Figment) -> Result<Self, ConfigError> {
        let config: Config = figment.extract().map_err(Box::new)?;
        config.validate()?;
        Ok(config)
    }

    /// Refuses to start with a key that doesn't match the environment, so a
    /// staging deploy can never charge real cards and production can never
    /// silently run in test mode.
    fn validate(&self) -> Result<(), ConfigError> {
        let key = self.stripe.secret_key.expose_secret();
        let live = key.starts_with("sk_live_") || key.starts_with("rk_live_");
        let test = key.starts_with("sk_test_") || key.starts_with("rk_test_");
        match (live, test, self.environment) {
            (false, false, _) => Err(ConfigError::UnrecognisedStripeKey),
            (true, _, env) if env != Environment::Production => Err(ConfigError::LiveKeyOutsideProduction),
            (_, true, Environment::Production) => Err(ConfigError::TestKeyInProduction),
            _ => Ok(()),
        }
    }

    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.http.request_timeout_secs)
    }

    pub fn shutdown_grace(&self) -> Duration {
        Duration::from_secs(self.http.shutdown_grace_secs)
    }
}

#[cfg(test)]
// figment's Jail closures return its large `figment::Error`; not ours to shrink.
#[allow(clippy::result_large_err)]
mod tests {
    use super::*;
    use figment::Jail;

    fn base_env(jail: &mut Jail, environment: &str, key: &str) {
        jail.set_env("COMMERCE_ENVIRONMENT", environment);
        jail.set_env("COMMERCE_DATABASE__URL", "postgres://localhost/commerce");
        jail.set_env("COMMERCE_STRIPE__SECRET_KEY", key);
        jail.set_env("COMMERCE_STRIPE__API_VERSION", "2025-09-30.clover");
        jail.set_env("COMMERCE_STRIPE__WEBHOOK_SECRETS", r#"["whsec_a","whsec_b"]"#);
        jail.set_env("COMMERCE_AUTH__ISSUER", "cash");
        jail.set_env("COMMERCE_AUTH__AUDIENCE", "commerce-service");
        jail.set_env("COMMERCE_AUTH__PUBLIC_KEYS", r#"[{kid="cash-1",x="abc"}]"#);
    }

    fn load() -> Result<Config, ConfigError> {
        Config::from_figment(&Figment::new().merge(Env::prefixed("COMMERCE_").split("__")))
    }

    #[test]
    fn loads_from_env() {
        Jail::expect_with(|jail| {
            base_env(jail, "development", "sk_test_123");
            jail.set_env("COMMERCE_HTTP__ADDR", "127.0.0.1:3000");
            let config = load().map_err(|e| e.to_string())?;
            assert_eq!(config.environment, Environment::Development);
            assert_eq!(config.http.addr.port(), 3000);
            assert_eq!(config.http.admin_addr.port(), 9090);
            assert_eq!(config.stripe.webhook_secrets.len(), 2);
            assert_eq!(config.auth.public_keys[0].kid, "cash-1");
            Ok(())
        });
    }

    #[test]
    fn rejects_live_key_outside_production() {
        Jail::expect_with(|jail| {
            base_env(jail, "staging", "sk_live_123");
            assert!(matches!(load(), Err(ConfigError::LiveKeyOutsideProduction)));
            Ok(())
        });
    }

    #[test]
    fn rejects_test_key_in_production() {
        Jail::expect_with(|jail| {
            base_env(jail, "production", "rk_test_123");
            assert!(matches!(load(), Err(ConfigError::TestKeyInProduction)));
            Ok(())
        });
    }

    #[test]
    fn rejects_unknown_key_format() {
        Jail::expect_with(|jail| {
            base_env(jail, "development", "pk_test_123");
            assert!(matches!(load(), Err(ConfigError::UnrecognisedStripeKey)));
            Ok(())
        });
    }
}
