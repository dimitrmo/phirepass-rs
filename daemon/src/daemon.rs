use crate::env::Env;
use crate::http::{AppState, get_version};
use crate::ws;
use axum::Router;
use axum::routing::get;
use log::{info, warn};
use phirepass_common::stats::Stats;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;

pub(crate) async fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let stats_refresh_interval = config.stats_refresh_interval;
    let (shutdown_tx, _) = broadcast::channel(1);

    let state = AppState::new(Arc::new(config));
    let ws_task = start_ws_connection(&state, shutdown_tx.subscribe());
    let http_task = start_http_server(state, shutdown_tx.subscribe());
    let stats_task = spawn_stats_logger(stats_refresh_interval as u64, shutdown_tx.subscribe());

    let shutdown_signal = async {
        if let Err(err) = signal::ctrl_c().await {
            warn!("failed to listen for shutdown signal: {}", err);
        } else {
            info!("ctrl+c pressed, shutting down");
        }
    };

    tokio::select! {
        _ = ws_task => warn!("ws task ended"),
        _ = http_task => warn!("http task ended"),
        _ = stats_task => warn!("stats logger task ended"),
        _ = shutdown_signal => info!("shutdown signal received"),
    }

    let _ = shutdown_tx.send(());

    info!("waiting for tasks to shut down gracefully...");
    tokio::time::sleep(Duration::from_millis(500)).await;

    Ok(())
}

fn start_http_server(
    state: AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let host = format!("{}:{}", state.env.host, state.env.port);

    tokio::spawn(async move {
        let app = Router::new()
            .route("/version", get(get_version))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(host).await.unwrap();
        info!("listening on: {}", listener.local_addr().unwrap());

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown.recv().await;
        })
        .await
        .unwrap();
    })
}

fn start_ws_connection(
    state: &AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let env = Arc::clone(&state.env);
    tokio::spawn(async move {
        let mut attempt: u32 = 0;
        let stored_node_id = Arc::new(tokio::sync::RwLock::new(None));

        loop {
            let conn = ws::WebSocketConnection::new(stored_node_id.clone());

            tokio::select! {
                res = conn.connect(Arc::clone(&env)) => {
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
    stats_refresh_interval: u64,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(stats_refresh_interval));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match Stats::refresh() {
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
