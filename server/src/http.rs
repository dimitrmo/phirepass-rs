use axum::extract::State;
use axum::Json;
use axum::response::IntoResponse;
use phirepass_common::stats::Stats;
use crate::env;
use serde_json::json;
use crate::state::AppState;

pub async fn get_version() -> impl IntoResponse {
    Json(json!({
        "version": env::version(),
    }))
}

pub async fn get_stats(
    State(state): State<AppState>
) -> impl IntoResponse {
    let nodes = state.nodes.read().await;
    let connections = state.connections.read().await;

    let body = match Stats::gather() {
        None => json!({
            //
        }),
        Some(stats) => match stats.encoded() {
            Ok(stats) => json!({
                "stats": stats,
                "nodes": nodes.len(),
                "connections": connections.len(),
            }),
            Err(_) => json!({
                //
            }),
        }
    };

    Json(body)
}
