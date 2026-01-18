use crate::connection::{NodeConnection, WebConnection};
use crate::env;
use crate::env::Env;
use crate::error::ServerError;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, Method};
use axum::response::IntoResponse;
use dashmap::DashMap;
use log::debug;
use phirepass_common::protocol::web::WebFrameData;
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

impl AppState {
    pub async fn get_node_id_by_cid_and_sid(
        &self,
        cid: &Ulid,
        node_id: String,
        sid: u32,
    ) -> Result<Ulid, ServerError> {
        debug!("get_node_id_by_cid_and_sid [cid={cid}, sid={sid}, node_id={node_id})]");

        let node_ulid = Ulid::from_string(&node_id).map_err(|_| {
            return format!("failed to decode ulid {}", node_id);
        })?;

        let key = TunnelSessionKey::new(node_ulid, sid);

        let (client_id, node_id) = match self.tunnel_sessions.get(&key) {
            Some(entry) => {
                let (cid, nid) = entry.value();
                (*cid, *nid)
            }
            _ => return Err(format!("node not found for session id {sid}").into()),
        };

        if !client_id.eq(cid) {
            return Err(format!("correct cid was not found for sid {sid}").into());
        }

        Ok(node_id)
    }

    pub async fn get_connection_id_by_sid(
        &self,
        sid: u32,
        target: Ulid,
    ) -> Result<Ulid, ServerError> {
        let key = TunnelSessionKey::new(target, sid);
        let (client_id, node_id) = match self.tunnel_sessions.get(&key) {
            Some(entry) => {
                let (cid, nid) = entry.value();
                (*cid, *nid)
            }
            _ => {
                return Err(format!("node not found for session id {sid}").into());
            }
        };

        if !node_id.eq(&target) {
            return Err(format!("correct node_id was not found for sid {sid}").into());
        }

        Ok(client_id)
    }

    pub async fn notify_client_by_cid(
        &self,
        cid: Ulid,
        frame: WebFrameData,
    ) -> Result<(), ServerError> {
        debug!("notify_client_by_cid {cid}");
        debug!("\tdata: {:?}", frame);

        let Some(connection) = self.connections.get(&cid) else {
            return Err(format!("connection {cid} not found").into());
        };

        connection.tx.send(frame).await.map_err(|err| {
            return format!("failed to send frame to client {cid}: {err}");
        })?;

        debug!("frame sent to web client {cid}");

        Ok(())
    }
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
