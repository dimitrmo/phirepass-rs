use crate::env::Env;
use axum::Json;
use axum::response::IntoResponse;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) env: Arc<Env>,
}

pub async fn get_version() -> impl IntoResponse {
    Json(json!({
        "version": crate::env::version(),
    }))
}
