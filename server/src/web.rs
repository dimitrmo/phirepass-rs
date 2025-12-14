use crate::connection::WebConnection;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum_client_ip::ClientIp;
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use phirepass_common::protocol::{
    Frame, NodeControlMessage, Protocol, WebControlErrorType, WebControlMessage,
    decode_web_control, encode_web_control_to_frame,
};
use std::net::IpAddr;
use std::time::SystemTime;
use tokio::sync::mpsc;
use ulid::Ulid;

pub(crate) async fn ws_web_handler(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let ip = phirepass_common::ip::extract_ip_from_headers(&headers).unwrap_or(ip);
    ws.on_upgrade(move |socket| handle_web_socket(socket, state, ip))
}

async fn handle_web_socket(socket: WebSocket, state: AppState, ip: IpAddr) {
    let id = Ulid::new();
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Bounded channel so slow clients cannot grow memory unbounded.
    let (tx, mut rx) = mpsc::channel::<Frame>(256);

    {
        let mut connections = state.connections.write().await;
        connections.insert(id, WebConnection::new(ip, tx));
        let total = connections.len();
        info!(
            "connection {id} ({ip}) established (total: {total})",
            id = id
        );
    }

    let write_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if let Err(err) = ws_tx.send(Message::Binary(frame.to_bytes().into())).await {
                warn!("failed to send frame to web connection: {}", err);
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
                                username,
                                password,
                            } => {
                                handle_open_tunnel(
                                    &state, id, protocol, target, username, password,
                                )
                                .await;
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
                            WebControlMessage::TunnelClosed { .. } => {}
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
    let mut connections = state.connections.write().await;
    if let Some(info) = connections.remove(&id) {
        let alive = info.node.connected_at.elapsed();
        info!(
            "connection {id} ({}) removed after {:.1?} (total: {})",
            info.node.ip,
            alive,
            connections.len()
        );
    }

    notify_nodes_client_disconnect(state, id).await;
}

async fn update_web_heartbeat(state: &AppState, id: Ulid) {
    let mut connections = state.connections.write().await;
    if let Some(info) = connections.get_mut(&id) {
        let since_last = info.node.last_heartbeat.elapsed();
        info.node.last_heartbeat = SystemTime::now();
        info!(
            "heartbeat from web {id} ({}) after {:.1?}",
            info.node.ip, since_last
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
    info!("tunnel data received: {} bytes", data.len());

    let target_id = match Ulid::from_string(&target) {
        Ok(id) => id,
        Err(err) => {
            warn!("invalid target id {target}: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&target_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for target not found {target}");
        return;
    };

    match tx.try_send(NodeControlMessage::TunnelData {
        protocol,
        cid: cid.to_string(),
        data,
    }) {
        Ok(_) => {
            info!(
                "forwarded open tunnel to node {target} (protocol {})",
                protocol
            );
        }
        Err(err) => warn!("dropping tunnel data for target {target}: {err}"),
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
        let nodes = state.nodes.read().await;
        nodes.get(&target_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for target not found {target}");
        return;
    };

    if let Err(err) = tx.try_send(NodeControlMessage::Resize {
        cid: cid.to_string(),
        cols,
        rows,
    }) {
        warn!("dropping resize for node {target}: {err}");
    }
}

async fn handle_open_tunnel(
    state: &AppState,
    cid: Ulid,
    protocol: u8,
    target: String,
    username: Option<String>,
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
        let nodes = state.nodes.read().await;
        nodes.get(&target_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for target not found {target}");
        return;
    };

    let Some(username) = username else {
        warn!("username not found");
        let _ = send_requires_username_password_error(&state, cid).await;
        return;
    };

    let Some(password) = password else {
        warn!("password not found");
        let _ = send_requires_password_error(&state, cid).await;
        return;
    };

    if let Err(err) = tx.try_send(NodeControlMessage::OpenTunnel {
        protocol,
        cid: cid.to_string(),
        username,
        password,
    }) {
        warn!("dropping open tunnel to node {target}: {err}");
    } else {
        info!(
            "forwarded open tunnel to node {target} (protocol {})",
            protocol
        );
    }
}

async fn send_requires_username_password_error(state: &AppState, cid: Ulid) -> anyhow::Result<()> {
    let connections = state.connections.read().await;
    if let Some(info) = connections.get(&cid) {
        let error = WebControlMessage::Error {
            kind: WebControlErrorType::RequiresUsernamePassword,
            message: "Credentials are required".to_string(),
        };

        let frame = encode_web_control_to_frame(&error)?;
        info.tx.send(frame).await?;
    } else {
        warn!("failed to find connection {cid}");
    }

    Ok(())
}

async fn send_requires_password_error(state: &AppState, cid: Ulid) -> anyhow::Result<()> {
    let connections = state.connections.read().await;
    if let Some(info) = connections.get(&cid) {
        let error = WebControlMessage::Error {
            kind: WebControlErrorType::RequiresPassword,
            message: "Password is required".to_string(),
        };

        let frame = encode_web_control_to_frame(&error)?;
        info.tx.send(frame).await?;
    } else {
        warn!("failed to find connection {cid}");
    }

    Ok(())
}

async fn notify_nodes_client_disconnect(state: &AppState, cid: Ulid) {
    let cid_str = cid.to_string();
    let nodes = state.nodes.read().await;

    for (node_id, conn) in nodes.iter() {
        match conn
            .tx
            .send(NodeControlMessage::ConnectionDisconnect {
                cid: cid_str.clone(),
            })
            .await
        {
            Ok(..) => info!("notified node {node_id} about client {cid_str} disconnect"),
            Err(err) => {
                warn!("failed to notify node {node_id} about client {cid_str} disconnect: {err}")
            }
        }
    }
}
