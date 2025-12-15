use axum::Json;
use axum::response::IntoResponse;
use serde_json::json;

pub async fn get_version() -> impl IntoResponse {
    Json(json!({
        "version": crate::env::version(),
    }))
}
