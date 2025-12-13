use crate::env;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderValue, Method};
use axum::response::IntoResponse;
use phirepass_common::stats::Stats;
use serde_json::json;
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

pub async fn list_nodes(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let nodes = state.nodes.read().await;

    let data: Vec<_> = nodes.iter().map(|(_, info)| info.node.clone()).collect();

    Json(data)
}
