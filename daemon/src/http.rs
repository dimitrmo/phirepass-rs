use axum::http::StatusCode;
use axum::response::IntoResponse;

pub async fn get_ready() -> impl IntoResponse {
    StatusCode::OK
}

pub async fn get_alive() -> impl IntoResponse {
    StatusCode::OK
}
