//! Background job runner.
//!
//! Each job runs on its own fixed interval until shutdown. Jobs claim work
//! from Postgres with `FOR UPDATE SKIP LOCKED`, so any number of worker
//! replicas can run side by side. Features register their jobs in
//! [`default_jobs`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use commerce_store::Db;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

mod heartbeat;

pub use heartbeat::Heartbeat;

#[async_trait]
pub trait Job: Send + Sync {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration;
    /// One pass over due work. Errors are logged and counted; the job runs
    /// again on its next tick.
    async fn run(&self) -> anyhow::Result<()>;
}

pub fn default_jobs(db: Db) -> Vec<Arc<dyn Job>> {
    vec![Arc::new(Heartbeat::new(db))]
}

/// Runs every job until `shutdown` is cancelled, then waits for in-flight
/// passes to finish.
pub async fn run(jobs: Vec<Arc<dyn Job>>, shutdown: CancellationToken) {
    let mut set = JoinSet::new();
    for job in jobs {
        let shutdown = shutdown.clone();
        set.spawn(async move { run_job(job, shutdown).await });
    }
    while set.join_next().await.is_some() {}
    tracing::info!("worker stopped");
}

async fn run_job(job: Arc<dyn Job>, shutdown: CancellationToken) {
    let name = job.name();
    let mut ticker = tokio::time::interval(job.interval());
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tracing::info!(job = name, interval = ?job.interval(), "job started");

    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = ticker.tick() => {}
        }
        let started = Instant::now();
        let result = job.run().await;
        metrics::histogram!("worker_job_duration_seconds", "job" => name).record(started.elapsed().as_secs_f64());
        if let Err(error) = result {
            metrics::counter!("worker_job_failures_total", "job" => name).increment(1);
            tracing::error!(job = name, error = format!("{error:#}"), "job failed");
        }
    }
}
