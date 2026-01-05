use crate::connection::NodeConnection;
use crate::env;
use crate::http::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum_client_ip::ClientIp;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use phirepass_common::protocol::common::{Frame, FrameData};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::web::WebFrameData;
use phirepass_common::stats::Stats;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use ulid::Ulid;

pub(crate) async fn ws_node_handler(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let ip = phirepass_common::ip::extract_ip_from_headers(&headers).unwrap_or(ip);
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
            version: daemon_version,
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
                "node {id} authenticated from {ip} (daemon version: {daemon_version}, reusing id: {})",
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
    ws_rx: &mut futures_util::stream::SplitStream<axum::extract::ws::WebSocket>,
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
                    // ping from daemon
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
                    // daemon notified server that a tunnel has been opened
                    NodeFrameData::TunnelOpened {
                        protocol,
                        cid,
                        sid,
                        msg_id,
                    } => {
                        handle_tunnel_opened(&state, protocol, cid, sid, &id, msg_id).await;
                    }
                    // daemon notified server with data for web
                    NodeFrameData::WebFrame { frame, sid } => {
                        handle_frame_response(&state, frame, sid, &id).await;
                    }
                    // daemon notified server with data for web
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

async fn get_connection_id_by_sid(
    state: &AppState,
    sid: u32,
    target: &Ulid,
) -> anyhow::Result<Ulid> {
    let key = crate::http::TunnelSessionKey::new(*target, sid);
    let (client_id, node_id) = match state.tunnel_sessions.get(&key) {
        Some(entry) => {
            let (cid, nid) = entry.value();
            (*cid, *nid)
        }
        _ => {
            anyhow::bail!("node not found for session id {sid}")
        }
    };

    if !node_id.eq(target) {
        anyhow::bail!("correct node_id was not found for sid {sid}")
    }

    Ok(client_id)
}

async fn handle_frame_response(state: &AppState, frame: WebFrameData, sid: u32, node_id: &Ulid) {
    debug!("web frame response received");

    let client_id = match get_connection_id_by_sid(state, sid, node_id).await {
        Ok(client_id) => client_id,
        Err(err) => {
            warn!("error getting client id: {err}");
            return;
        }
    };

    let tx = state
        .connections
        .get(&client_id)
        .map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for client not found {node_id}");
        return;
    };

    match tx.send(frame).await {
        Ok(_) => debug!("forwarded tunnel data to node {node_id}"),
        Err(err) => warn!("failed to forward tunnel data to node {node_id}: {err}"),
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

    let Some(connection) = state.connections.get(&cid) else {
        warn!("connection {cid} not found");
        return;
    };

    {
        let key = crate::http::TunnelSessionKey::new(*node_id, sid);
        state.tunnel_sessions.remove(&key);
    }

    match connection
        .tx
        .send(WebFrameData::TunnelClosed {
            protocol,
            sid,
            msg_id,
        })
        .await
    {
        Ok(..) => info!("tunnel closed notification sent to web client {cid}"),
        Err(err) => warn!("failed to send tunnel closed to client {cid}: {err}"),
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

    let Some(connection) = state.connections.get(&cid) else {
        warn!("connection {cid} not found");
        return;
    };

    {
        let key = crate::http::TunnelSessionKey::new(*node_id, sid);
        state.tunnel_sessions.insert(key, (cid, *node_id));
    }

    match connection
        .tx
        .send(WebFrameData::TunnelOpened {
            protocol,
            sid,
            msg_id,
        })
        .await
    {
        Ok(..) => info!("tunnel opened notification sent to web client {cid}"),
        Err(err) => warn!("failed to send tunnel opened to client {cid}: {err}"),
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
    }
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
