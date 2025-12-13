use crate::env::Env;
use crate::ws;
use log::{info, warn};
use phirepass_common::stats::Stats;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;

pub(crate) async fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let stats_refresh_interval = config.stats_refresh_interval;
    let (shutdown_tx, _) = broadcast::channel(1);

    let ws_task = start_ws_connection(config, shutdown_tx.subscribe());
    let stats_task = spawn_stats_logger(stats_refresh_interval, shutdown_tx.subscribe());

    let shutdown_signal = async {
        if let Err(err) = signal::ctrl_c().await {
            warn!("failed to listen for shutdown signal: {}", err);
        } else {
            info!("ctrl+c pressed, shutting down");
        }
    };

    tokio::select! {
        _ = ws_task => warn!("ws task ended"),
        _ = stats_task => warn!("stats logger task ended"),
        _ = shutdown_signal => info!("shutdown signal received"),
    }

    let _ = shutdown_tx.send(());

    Ok(())
}

fn start_ws_connection(
    config: Env,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let env = Arc::new(config);

    tokio::spawn(async move {
        let mut attempt: u32 = 0;

        loop {
            let conn = ws::WSConnection::new();

            tokio::select! {
                res = conn.connect(env.clone()) => {
                    match res {
                        Ok(()) => warn!("ws connection ended, attempting reconnect"),
                        Err(err) => warn!("ws client error: {err}, attempting reconnect"),
                    }
                }
                _ = shutdown.recv() => {
                    info!("ws connection shutting down");
                    break;
                }
            }

            attempt = attempt.saturating_add(1);
            let backoff_secs = 2u64.saturating_pow(attempt.min(4));
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {},
                _ = shutdown.recv() => {
                    info!("ws connection shutting down");
                    break;
                }
            }
        }
    })
}

fn spawn_stats_logger(
    stats_refresh_interval: u16,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(stats_refresh_interval as u64));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match Stats::gather() {
                        Some(stats) => info!("daemon stats\n{}", stats.log_line()),
                        None => warn!("stats: unable to read process metrics"),
                    }
                }
                _ = shutdown.recv() => {
                    info!("stats logger shutting down");
                    break;
                }
            }
        }
    })
}
