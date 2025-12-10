use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::env::Env;
use crate::http::{get_alive, get_ready};
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

pub async fn start(config: Env) -> anyhow::Result<()> {
    let stats_refresh_interval = config.stats_refresh_interval;

    let http_task = start_http_server(config);
    let stats_task = spawn_stats_logger(stats_refresh_interval);

    tokio::select! {
        _ = http_task => warn!("http task ended"),
        _ = stats_task => warn!("stats logger task ended"),
    }

    Ok(())
}

fn start_http_server(config: Env) -> tokio::task::JoinHandle<()> {
    let host = format!("{}:{}", config.host, config.port);

    tokio::spawn(async move {
        let state = AppState {
            env: Arc::new(config),
            nodes: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            clients: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        };

        let app = Router::new()
            .route("/web/ws", get(ws_web_handler))
            .route("/nodes/ws", get(ws_node_handler))
            .route("/nodes", get(list_nodes))
            .route("/ready", get(get_ready))
            .route("/alive", get(get_alive))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(host).await.unwrap();
        info!("listening on: {}", listener.local_addr().unwrap());

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    })
}

fn spawn_stats_logger(stats_refresh_interval: u16) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(stats_refresh_interval as u64));
        loop {
            interval.tick().await;
            match Stats::gather() {
                Some(stats) => info!("{}", stats.log_line()),
                None => warn!("Stats: unable to read process metrics"),
            }
        }
    })
}

#[derive(Serialize)]
struct NodeSummary {
    id: String,
    addr: String,
    connected_for_secs: f64,
    since_last_heartbeat_secs: f64,
    stats: Option<Stats>,
}

async fn list_nodes(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let nodes = state.nodes.lock().await;
    let now = std::time::Instant::now();

    let data: Vec<NodeSummary> = nodes
        .iter()
        .map(|(id, info)| NodeSummary {
            id: id.to_string(),
            addr: info.addr.to_string(),
            connected_for_secs: now.duration_since(info.connected_at).as_secs_f64(),
            since_last_heartbeat_secs: now.duration_since(info.last_heartbeat).as_secs_f64(),
            stats: info.last_stats.clone(),
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
