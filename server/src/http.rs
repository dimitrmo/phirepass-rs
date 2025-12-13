use crate::env;
use crate::state::AppState;
use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use phirepass_common::stats::Stats;
use serde_json::json;

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
