use axum::Json;
use axum::response::IntoResponse;
use phirepass_common::stats::Stats;
use crate::env;
use serde_json::json;

pub async fn get_version() -> impl IntoResponse {
    Json(json!({
        "version": env::version(),
    }))
}

pub async fn get_stats() -> impl IntoResponse {
    let body = match Stats::gather() {
        None => json!({
            //
        }),
        Some(stats) => match stats.encoded() {
            Ok(stats) => json!({
                "stats": stats
            }),
            Err(_) => json!({
                //
            }),
        }
    };

    Json(body)
}
