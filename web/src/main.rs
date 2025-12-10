use std::net::SocketAddr;

use axum::{
    Router,
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};
use log::info;

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

async fn xterm_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../static/xterm.min.js"),
    )
}

async fn xterm_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../static/xterm.css"),
    )
}

async fn xterm_fit_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../static/xterm-addon-fit.js"),
    )
}

async fn favicon() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "image/x-icon")],
        include_bytes!("../static/favicon.ico").as_slice(),
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    phirepass_common::logger::init_logger("phirepass:web");

    let app = Router::new()
        .route("/", get(index))
        .route("/xterm.js", get(xterm_js))
        .route("/xterm.css", get(xterm_css))
        .route("/xterm-addon-fit.js", get(xterm_fit_js))
        .route("/favicon.ico", get(favicon));

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    info!("serving web ui on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
