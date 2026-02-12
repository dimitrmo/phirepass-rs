use crate::creds::TokenStore;
use crate::env::Env;
use crate::http::{AppState, get_version};
use crate::ws;
use anyhow::Context;
use axum::Router;
use axum::routing::get;
use log::{debug, info, warn};
use phirepass_common::stats::Stats;
use phirepass_common::token::mask_after_10;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::signal;
use tokio::sync::broadcast;
use uuid::Uuid;

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
    from_stdin: bool,
) -> anyhow::Result<()> {
    info!("logging in with {server_host}:{server_port}");

    let token = if let Some(file_path) = file {
        info!("reading token from file: {}", file_path.display());
        if !file_path.exists() {
            return Err(anyhow::anyhow!("file does not exist"));
        }

        let token = fs::read_to_string(&file_path).await?;
        token.trim().to_string()
    } else if from_stdin {
        info!("reading token from stdin");
        let mut token = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut token)?;
        token.trim().to_string()
    } else {
        rpassword::prompt_password("Enter authentication token: ")?
    };

    info!("token found: {}", mask_after_10(token.as_str()));

    let username = whoami::username()?;
    let ts = TokenStore::new(
        "phirepass",
        "agent",
        server_host.as_str(),
        username.as_str(),
    )?;

    let existing_node_id = match ts.load_state_public() {
        Ok(Some(state)) if state.server_host == server_host && state.node_id != Uuid::nil() => {
            Some(state.node_id)
        }
        _ => None,
    };

    let url = match server_port {
        443 | 8443 => format!("https://{}/api/nodes/login", server_host),
        port => format!("http://{}:{}/api/nodes/login", server_host, port),
    };

    info!("authenticating with server at {}", url);

    let client = reqwest::Client::new();
    let mut payload = json!({
        "token": token,
        "version": crate::env::version(),
    });

    if let Some(node_id) = existing_node_id {
        payload["node_id"] = json!(node_id);
    }

    let response = client.post(&url).json(&payload).send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let body = serde_json::from_str::<serde_json::Value>(&body_text)
            .unwrap_or_else(|_| json!({ "raw": body_text }));

        let error_message = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("authentication failed");

        warn!("authentication failed: {}", error_message);

        let err_lower = error_message.to_ascii_lowercase();
        let should_clear = err_lower.contains("expired")
            || err_lower.contains("revoked")
            || err_lower.contains("invalid token")
            || err_lower.contains("failed to verify token")
            || err_lower.contains("token has expired");

        if should_clear {
            ts.delete().context("failed to delete local credentials")?;
            info!("local credentials deleted due to token failure");
        }

        anyhow::bail!("authentication failed ({}): {}", status, error_message);
    }

    let body = response.json::<serde_json::Value>().await?;

    let node_id_str = if let Some(node_id_val) = body.get("node_id") {
        if let Some(s) = node_id_val.as_str() {
            s.to_string()
        } else {
            serde_json::from_value::<Uuid>(node_id_val.clone())
                .map(|uuid| uuid.to_string())
                .map_err(|e| anyhow::anyhow!("Invalid node_id format in response: {}", e))?
        }
    } else {
        anyhow::bail!("node_id not found in response")
    };

    info!("successfully authenticated node_id={node_id_str}");

    info!("logging in with {username}");

    ts.save(&node_id_str, &SecretString::from(token))
        .context("failed to save token")?;

    info!("successfully saved credentials for node_id={}", node_id_str);

    Ok(())
}

pub(crate) async fn logout(server_host: String, server_port: u16) -> anyhow::Result<()> {
    info!("logging out from {server_host}:{server_port}");

    let username = whoami::username()?;
    let ts = TokenStore::new("phirepass", "agent", &server_host, &username)?;

    // Load current credentials
    let (node_id, token) = ts
        .load()
        .context("no active login found - please login first")?;

    info!("loaded credentials for node {node_id}");

    let scheme = if server_port == 443 { "https" } else { "http" };
    let url = format!(
        "{}://{}:{}/api/nodes/logout",
        scheme, server_host, server_port
    );

    info!("sending logout request to {}", url);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&json!({
            "node_id": node_id,
            "token": token.expose_secret(),
        }))
        .send()
        .await?;

    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    let body = serde_json::from_str::<serde_json::Value>(&body_text)
        .unwrap_or_else(|_| json!({ "raw": body_text }));

    let server_ok = status.is_success()
        && body
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

    if server_ok {
        info!("successfully logged out from server");
    } else {
        warn!("logout request rejected by server: {:?}", body);
    }

    // Delete local credentials regardless of server response
    ts.delete().context("failed to delete local credentials")?;

    info!("local credentials deleted - token is now free for use with another node");
    println!("Successfully logged out locally. Token is now available for reuse.");

    if server_ok {
        Ok(())
    } else {
        anyhow::bail!("logout request rejected by server: {:?}", body)
    }
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

/// Load credentials from any saved server.
/// This tries to load from a generic location first, without depending on env vars.
pub(crate) fn load_creds_from_any_server() -> Option<(String, Uuid, SecretString)> {
    let username = match whoami::username() {
        Ok(u) => u,
        Err(e) => {
            warn!("failed to get username: {}", e);
            return None;
        }
    };

    // Try to load state from a generic location (server_host doesn't matter for reading state)
    // We use empty string as service which will just use the standard path
    let ts = match TokenStore::new("phirepass", "agent", "", username.as_str()) {
        Ok(t) => t,
        Err(e) => {
            warn!("failed to create token store: {}", e);
            return None;
        }
    };

    // Load the state file directly to get server_host
    match ts.load_state_public() {
        Ok(Some(state)) => {
            if state.node_id == Uuid::nil() {
                warn!("stored node_id is nil");
                return None;
            }

            let token_str = if state.token.is_empty() {
                // Try to get from keyring with fixed service name
                match keyring::Entry::new("phirepass-agent", &username) {
                    Ok(entry) => match entry.get_password() {
                        Ok(t) => {
                            debug!("Token retrieved from keyring");
                            t
                        }
                        Err(e) => {
                            warn!("failed to get token from keyring: {}", e);
                            state.token
                        }
                    },
                    Err(_) => {
                        debug!("Keyring backend unavailable");
                        state.token
                    }
                }
            } else {
                debug!("Using token from state file");
                state.token
            };

            if token_str.is_empty() {
                warn!("no token found in state or keyring");
                return None;
            }

            Some((
                state.server_host,
                state.node_id,
                SecretString::from(token_str),
            ))
        }
        Ok(None) => {
            warn!("no state file found");
            None
        }
        Err(e) => {
            warn!("failed to load state: {}", e);
            None
        }
    }
}

fn start_ws_connection(
    state: &AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let env = Arc::clone(&state.env);
    tokio::spawn(async move {
        let mut attempt: u32 = 0;

        loop {
            // Load credentials from stored state, which includes the correct server_host
            let creds_result = load_creds_from_any_server();
            info!("credentials load result: {creds_result:?}");

            if let Some((_, node_id, token)) = creds_result {
                let conn = ws::WebSocketConnection::new(node_id, token);
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
            } else {
                warn!("credentials not found");
                info!("please login first");
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
