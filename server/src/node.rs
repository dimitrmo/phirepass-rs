use crate::connection::NodeConnection;
use crate::env;
use crate::http::AppState;
use argon2::{PasswordHash, PasswordVerifier};
use axum::Json;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum_client_ip::ClientIp;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use phirepass_common::protocol::common::{Frame, FrameData};
use phirepass_common::protocol::node::{NodeFrameData, WebFrameId};
use phirepass_common::protocol::web::WebFrameData;
use phirepass_common::stats::Stats;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use ulid::Ulid;

pub(crate) async fn ws_node_handler(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_node_socket(socket, state, ip))
}

async fn wait_for_auth(
    ws_rx: &mut futures_util::stream::SplitStream<WebSocket>,
    tx: &mpsc::Sender<NodeFrameData>,
    ip: IpAddr,
) -> anyhow::Result<Ulid> {
    // Wait for the first message which must be Auth
    let msg = ws_rx
        .next()
        .await
        .ok_or_else(|| anyhow::anyhow!("connection closed before auth"))??;

    let data = match msg {
        Message::Binary(data) => data,
        Message::Close(reason) => {
            anyhow::bail!("connection closed before auth: {:?}", reason);
        }
        _ => {
            anyhow::bail!("expected binary message for auth, got: {:?}", msg);
        }
    };

    let frame = Frame::decode(&data)?;

    let node_frame = match frame.data {
        FrameData::Node(data) => data,
        FrameData::Web(_) => {
            anyhow::bail!("expected node frame for auth, got web frame");
        }
    };

    match node_frame {
        NodeFrameData::Auth {
            token: _,
            node_id: received_node_id,
            version: agent_version,
        } => {
            // Use provided node_id if available, otherwise generate a new one
            let id = match received_node_id {
                Some(node_id) => {
                    info!("suggested node id found: {node_id}");
                    node_id
                }
                None => {
                    let id = Ulid::new();
                    info!("assigning new node id: {id}");
                    id
                }
            };

            info!(
                "node {id} authenticated from {ip} (agent version: {agent_version}, reusing id: {})",
                received_node_id.is_some()
            );

            // Send auth response
            let resp = NodeFrameData::AuthResponse {
                node_id: id,
                success: true,
                version: env::version().to_string(),
            };

            tx.send(resp)
                .await
                .map_err(|err| anyhow::anyhow!("failed to send auth response: {err}"))?;

            Ok(id)
        }
        other => {
            anyhow::bail!("expected Auth as first message, got: {:?}", other);
        }
    }
}

async fn handle_node_socket(socket: WebSocket, state: AppState, ip: IpAddr) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Bounded channel to avoid unbounded memory growth if the node socket is back-pressured.
    let (tx, mut rx) = mpsc::channel::<NodeFrameData>(256);

    // Wait for authentication as the first message
    let id = match wait_for_auth(&mut ws_rx, &tx, ip).await {
        Ok(node_id) => node_id,
        Err(err) => {
            warn!("authentication failed from {ip}: {err}");
            let _ = ws_tx.close().await;
            return;
        }
    };

    {
        state.nodes.insert(id, NodeConnection::new(ip, tx.clone()));
        let total = state.nodes.len();
        info!("node {id} ({ip}) authenticated and registered (total: {total})");
    }

    let write_task = tokio::spawn(async move {
        while let Some(node_frame) = rx.recv().await {
            let frame: Frame = node_frame.into();
            let frame = match frame.to_bytes() {
                Ok(frame) => frame,
                Err(err) => {
                    warn!("web frame error: {err}");
                    break;
                }
            };

            if let Err(err) = ws_tx.send(Message::Binary(frame.into())).await {
                warn!("failed to send frame to web connection: {}", err);
                break;
            }
        }
    });

    // Handle messages in separate function to ensure cleanup always happens
    handle_node_messages(&mut ws_rx, &state, id, &tx).await;

    // Always abort write task regardless of how we exited message loop
    drop(tx); // Close sender first to wake write task
    write_task.abort();
    disconnect_node(&state, id).await;
}

