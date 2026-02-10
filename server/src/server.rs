use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::db::postgres::Database;
use crate::db::redis::MemoryDB;
use crate::env::Env;
use crate::http::{AppState, build_cors, get_stats, get_version, list_connections};
use crate::node::{login_node, logout_node, ws_node_handler};
use crate::stun;
use crate::web::ws_web_handler;
use axum::Router;
use axum::routing::{get, post};
use dashmap::DashMap;
use log::{info, warn};
use phirepass_common::stats::Stats;
use tokio::signal;
use tokio::sync::broadcast;
use uuid::Uuid;

pub async fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let stats_refresh_interval = config.stats_refresh_interval;
    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    let address = stun::get_public_address()?;
    info!("public ip: {}", address.ip());

    let db = Database::create(&config).await?;
    info!("connected to postgres");

    let memory_db = MemoryDB::create(&config).await?;
    info!("connected to valkey");

    let state = AppState {
        id: Arc::new(Uuid::new_v4()),
        address: Arc::new(address),
        env: Arc::new(config),
        db: Arc::new(db),
        memory_db: Arc::new(memory_db),
        nodes: Arc::new(DashMap::new()),
        connections: Arc::new(DashMap::new()),
        tunnel_sessions: Arc::new(DashMap::new()),
    };

    let server_task = spawn_server_update_task(&state, 30u64);
    let conns_task = spawn_stats_connections_logger(&state, stats_refresh_interval as u64);
    let http_task = start_http_server(state.clone(), shutdown_tx.subscribe());
    let stats_task = spawn_stats_logger(stats_refresh_interval as u64, shutdown_tx.subscribe());
    let cleanup_task = spawn_connection_cleanup_task(&state, 30, shutdown_tx.subscribe());

    let shutdown_signal = async {
        if let Err(err) = signal::ctrl_c().await {
            warn!("failed to listen for shutdown signal: {}", err);
        } else {
            info!("ctrl+c pressed, shutting down");
        }
    };

    tokio::select! {
        _ = server_task => warn!("server task terminated"),
        _ = http_task => warn!("http task ended"),
        _ = stats_task => warn!("stats logger task ended"),
        _ = conns_task => warn!("connections stats task ended"),
        _ = cleanup_task => warn!("cleanup task ended"),
        _ = shutdown_signal => info!("shutdown signal received"),
    }

    // Tell all tasks to shut down if they have not already received the signal.
    let _ = shutdown_tx.send(());

    Ok(())
}

fn start_http_server(
    state: AppState,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    info!("starting http server");

    let ip_source = state.env.ip_source.clone();
    let host = format!("{}:{}", state.env.host, state.env.port);

    tokio::spawn(async move {
        let cors = build_cors(&state);

        let app = Router::new()
            .route("/api/web/ws", get(ws_web_handler))
            .route("/api/nodes/login", post(login_node))
            .route("/api/nodes/logout", post(logout_node))
            .route("/api/nodes/ws", get(ws_node_handler))
            .route("/api/connections", get(list_connections))
            .route("/stats", get(get_stats))
            .route("/version", get(get_version))
            .layer(ip_source.into_extension())
            .layer(cors)
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

fn spawn_server_update_task(state: &AppState, interval: u64) -> tokio::task::JoinHandle<()> {
    info!("spawning server update task");

    let db = state.memory_db.clone();
    let id = state.id.clone();
    let ip = state.address.ip().to_string();
    let port = state.env.port;
    let fqdn = state.env.fqdn.clone();

    tokio::spawn(async move {
        let id = id.as_ref();
        let ip = ip.clone();
        let fqdn = fqdn.clone();
        let db = db.clone();
        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            if let Err(err) = db.save_server(id, ip.clone(), port, fqdn.clone()).await {
                warn!("failed to save server info: {}", err);
            }
            interval.tick().await;
        }
    })
}

fn spawn_stats_connections_logger(state: &AppState, interval: u64) -> tokio::task::JoinHandle<()> {
    info!("starting stats connections worker");

    let connections = state.connections.clone();
    let nodes = state.nodes.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            interval.tick().await;
            info!("active web connections: {}", connections.len());
            info!("active nodes connections: {}", nodes.len());
        }
    })
}

fn spawn_stats_logger(
    interval: u64,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    info!("starting stats logger");

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match Stats::refresh() {
                        Some(stats) => info!("server stats\n{}", stats.log_line()),
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

fn spawn_connection_cleanup_task(
    state: &AppState,
    interval: u64,
    mut shutdown: broadcast::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    info!("starting connection cleanup task");

    let connections = state.connections.clone();
    let nodes = state.nodes.clone();

    tokio::spawn(async move {
        // Connections without heartbeat for longer than this are considered stale and removed
        const CONNECTION_TIMEOUT: Duration = Duration::from_secs(3600); // 1 hour

        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let now = SystemTime::now();

                    // Clean up stale web connections
                    let mut removed_count = 0;
                    connections.retain(|_, conn| {
                        match now.duration_since(conn.last_heartbeat) {
                            Ok(elapsed) => {
                                if elapsed > CONNECTION_TIMEOUT {
                                    warn!(
                                        "removing stale web connection from {} (inactive for {:.1?})",
                                        conn.ip, elapsed
                                    );
                                    removed_count += 1;
                                    false  // Remove this connection
                                } else {
                                    true  // Keep this connection
                                }
                            }
                            Err(_) => true,  // Keep if time went backwards
                        }
                    });

                    if removed_count > 0 {
                        info!(
                            "cleanup: removed {} stale web connections (active: {})",
                            removed_count,
                            connections.len()
                        );
                    }

                    // Clean up stale node connections
                    let mut removed_count = 0;
                    nodes.retain(|_, node| {
                        match now.duration_since(node.node.last_heartbeat) {
                            Ok(elapsed) => {
                                if elapsed > CONNECTION_TIMEOUT {
                                    warn!(
                                        "removing stale node from {} (inactive for {:.1?})",
                                        node.node.ip, elapsed
                                    );
                                    removed_count += 1;
                                    false  // Remove this node
                                } else {
                                    true  // Keep this node
                                }
                            }
                            Err(_) => true,  // Keep if time went backwards
                        }
                    });

                    if removed_count > 0 {
                        info!(
                            "cleanup: removed {} stale node connections (active: {})",
                            removed_count,
                            nodes.len()
                        );
                    }
                }
                _ = shutdown.recv() => {
                    info!("connection cleanup task shutting down");
                    break;
                }
            }
        }
    })
}
