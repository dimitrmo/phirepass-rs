use crate::connection::WebConnection;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use phirepass_common::protocol::{
    Frame, NodeControlMessage, Protocol, WebControlMessage, decode_web_control,
};
use std::net::SocketAddr;
use std::time::Instant;
use tokio::sync::mpsc::unbounded_channel;
use ulid::Ulid;

pub(crate) async fn ws_web_handler(
    State(state): State<AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_web_socket(socket, state, addr))
}

async fn handle_web_socket(socket: WebSocket, state: AppState, addr: SocketAddr) {
    let id = Ulid::new();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = unbounded_channel::<Frame>();

    {
        let mut clients = state.clients.lock().await;
        clients.insert(id, WebConnection::new(addr, tx));
        let total = clients.len();
        info!("client {id} ({addr}) connected (total: {total})", id = id);
    }

    let write_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if let Err(err) = ws_tx.send(Message::Binary(frame.to_bytes().into())).await {
                warn!("failed to send frame to web client: {}", err);
                break;
            }
        }
    });

    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(_) => {
                disconnect_web_client(&state, id).await;
                return;
            }
        };

        match msg {
            Message::Binary(data) => {
                let Some(frame) = Frame::parse(&data) else {
                    warn!("received malformed frame");
                    continue;
                };

                match frame.protocol {
                    Protocol::Control => match decode_web_control(&frame.payload) {
                        Ok(msg) => match msg {
                            WebControlMessage::Heartbeat => {
                                update_web_heartbeat(&state, id).await;
                            }
                            WebControlMessage::OpenTunnel {
                                protocol,
                                target,
                                password,
                            } => {
                                handle_open_tunnel(&state, id, protocol, target, password).await;
                            }
                            WebControlMessage::TunnelData {
                                protocol,
                                target,
                                data,
                            } => {
                                handle_tunnel_data(&state, id, protocol, target, data).await;
                            }
                            WebControlMessage::Resize { target, cols, rows } => {
                                handle_resize(&state, id, target, cols, rows).await;
                            }
                            WebControlMessage::TunnelClosed { .. } => {},
                            WebControlMessage::Error { .. } => {}
                            WebControlMessage::Ok => {}
                        },
                        Err(err) => warn!("failed to parse control message: {}", err),
                    },
                    _ => {}
                }
            }
            Message::Close(err) => {
                match err {
                    None => warn!("client {id} disconnected"),
                    Some(err) => warn!("client {id} disconnected: {:?}", err),
                }
                disconnect_web_client(&state, id).await;
                return;
            }
            _ => {
                info!("unknown message: {:?}", msg);
            }
        }
    }

    disconnect_web_client(&state, id).await;
    write_task.abort();
}

async fn disconnect_web_client(state: &AppState, id: Ulid) {
    let mut clients = state.clients.lock().await;
    if let Some(info) = clients.remove(&id) {
        let alive = info.connected_at.elapsed();
        info!(
            "client {id} ({}) removed after {:.1?} (total: {})",
            info.addr,
            alive,
            clients.len()
        );
    }

    notify_nodes_client_disconnect(state, id).await;
}

async fn update_web_heartbeat(state: &AppState, id: Ulid) {
    let mut clients = state.clients.lock().await;
    if let Some(info) = clients.get_mut(&id) {
        let since_last = info.last_heartbeat.elapsed();
        info.last_heartbeat = Instant::now();
        info!(
            "heartbeat from web {id} ({}) after {:.1?}",
            info.addr, since_last
        );
    } else {
        warn!("received heartbeat for unknown web client {id}");
    }
}

async fn handle_tunnel_data(
    state: &AppState,
    cid: Ulid,
    protocol: u8,
    target: String,
    data: Vec<u8>,
) {
    info!("tunnel data received: {:?}", data);

    let target_id = match Ulid::from_string(&target) {
        Ok(id) => id,
        Err(err) => {
            warn!("invalid target id {target}: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.lock().await;
        nodes.get(&target_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for target not found {target}");
        return;
    };

    if tx
        .send(NodeControlMessage::TunnelData {
            protocol,
            cid: cid.to_string(),
            data,
        })
        .is_err()
    {
        warn!("failed to forward open tunnel to node {target}");
    } else {
        info!(
            "forwarded open tunnel to node {target} (protocol {})",
            protocol
        );
    }
}

async fn handle_resize(state: &AppState, cid: Ulid, target: String, cols: u32, rows: u32) {
    let target_id = match Ulid::from_string(&target) {
        Ok(id) => id,
        Err(err) => {
            warn!("invalid target id {target}: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.lock().await;
        nodes.get(&target_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for target not found {target}");
        return;
    };

    if tx
        .send(NodeControlMessage::Resize {
            cid: cid.to_string(),
            cols,
            rows,
        })
        .is_err()
    {
        warn!("failed to forward resize to node {target}");
    }
}

async fn handle_open_tunnel(
    state: &AppState,
    cid: Ulid,
    protocol: u8,
    target: String,
    password: Option<String>,
) {
    info!(
        "received open tunnel message protocol={:?} target={:?}",
        protocol, target
    );

    let target_id = match Ulid::from_string(&target) {
        Ok(id) => id,
        Err(err) => {
            warn!("invalid target id {target}: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.lock().await;
        nodes.get(&target_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for target not found {target}");
        return;
    };

    if tx
        .send(NodeControlMessage::OpenTunnel {
            protocol,
            cid: cid.to_string(),
            password,
        })
        .is_err()
    {
        warn!("failed to forward open tunnel to node {target}");
    } else {
        info!(
            "forwarded open tunnel to node {target} (protocol {})",
            protocol
        );
    }
}

async fn notify_nodes_client_disconnect(state: &AppState, cid: Ulid) {
    let cid_str = cid.to_string();
    let nodes = state.nodes.lock().await;

    for (node_id, conn) in nodes.iter() {
        match conn.tx.send(NodeControlMessage::ConnectionDisconnect {
            cid: cid_str.clone(),
        }) {
            Ok(..) => info!("notified node {node_id} about client {cid_str} disconnect"),
            Err(err) => {
                warn!("failed to notify node {node_id} about client {cid_str} disconnect: {err}")
            }
        }
    }
}