/// Handles incoming WebSocket messages. Always returns to parent for cleanup.
async fn handle_node_messages(
    ws_rx: &mut futures_util::stream::SplitStream<WebSocket>,
    state: &AppState,
    id: Ulid,
    tx: &mpsc::Sender<NodeFrameData>,
) {
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(err) => {
                warn!("node web socket error: {err}");
                disconnect_node(&state, id).await;
                return;
            }
        };

        match msg {
            Message::Close(reason) => {
                warn!("node connection close message: {:?}", reason);
                return; // Cleanup handled by caller
            }
            Message::Binary(data) => {
                let frame = match Frame::decode(&data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        warn!("received malformed frame 23: {err}");
                        warn!("received frame: {data:?}");
                        break;
                    }
                };

                debug!("received frame: {frame:?}");

                let node_frame = match frame.data {
                    FrameData::Node(data) => data,
                    FrameData::Web(_) => {
                        warn!("received web frame, but expected a node frame");
                        break;
                    }
                };

                match node_frame {
                    NodeFrameData::Heartbeat { stats } => {
                        update_node_heartbeat(&state, &id, Some(stats)).await;
                    }
                    NodeFrameData::Auth { .. } => {
                        warn!("received Auth message after initial authentication from node {id}");
                    }
                    // ping from agent
                    NodeFrameData::Ping { sent_at } => {
                        let now = now_millis();
                        let latency = now.saturating_sub(sent_at);
                        info!("ping from node {id}; latency={}ms", latency);
                        let pong = NodeFrameData::Pong { sent_at: now };
                        if let Err(err) = tx.send(pong).await {
                            warn!("failed to queue pong for node {id}: {err}");
                        } else {
                            info!("pong response to node {id} sent");
                        }
                    }
                    // agent notified server that a tunnel has been opened
                    NodeFrameData::TunnelOpened {
                        protocol,
                        cid,
                        sid,
                        msg_id,
                    } => {
                        handle_tunnel_opened(&state, protocol, cid, sid, &id, msg_id).await;
                    }
                    // agent notified server with data for web
                    NodeFrameData::WebFrame { .. } => {
                        handle_frame_response(&state, node_frame, id).await;
                    }
                    // agent notified server with data for web
                    NodeFrameData::TunnelClosed {
                        protocol,
                        cid,
                        sid,
                        msg_id,
                    } => {
                        handle_tunnel_closed(&state, protocol, cid, sid, &id, msg_id).await;
                    }
                    o => warn!("unhandled node frame: {o:?}"),
                }
            }
            _ => {
                info!("unknown message: {:?}", msg);
            }
        }
    }
}

async fn handle_frame_response(state: &AppState, node_frame: NodeFrameData, node_id: Ulid) {
    debug!("web frame response received");

    let NodeFrameData::WebFrame { frame, id } = node_frame else {
        warn!("node frame not of webframe type");
        return;
    };

    let cid = match id {
        WebFrameId::ConnectionId(cid) => cid,
        WebFrameId::SessionId(sid) => match state.get_connection_id_by_sid(sid, node_id).await {
            Ok(client_id) => client_id,
            Err(err) => {
                warn!("error getting client id: {err}");
                return;
            }
        },
    };

    match state.notify_client_by_cid(cid, frame).await {
        Ok(_) => debug!("forwarded tunnel data to node {node_id} for client {cid}"),
        Err(_) => {} // Error already logged in notify_client_by_cid
    }
}

async fn handle_tunnel_closed(
    state: &AppState,
    protocol: u8,
    cid: Ulid,
    sid: u32,
    node_id: &Ulid,
    msg_id: Option<u32>,
) {
    debug!("handling tunnel closed for connection {cid} with session {sid}");

    let key = crate::http::TunnelSessionKey::new(*node_id, sid);
    state.tunnel_sessions.remove(&key);

    match state
        .notify_client_by_cid(
            cid,
            WebFrameData::TunnelClosed {
                protocol,
                sid,
                msg_id,
            },
        )
        .await
    {
        Ok(..) => info!("tunnel closed notification sent to web client {cid}"),
        Err(_) => {} // Error already logged in notify_client_by_cid
    }
}

async fn handle_tunnel_opened(
    state: &AppState,
    protocol: u8,
    cid: Ulid,
    sid: u32,
    node_id: &Ulid,
    msg_id: Option<u32>,
) {
    debug!("handling tunnel opened for connection {cid} with session {sid}");

    let key = crate::http::TunnelSessionKey::new(*node_id, sid);
    state.tunnel_sessions.insert(key, (cid, *node_id));

    match state
        .notify_client_by_cid(
            cid,
            WebFrameData::TunnelOpened {
                protocol,
                sid,
                msg_id,
            },
        )
        .await
    {
        Ok(..) => info!("tunnel opened notification sent to web client {cid}"),
        Err(_) => {} // Error already logged in notify_client_by_cid
    }
}

async fn disconnect_node(state: &AppState, id: Ulid) {
    if let Some((_, info)) = state.nodes.remove(&id) {
        let alive = info.node.connected_at.elapsed();
        let total = state.nodes.len();
        info!(
            "node {id} ({}) removed after {:.1?} (total: {})",
            info.node.ip, alive, total
        );
        let total = notify_all_clients_for_closed_tunnel(state, id).await;
        info!("notified {total} client(s) for node {id} shutdown",)
    }
}

