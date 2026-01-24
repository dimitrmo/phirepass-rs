use crate::creds::TokenStore;
use crate::env::Env;
use crate::http::{AppState, get_version};
use crate::ws;
use axum::Router;
use axum::routing::get;
use log::{info, warn};
use phirepass_common::stats::Stats;
use secrecy::SecretString;
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::signal;
use tokio::sync::broadcast;

pub(crate) async fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let stats_refresh_interval = config.stats_refresh_interval;
    let (shutdown_tx, _) = broadcast::channel(1);

    let state = AppState::new(Arc::new(config));
    let ws_task = start_ws_connection(&state, shutdown_tx.subscribe());
    let http_task = start_http_server(state, shutdown_tx.subscribe());
    let stats_task = spawn_stats_logger(stats_refresh_interval as u64, shutdown_tx.subscribe());

    let shutdown_signal = async {
        if let Err(err) = signal::ctrl_c().await {
            warn!("failed to listen for shutdown signal: {}", err);
        } else {
            info!("ctrl+c pressed, shutting down");
        }
    };

    tokio::select! {
        _ = ws_task => warn!("ws task ended"),
        _ = http_task => warn!("http task ended"),
        _ = stats_task => warn!("stats logger task ended"),
        _ = shutdown_signal => info!("shutdown signal received"),
    }

    let _ = shutdown_tx.send(());

    info!("waiting for tasks to shut down gracefully...");
    tokio::time::sleep(Duration::from_millis(500)).await;

    Ok(())
}

pub(crate) async fn login(
    server_host: String,
    server_port: u16,
    file: Option<PathBuf>,
) -> anyhow::Result<()> {
    info!("logging in with {server_host}:{server_port}");

    let token = if let Some(file_path) = file {
        info!("reading token from file: {}", file_path.display());
        if !file_path.exists() {
            return Err(anyhow::anyhow!("file does not exist"));
        }

        let token = fs::read_to_string(&file_path).await?;
        token.trim().to_string()
    } else {
        rpassword::prompt_password("Enter authentication token: ")?
    };

    let scheme = if server_port == 443 { "https" } else { "http" };
    let url = format!(
        "{}://{}:{}/api/nodes/auth",
        scheme, server_host, server_port
    );

    info!("authenticating with server at {}", url);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&json!({
            "token": token,
            "version": crate::env::version(),
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!("authentication failed with status: {}", response.status());
    }

    let body = response.json::<serde_json::Value>().await?;

    // Extract node_id from response
    let node_id = body
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("node_id not found in response"))?
        .to_string();

    info!("successfully authenticated node_id={node_id}");

    let username = whoami::username()?;
    let ts = TokenStore::new(
        "phirepass",
        "agent",
        server_host.as_str(),
        username.as_str(),
    )?;
    ts.save(Some(&node_id), Some(&SecretString::from(token)))?;

    Ok(())
}

pub(crate) fn load_stored_node_id(server_host: &str) -> Option<ulid::Ulid> {
    let username = whoami::username().ok()?;
    let ts = TokenStore::new("phirepass", "agent", server_host, username.as_str()).ok()?;
    let (node_id, _) = ts.load().ok()?;
    node_id.and_then(|id| id.parse().ok())
}

fn start_http_server(
    state: AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let host = format!("{}:{}", state.env.host, state.env.port);

    tokio::spawn(async move {
        let app = Router::new()
            .route("/version", get(get_version))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(host).await.unwrap();
        info!("listening on: {}", listener.local_addr().unwrap());

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown.recv().await;
        })
        .await
        .unwrap();
    })
}

fn start_ws_connection(
    state: &AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let env = Arc::clone(&state.env);
    tokio::spawn(async move {
        let mut attempt: u32 = 0;

        let node_id = load_stored_node_id(&env.server_host);
        let stored_node_id = Arc::new(tokio::sync::RwLock::new(node_id));

        loop {
            let conn = ws::WebSocketConnection::new(stored_node_id.clone());

            tokio::select! {
                res = conn.connect(Arc::clone(&env)) => {
                    match res {
                        Ok(()) => warn!("ws connection ended, attempting reconnect"),
                        Err(err) => warn!("ws client error: {err}, attempting reconnect"),
                    }
                }
                _ = shutdown.recv() => {
                    info!("ws connection shutting down");
                    break;
                }
            }

            attempt = attempt.saturating_add(1);
            let backoff_secs = 2u64.saturating_pow(attempt.min(4));
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {},
                _ = shutdown.recv() => {
                    info!("ws connection shutting down");
                    break;
                }
            }
        }
    })
}

fn spawn_stats_logger(
    stats_refresh_interval: u64,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(stats_refresh_interval));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match Stats::refresh() {
                        Some(stats) => info!("agent stats\n{}", stats.log_line()),
                        None => warn!("stats: unable to read process metrics"),
                    }
                }
                _ = shutdown.recv() => {
                    info!("stats logger shutting down");
                    break;
                }
            }
        }
    })
}
