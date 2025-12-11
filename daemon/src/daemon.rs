use crate::env::Env;
use crate::http::{get_alive, get_ready};
use crate::ws;
use axum::Router;
use axum::routing::get;
use log::{info, warn};
use phirepass_common::stats::Stats;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub(crate) async fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let stats_refresh_interval = config.stats_refresh_interval;
    let host = format!("{}:{}", config.host, config.port);

    let ws_task = start_ws_connection(config);
    let stats_task = spawn_stats_logger(stats_refresh_interval);
    let http_task = start_http_server(host);

    tokio::select! {
        _ = ws_task => warn!("ws task ended"),
        _ = http_task => warn!("http task ended"),
        _ = stats_task => warn!("stats logger task ended"),
    }

    Ok(())
}

fn start_ws_connection(config: Env) -> tokio::task::JoinHandle<()> {
    let env = Arc::new(config);

    tokio::spawn(async move {
        let mut attempt: u32 = 0;

        loop {
            let conn = ws::WSConnection::new();

            match conn.connect(env.clone()).await {
                Ok(()) => warn!("ws connection ended, attempting reconnect"),
                Err(err) => warn!("ws client error: {err}, attempting reconnect"),
            }

            attempt = attempt.saturating_add(1);
            let backoff_secs = 2u64.saturating_pow(attempt.min(4));
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        }
    })
}

fn start_http_server(host: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let app = Router::new()
            .route("/ready", get(get_ready))
            .route("/alive", get(get_alive));

        let listener = tokio::net::TcpListener::bind(host).await.unwrap();
        info!("listening on: {}", listener.local_addr().unwrap());

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    })
}

fn spawn_stats_logger(stats_refresh_interval: u16) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(stats_refresh_interval as u64));
        loop {
            interval.tick().await;
            match Stats::gather() {
                Some(stats) => info!("{}", stats.log_line()),
                None => warn!("Stats: unable to read process metrics"),
            }
        }
    })
}