async fn notify_all_clients_for_closed_tunnel(state: &AppState, id: Ulid) -> u32 {
    let mut count = 0u32;

    let sessions_to_close: Vec<_> = state
        .tunnel_sessions
        .iter()
        .filter(|entry| entry.key().node_id == id)
        .map(|entry| (entry.key().clone(), entry.value().clone()))
        .collect();

    for (key, (cid, _)) in sessions_to_close {
        state.tunnel_sessions.remove(&key);

        if let Ok(_) = state
            .notify_client_by_cid(
                cid,
                WebFrameData::TunnelClosed {
                    protocol: 0,
                    sid: key.sid,
                    msg_id: None,
                },
            )
            .await
        {
            count += 1;
            info!("tunnel closed notification sent to web client {cid} due to node disconnect");
        }
    }

    count
}

async fn update_node_heartbeat(state: &AppState, id: &Ulid, stats: Option<Stats>) {
    if let Some(mut info) = state.nodes.get_mut(id) {
        let since_last = info.node.last_heartbeat.elapsed();
        info.node.last_heartbeat = SystemTime::now();
        if let Some(stats) = stats {
            let log_line = stats.log_line();
            info.node.last_stats = Some(stats);
            info!(
                "heartbeat from node {id} ({}) after {:.1?}; \n{}",
                info.node.ip, since_last, log_line
            );
        } else {
            info!(
                "heartbeat from node {id} ({}) after {:.1?}",
                info.node.ip, since_last
            );
        }
    } else {
        warn!("received heartbeat for unknown node {id}");
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthRequest {
    pub token: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct AuthResponse {
    pub node_id: String,
    pub success: bool,
}

fn unauthorized(value: serde_json::Value) -> Response {
    (StatusCode::UNAUTHORIZED, Json(value)).into_response()
}

fn success(value: serde_json::Value) -> Response {
    (StatusCode::OK, Json(value)).into_response()
}

pub async fn authenticate_node(
    State(state): State<AppState>,
    Json(payload): Json<AuthRequest>,
) -> impl IntoResponse {
    info!(
        "authenticating node with token version: {}",
        payload.version
    );

    let token = payload.token.trim();
    if token.is_empty() {
        return unauthorized(json!({
            "success": false,
            "error": "token is required",
        }));
    }

    info!("validating token for node...");

    if !token.starts_with("pat_") {
        warn!("token does not look like a PAT");
        return unauthorized(json!({
            "success": false,
            "error": "invalid token format",
        }));
    }

    info!("found token starting with pat_");

    let Some(pat_body) = token.strip_prefix("pat_") else {
        warn!("token missing pat_ prefix after trim");
        return unauthorized(json!({
            "success": false,
            "error": "broken token format",
        }));
    };

    info!("token body extracted");

    let (token_id, secret) = match pat_body.split_once('.') {
        Some((token_id, secret)) => (token_id, Some(secret)),
        None => (pat_body, None), // allow legacy pat_<id> tokens without a secret
    };

    let Some(secret) = secret else {
        warn!("token missing secret");
        return unauthorized(json!({
            "success": false,
            "error": "invalid token format",
        }));
    };

    info!("token format verified: contains secret");

    let token_record = match state.db.get_token_by_id(token_id).await {
        Ok(record) => record,
        Err(err) => {
            warn!("database error while validating token {token_id}: {err}");
            return unauthorized(json!({
                "success": false,
                "error": "invalid token",
            }));
        }
    };

    info!("token record {} found", token_record.id);

    if let Some(expires_at) = token_record.expires_at {
        if expires_at < Utc::now() {
            warn!("token expired: {}", token_id);
            return unauthorized(json!({
                "success": false,
                "error": "token has expired",
            }));
        }
    }

    info!("token is still valid");

    let parsed_hash = match PasswordHash::new(&token_record.token_hash) {
        Ok(hash) => hash,
        Err(err) => {
            warn!("failed to parse stored password hash: {}", err);
            return unauthorized(json!({
                "success": false,
                "error": "failed to validate token",
            }));
        }
    };

    info!("password hash calculated");

    if let Err(e) = state
        .db
        .hasher
        .verify_password(secret.as_bytes(), &parsed_hash)
    {
        warn!("invalid token secret for token_id={}: {}", token_id, e);
        return unauthorized(json!({
            "success": false,
            "error": "failed to verify token",
        }));
    }

    info!("password verified successfully");

    let node_record = match state.db.create_node_from_token(&token_record).await {
        Ok(record) => record,
        Err(err) => {
            warn!("failed to create node for token {token_id}: {err}");
            return unauthorized(json!({
                "success": false,
                "error": "failed to create node",
            }));
        }
    };

    let node_id = node_record.id;
    info!("node authenticated successfully: {}", node_id);

    success(json!({
        "success": true,
        "node_id": node_id.to_string(),
    }))
}
