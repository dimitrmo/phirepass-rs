use crate::connection::{NodeConnection, WebConnection};
use crate::env;
use crate::env::Env;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, Method};
use axum::response::IntoResponse;
use dashmap::DashMap;
use phirepass_common::stats::Stats;
use serde_json::json;
use std::sync::Arc;
use std::time::SystemTime;
use tower_http::cors::{Any, CorsLayer};
use ulid::Ulid;

/// Composite key for tunnel sessions: (node_id, session_id)
/// This avoids string formatting on every tunnel operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TunnelSessionKey {
    pub node_id: Ulid,
    pub sid: u32,
}

impl TunnelSessionKey {
    pub fn new(node_id: Ulid, sid: u32) -> Self {
        Self { node_id, sid }
    }
}

pub type Nodes = Arc<DashMap<Ulid, NodeConnection>>;

pub type Connections = Arc<DashMap<Ulid, WebConnection>>;

pub type TunnelSessions = Arc<DashMap<TunnelSessionKey, (Ulid, Ulid)>>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) env: Arc<Env>,
    pub(crate) nodes: Nodes,
    pub(crate) connections: Connections,
    pub(crate) tunnel_sessions: TunnelSessions,
}

pub fn build_cors(state: &AppState) -> CorsLayer {
    let mut cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);

    if !state.env.mode.is_production() {
        cors = cors.allow_origin(Any);
    } else if let Some(origin) = state
        .env
        .access_control_allowed_origin
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|s| HeaderValue::from_str(s).ok())
    {
        cors = cors.allow_origin(origin);
    }

    cors
}

pub async fn get_version() -> impl IntoResponse {
    Json(json!({
        "version": env::version(),
    }))
}

pub async fn get_stats(State(state): State<AppState>) -> impl IntoResponse {
    let body = match Stats::get() {
        Some(stats) => json!({
            "stats": stats,
            "nodes": state.nodes.len(),
            "connections": state.connections.len(),
        }),
        None => json!({
            "stats": {},
            "nodes": 0,
            "connections": 0,
        }),
    };

    Json(body)
}

pub async fn list_nodes(State(state): State<AppState>) -> impl IntoResponse {
    let now = SystemTime::now();

    let data: Vec<_> = state
        .nodes
        .iter()
        .map(|entry| {
            let (id, info) = entry.pair();
            json!({
                "id": id,
                "ip": info.node.ip,
                "connected_for_secs": now
                    .duration_since(info.node.connected_at)
                    .unwrap()
                    .as_secs(),
                "since_last_heartbeat_secs": now
                    .duration_since(info.node.last_heartbeat)
                    .unwrap()
                    .as_secs(),
                "stats": &info.node.last_stats,
            })
        })
        .collect();
    Json(data)
}

pub async fn list_connections(State(state): State<AppState>) -> impl IntoResponse {
    let now = SystemTime::now();

    let data: Vec<_> = state
        .connections
        .iter()
        .map(|entry| {
            let (id, info) = entry.pair();
            json!({
                "id": id,
                "ip": info.ip,
                "connected_for_secs": now
                    .duration_since(info.connected_at)
                    .unwrap()
                    .as_secs(),
                "since_last_heartbeat_secs": now
                    .duration_since(info.last_heartbeat)
                    .unwrap()
                    .as_secs(),
            })
        })
        .collect();

    Json(data)
}
