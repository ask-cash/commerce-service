//! Postgres adapter. Owns the connection pool and migrations; each feature
//! adds a repository module implementing the ports defined in `commerce-app`.

use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

pub use sqlx::migrate::MigrateError;

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct DbConfig {
    pub url: SecretString,
    pub max_connections: u32,
}

impl Db {
    /// Builds the pool lazily so the process can start (and report not-ready)
    /// while the database is still coming up.
    pub fn connect_lazy(config: &DbConfig) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy(config.url.expose_secret())?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<(), MigrateError> {
        sqlx::migrate!("../../migrations").run(&self.pool).await
    }

    /// Readiness: the database answers and every migration in this binary
    /// has been applied.
    pub async fn ready(&self) -> Result<(), sqlx::Error> {
        let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
            .fetch_one(&self.pool)
            .await?;
        let expected = sqlx::migrate!("../../migrations").iter().count() as i64;
        if applied < expected {
            return Err(sqlx::Error::Protocol(format!(
                "{applied} of {expected} migrations applied"
            )));
        }
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
