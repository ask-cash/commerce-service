//! commerce-service: payment infrastructure for Cash, backed by Stripe.
//!
//! One binary, three commands, so the server and worker ship as one image:
//! - `server`: public API (`/v1`, webhooks) + admin port
//! - `worker`: background jobs + admin port
//! - `migrate`: apply database migrations, then exit (a Helm pre-upgrade Job)

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use commerce_api::{AdminState, AppState, Authenticator};
use commerce_store::{Db, DbConfig};
use commerce_stripe::{StripeClient, StripeConfig};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

mod config;
mod telemetry;

use config::Config;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the public API and the admin endpoints.
    Server,
    /// Run background jobs and the admin endpoints.
    Worker,
    /// Apply pending database migrations and exit.
    Migrate,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load().context("loading configuration")?;
    telemetry::init_tracing(config.log_format);
    tracing::info!(environment = ?config.environment, version = env!("CARGO_PKG_VERSION"), "starting");

    let db = Db::connect_lazy(&DbConfig {
        url: config.database.url.clone(),
        max_connections: config.database.max_connections,
    })
    .context("configuring database pool")?;

    let result = match cli.command {
        Command::Migrate => db.migrate().await.context("running migrations"),
        Command::Server => run_server(&config, db.clone()).await,
        Command::Worker => run_worker(&config, db.clone()).await,
    };
    db.close().await;
    result
}

async fn run_server(config: &Config, db: Db) -> anyhow::Result<()> {
    let metrics = telemetry::init_metrics()?;
    let stripe = StripeClient::new(StripeConfig {
        secret_key: config.stripe.secret_key.clone(),
        api_version: config.stripe.api_version.clone(),
        base_url: None,
        timeout: Duration::from_secs(config.stripe.timeout_secs),
    })?;
    let auth = Authenticator::new(&config.auth).context("loading auth keys")?;
    let state = AppState {
        db: db.clone(),
        stripe,
        webhook_secrets: config.stripe.webhook_secrets.clone().into(),
        auth: Arc::new(auth),
        request_timeout: config.request_timeout(),
    };

    let shutdown = shutdown_token();
    let public = serve(
        "public",
        config.http.addr,
        commerce_api::public_router(state),
        shutdown.clone(),
    );
    let admin = serve(
        "admin",
        config.http.admin_addr,
        commerce_api::admin_router(AdminState { db, metrics }),
        shutdown.clone(),
    );
    let grace = config.shutdown_grace();
    tokio::select! {
        result = async { tokio::try_join!(public, admin) } => { result?; }
        () = async { shutdown.cancelled().await; tokio::time::sleep(grace).await } => {
            tracing::warn!("in-flight requests did not finish within the grace period");
        }
    }
    Ok(())
}

async fn run_worker(config: &Config, db: Db) -> anyhow::Result<()> {
    let metrics = telemetry::init_metrics()?;
    let shutdown = shutdown_token();
    let admin = serve(
        "admin",
        config.http.admin_addr,
        commerce_api::admin_router(AdminState {
            db: db.clone(),
            metrics,
        }),
        shutdown.clone(),
    );
    let jobs = commerce_worker::run(commerce_worker::default_jobs(db), shutdown.clone());
    let (admin, ()) = tokio::join!(admin, jobs);
    admin
}

async fn serve(
    name: &'static str,
    addr: std::net::SocketAddr,
    router: axum::Router,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {name} listener on {addr}"))?;
    tracing::info!(listener = name, %addr, "listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { shutdown.cancelled().await })
        .await
        .with_context(|| format!("{name} server"))
}

/// Cancelled on SIGTERM (Kubernetes) or Ctrl-C (local).
fn shutdown_token() -> CancellationToken {
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        let ctrl_c = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        #[cfg(unix)]
        let terminate = async {
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(mut sig) => {
                    sig.recv().await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "cannot listen for SIGTERM");
                    std::future::pending::<()>().await;
                }
            }
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        tokio::select! {
            () = ctrl_c => {}
            () = terminate => {}
        }
        tracing::info!("shutdown signal received");
        trigger.cancel();
    });
    token
}
