use crate::env;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::response::IntoResponse;
use phirepass_common::stats::Stats;
use serde_json::json;
use std::time::SystemTime;
use tower_http::cors::{Any, CorsLayer};

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
    } else {
        if let Some(origin) = state
            .env
            .access_control_allowed_origin
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| HeaderValue::from_str(s).ok())
        {
            cors = cors.allow_origin(origin);
        }
    }

    cors
}

pub async fn get_version() -> impl IntoResponse {
    Json(json!({
        "version": env::version(),
    }))
}

pub async fn get_stats(State(state): State<AppState>) -> impl IntoResponse {
    let nodes = state.nodes.read().await;
    let connections = state.connections.read().await;

    let body = match Stats::gather() {
        Some(stats) => json!({
            "stats": stats,
            "nodes": nodes.len(),
            "connections": connections.len(),
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
    let nodes = state.nodes.read().await;
    let now = SystemTime::now();

    let data: Vec<_> = nodes
        .iter()
        .map(|(id, info)| {
            json!({
                "id": id.to_string(),
                "ip": info.node.ip.to_string(),
                "connected_for_secs": now
                    .duration_since(info.node.connected_at)
                    .unwrap()
                    .as_secs(),
                "since_last_heartbeat_secs": now
                    .duration_since(info.node.last_heartbeat)
                    .unwrap()
                    .as_secs(),
                "stats": info.node.last_stats.clone(),
            })
        })
        .collect();
    Json(data)
}

pub async fn list_connections(State(state): State<AppState>) -> impl IntoResponse {
    let connections = state.connections.read().await;
    let now = SystemTime::now();

    let data: Vec<_> = connections
        .iter()
        .map(|(id, info)| {
            json!({
                "id": id.to_string(),
                "ip": info.node.ip.to_string(),
                "connected_for_secs": now
                    .duration_since(info.node.connected_at)
                    .unwrap()
                    .as_secs(),
                "since_last_heartbeat_secs": now
                    .duration_since(info.node.last_heartbeat)
                    .unwrap()
                    .as_secs(),
            })
        })
        .collect();

    Json(data)
}

pub async fn list_headers(headers: HeaderMap) -> impl IntoResponse {
    let mut obj = serde_json::Map::<String, serde_json::Value>::new();

    // Iterate unique header names, then collect all values for each name
    for name in headers.keys() {
        let values: Vec<serde_json::Value> = headers
            .get_all(name)
            .iter()
            .map(|v| serde_json::Value::String(v.to_str().unwrap_or("<non-utf8>").to_string()))
            .collect();

        obj.insert(name.as_str().to_string(), serde_json::Value::Array(values));
    }

    Json(serde_json::Value::Object(obj))
}
