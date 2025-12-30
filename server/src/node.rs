use crate::connection::NodeConnection;
use crate::env;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum_client_ip::ClientIp;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use phirepass_common::protocol::common::{Frame, FrameData, FrameEncoding};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::stats::Stats;
use std::net::IpAddr;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use ulid::Ulid;
use phirepass_common::protocol::web::WebFrameData;

pub(crate) async fn ws_node_handler(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let ip = phirepass_common::ip::extract_ip_from_headers(&headers).unwrap_or(ip);
    ws.on_upgrade(move |socket| handle_node_socket(socket, state, ip))
}

async fn handle_node_socket(socket: WebSocket, state: AppState, ip: IpAddr) {
    let id = Ulid::new();
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Bounded channel to avoid unbounded memory growth if the node socket is back-pressured.
    let (tx, mut rx) = mpsc::channel::<NodeFrameData>(256);

    {
        let mut nodes = state.nodes.write().await;
        nodes.insert(id, NodeConnection::new(ip, tx.clone()));
        let total = nodes.len();
        info!("node {id} ({ip}) connected (total: {total})", id = id);
    }

    let write_task = tokio::spawn(async move {
        while let Some(node_frame) = rx.recv().await {
            let frame = Frame {
                version: Frame::version(),
                encoding: FrameEncoding::JSON,
                data: node_frame.into(),
            };

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

    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(_) => {
                disconnect_node(&state, id).await;
                return;
            }
        };

        match msg {
            Message::Binary(data) => {
                let frame = match Frame::decode(&data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        warn!("received malformed frame: {err}");
                        break;
                    }
                };

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
                    NodeFrameData::Auth { token } => {
                        info!("node {id} is asking to be authenticated");

                        let resp = NodeFrameData::AuthResponse {
                            nid: id.to_string(),
                            success: true,
                            version: env::version().to_string(),
                        };

                        if let Err(err) = tx.send(resp).await {
                            warn!("failed to respond to node {id}: {err}");
                        } else {
                            info!("auth response sent {id}");
                        }
                    }
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
                    NodeFrameData::TunnelOpened {
                        protocol,
                        cid,
                        sid,
                        msg_id,
                    } => {
                        handle_tunnel_opened(&state, protocol, cid.as_str(), sid, &id, msg_id).await;
                    }
                    NodeFrameData::Frame { frame, cid } => {

                    }
                    _ => todo!(),
                }
            }
            /*
            Message::Binary(data) => match decode_node_control(&data) {
                Ok(msg) => match msg {
                    NodeControlMessage::Auth { .. } => {
                        //
                    }
                    NodeControlMessage::Heartbeat { stats } => {
                        //
                    }
                    NodeControlMessage::Frame { frame, cid } => {
                        // frames are sent by daemon directly to connections via server
                        // TODO: check if frame comes from authenticated daemons
                        handle_frame_response(&state, frame, id.to_string(), cid).await;
                    }
                    NodeControlMessage::Ping { sent_at } => {
                        //
                    }
                    NodeControlMessage::TunnelOpened { protocol, cid, sid } => {
                        //
                    }
                    _ => {}
                },
                Err(err) => warn!("failed to decode node control: {}", err),
            },*/
            Message::Close(err) => {
                match err {
                    None => warn!("node {id} disconnected"),
                    Some(err) => warn!("node {id} disconnected: {:?}", err),
                }
                disconnect_node(&state, id).await;
                return;
            }
            _ => {
                info!("unknown message: {:?}", msg);
            }
        }
    }

    disconnect_node(&state, id).await;
    write_task.abort();
}

/*
async fn handle_frame_response(state: &AppState, frame: Frame, nid: String, cid: String) {
    debug!("node {nid} is asking to send a frame directly to user {cid}");

    let Ok(cid_as_str) = Ulid::from_string(cid.as_str()) else {
        warn!("{cid} is not a valid format");
        return;
    };

    let connections = state.connections.read().await;
    if let Some(conn) = connections.get(&cid_as_str) {
        match conn.tx.send(frame).await {
            Ok(..) => debug!("frame response sent to connection {cid_as_str}"),
            Err(err) => warn!("failed to send frame to user({}): {}", cid_as_str, err),
        }
    }
}*/

async fn handle_tunnel_opened(
    state: &AppState,
    protocol: u8,
    cid: &str,
    sid: u64,
    node_id: &Ulid,
    msg_id: Option<u64>,
) {
    debug!("handling tunnel opened for connection {cid} with session {sid}");
    let cid = Ulid::from_str(cid).unwrap();

    let connections = state.connections.read().await;
    let Some(connection) = connections.get(&cid) else {
        warn!("connection {cid} not found");
        return;
    };

    {
        let key = format!("{node_id}-{sid}");
        let mut tunnel_sessions = state.tunnel_sessions.write().await;
        tunnel_sessions.insert(key, (cid, node_id.clone()));
    }

    match connection.tx.send(WebFrameData::TunnelOpened {
        protocol,
        sid,
        msg_id,
    }).await {
        Ok(..) => info!("tunnel opened notification sent to web client {cid}"),
        Err(err) => warn!("failed to send tunnel opened to client {cid}: {err}")
    }
}

async fn disconnect_node(state: &AppState, id: Ulid) {
    let mut nodes = state.nodes.write().await;
    if let Some(info) = nodes.remove(&id) {
        let alive = info.node.connected_at.elapsed();
        info!(
            "node {id} ({}) removed after {:.1?} (total: {})",
            info.node.ip,
            alive,
            nodes.len()
        );
    }
}

async fn update_node_heartbeat(state: &AppState, id: &Ulid, stats: Option<Stats>) {
    let mut nodes = state.nodes.write().await;
    if let Some(info) = nodes.get_mut(id) {
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
