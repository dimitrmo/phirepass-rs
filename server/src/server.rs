use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::env::Env;
use crate::http::{get_stats, get_version};
use crate::node::ws_node_handler;
use crate::state::AppState;
use crate::web::ws_web_handler;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, header};
use axum::routing::get;
use axum::{Json, Router};
use log::{info, warn};
use phirepass_common::stats::Stats;
use serde::Serialize;
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

        let app = Router::new()
            .route("/web/ws", get(ws_web_handler))
            .route("/nodes/ws", get(ws_node_handler))
            .route("/nodes", get(list_nodes))
            .route("/stats", get(get_stats))
            .route("/version", get(get_version))
            .layer(ip_source.into_extension())
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

#[derive(Serialize)]
struct NodeSummary {
    id: String,
    ip: String,
    connected_for_secs: u64,
    since_last_heartbeat_secs: u64,
    stats: Option<Stats>,
}

async fn list_nodes(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let nodes = state.nodes.read().await;
    let now = SystemTime::now();

    let data: Vec<NodeSummary> = nodes
        .iter()
        .map(|(id, info)| NodeSummary {
            id: id.to_string(),
            ip: info.node.ip.to_string(),
            connected_for_secs: now
                .duration_since(info.node.connected_at)
                .unwrap()
                .as_secs(),
            since_last_heartbeat_secs: now
                .duration_since(info.node.last_heartbeat)
                .unwrap()
                .as_secs(),
            stats: info.node.last_stats.clone(),
        })
        .collect();

    let mut headers = HeaderMap::new();
    if !state.env.mode.is_production() {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
    }

    (headers, Json(data))
}
