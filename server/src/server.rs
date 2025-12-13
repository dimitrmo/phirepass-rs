use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::env::Env;
use crate::http::{build_cors, get_stats, get_version, list_connections, list_nodes};
use crate::node::ws_node_handler;
use crate::state::AppState;
use crate::web::ws_web_handler;
use axum::Router;
use axum::routing::get;
use log::{info, warn};
use phirepass_common::stats::Stats;
use tokio::signal;
use tokio::sync::broadcast;

pub async fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let stats_refresh_interval = config.stats_refresh_interval;
    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    let http_task = start_http_server(config, shutdown_tx.subscribe());
    let stats_task = spawn_stats_logger(stats_refresh_interval, shutdown_tx.subscribe());

    let shutdown_signal = async {
        if let Err(err) = signal::ctrl_c().await {
            warn!("failed to listen for shutdown signal: {}", err);
        } else {
            info!("ctrl+c pressed, shutting down");
        }
    };

    tokio::select! {
        _ = http_task => warn!("http task ended"),
        _ = stats_task => warn!("stats logger task ended"),
        _ = shutdown_signal => info!("shutdown signal received"),
    }

    // Tell all tasks to shut down if they have not already received the signal.
    let _ = shutdown_tx.send(());

    Ok(())
}

fn start_http_server(
    config: Env,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let ip_source = config.ip_source.clone();
    let host = format!("{}:{}", config.host, config.port);

    tokio::spawn(async move {
        let state = AppState {
            env: Arc::new(config),
            nodes: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            connections: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        };

        let cors = build_cors(&state);

        let app = Router::new()
            .route("/web/ws", get(ws_web_handler))
            .route("/nodes/ws", get(ws_node_handler))
            .route("/nodes", get(list_nodes))
            .route("/connections", get(list_connections))
            .route("/stats", get(get_stats))
            .route("/version", get(get_version))
            .layer(ip_source.into_extension())
            .layer(cors)
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
                        Some(stats) => info!("server stats\n{}", stats.log_line()),
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
