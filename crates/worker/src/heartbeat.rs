use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use commerce_store::Db;

use crate::Job;

/// Proves the worker loop is alive and can reach the database. Alert when
/// `worker_heartbeat_timestamp_seconds` stops advancing.
pub struct Heartbeat {
    db: Db,
}

impl Heartbeat {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl Job for Heartbeat {
    fn name(&self) -> &'static str {
        "heartbeat"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(15)
    }

    async fn run(&self) -> anyhow::Result<()> {
        self.db.ready().await?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        #[allow(clippy::cast_precision_loss)]
        metrics::gauge!("worker_heartbeat_timestamp_seconds").set(now as f64);
        Ok(())
    }
}
