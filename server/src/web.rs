use crate::connection::WebConnection;
use crate::state::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum_client_ip::ClientIp;
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use phirepass_common::protocol::common::{Frame, FrameData, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::web::WebFrameData;
use std::net::IpAddr;
use std::time::{Duration, SystemTime};
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
    let cid = Ulid::new();
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Bounded channel so slow clients cannot grow memory unbounded.
    let (tx, mut rx) = mpsc::channel::<WebFrameData>(256);

    {
        let mut connections = state.connections.write().await;
        connections.insert(cid, WebConnection::new(ip, tx));
        let total = connections.len();
        info!("connection {cid} ({ip}) established (total: {total})");
    }

    let write_task = tokio::spawn(async move {
        while let Some(web_frame) = rx.recv().await {
            let frame: Frame = web_frame.into();

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
                disconnect_web_client(&state, cid).await;
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

                let web_frame = match frame.data {
                    FrameData::Web(data) => data,
                    FrameData::Node(_) => {
                        warn!("received node frame, but expected a web frame");
                        break;
                    }
                };

                match web_frame {
                    WebFrameData::Heartbeat => {
                        update_web_heartbeat(&state, cid).await;
                    }
                    WebFrameData::OpenTunnel {
                        protocol,
                        node_id: target,
                        msg_id,
                        username,
                        password,
                    } => {
                        handle_web_open_tunnel(
                            &state, cid, protocol, target, msg_id, username, password,
                        )
                        .await;
                    }
                    WebFrameData::TunnelOpened { .. } => {
                        warn!("received tunnel opened frame which is invalid if sent by user");
                        break;
                    }
                    WebFrameData::TunnelData {
                        protocol,
                        sid,
                        node_id,
                        data,
                    } => {
                        handle_web_tunnel_data(&state, cid, protocol, sid, node_id, data).await;
                    }
                    WebFrameData::TunnelClosed { .. } => {
                        warn!(
                            "received tunnel closed frame which is invalid if sent by web client"
                        );
                        break;
                    }
                    WebFrameData::SSHWindowResize {
                        node_id,
                        sid,
                        cols,
                        rows,
                    } => {
                        handle_web_resize(&state, cid, sid, node_id, cols, rows).await;
                    }
                    WebFrameData::SFTPList {
                        path,
                        sid,
                        node_id,
                        msg_id,
                    } => {
                        handle_sftp_list(&state, cid, sid, node_id, path, msg_id).await;
                    }
                    WebFrameData::SFTPDownload {
                        path,
                        filename,
                        sid,
                        node_id,
                        msg_id,
                    } => {
                        handle_sftp_download(&state, cid, sid, node_id, path, filename, msg_id)
                            .await;
                    }
                    WebFrameData::SFTPUpload {
                        path,
                        sid,
                        node_id,
                        msg_id,
                        chunk,
                    } => {
                        handle_sftp_upload(&state, cid, sid, node_id, path, msg_id, chunk).await;
                    }
                    WebFrameData::SFTPDelete {
                        sid,
                        node_id,
                        msg_id,
                        data,
                    } => {
                        handle_sftp_delete(&state, cid, sid, node_id, msg_id, data).await;
                    }
                    WebFrameData::SFTPListItems { .. } => {
                        warn!("received sftp list items which is invalid if sent by web client");
                        break;
                    }
                    WebFrameData::SFTPFileChunk { .. } => {
                        warn!("received sftp file chunk which is invalid if sent by web client");
                        break;
                    }
                    WebFrameData::Error { .. } => {
                        warn!("received error frame which is invalid if sent by web client");
                        break;
                    }
                }
            }
            Message::Close(err) => {
                match err {
                    None => warn!("web client {cid} disconnected"),
                    Some(err) => warn!("web client {cid} disconnected: {:?}", err),
                }
                disconnect_web_client(&state, cid).await;
                return;
            }
            _ => {
                info!("unknown message: {:?}", msg);
            }
        }
    }

    disconnect_web_client(&state, cid).await;
    write_task.abort();
}

async fn disconnect_web_client(state: &AppState, cid: Ulid) {
    let mut connections = state.connections.write().await;
    if let Some(info) = connections.remove(&cid) {
        let alive = info.connected_at.elapsed();
        info!(
            "web client {cid} ({}) removed after {:.1?} (total: {})",
            info.ip,
            alive,
            connections.len()
        );
    }

    notify_nodes_client_disconnect(state, cid).await;
}

async fn update_web_heartbeat(state: &AppState, id: Ulid) {
    let mut connections = state.connections.write().await;
    if let Some(info) = connections.get_mut(&id) {
        let since_last = info
            .last_heartbeat
            .elapsed()
            .unwrap_or(Duration::from_secs(0));
        info.last_heartbeat = SystemTime::now();
        info!(
            "heartbeat from web {id} ({}) after {:.1?}",
            info.ip, since_last
        );
    } else {
        warn!("received heartbeat for unknown web client {id}");
    }
}

async fn get_node_id_by_cid(
    state: &AppState,
    cid: &Ulid,
    node_id: String,
    sid: u32,
) -> anyhow::Result<Ulid> {
    let key = format!("{}-{}", node_id, sid);
    let sessions = state.tunnel_sessions.read().await;
    let (client_id, node_id) = match sessions.get(&key) {
        Some((client_id, node_id)) => (client_id, node_id),
        _ => {
            anyhow::bail!("node not found for session id {sid}")
        }
    };

    if !client_id.eq(&cid) {
        anyhow::bail!("correct cid was not found for sid {sid}")
    }

    Ok(*node_id)
}

async fn handle_web_tunnel_data(
    state: &AppState,
    cid: Ulid,
    protocol: u8,
    sid: u32,
    node_id: String,
    data: Vec<u8>,
) {
    debug!("tunnel data received: {} bytes", data.len());

    let node_id = match get_node_id_by_cid(state, &cid, node_id, sid).await {
        Ok(node_id) => node_id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    if tx
        .send(NodeFrameData::TunnelData {
            cid: cid.to_string(),
            protocol,
            sid,
            data,
        })
        .await
        .is_err()
    {
        warn!("failed to forward tunnel data to node {node_id}");
    } else {
        debug!("forwarded tunnel data to node {node_id}");
    }
}

async fn handle_sftp_list(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    path: String,
    msg_id: Option<u32>,
) {
    debug!("handle sftp list request");

    let node_id = match get_node_id_by_cid(state, &cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPList {
            cid: cid.to_string(),
            path,
            sid,
            msg_id,
        })
        .await
    {
        Ok(_) => info!("sent sftp list to {node_id}"),
        Err(err) => warn!("failed to forward sftp list to node {node_id}: {err}"),
    }
}

async fn handle_sftp_download(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    path: String,
    filename: String,
    msg_id: Option<u32>,
) {
    debug!("handle sftp download request");

    let node_id = match get_node_id_by_cid(state, &cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPDownload {
            cid: cid.to_string(),
            path,
            filename,
            sid,
            msg_id,
        })
        .await
    {
        Ok(_) => info!("sent sftp download request to {node_id}"),
        Err(err) => warn!("failed to forward sftp download to node {node_id}: {err}"),
    }
}

async fn handle_sftp_upload(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    path: String,
    msg_id: Option<u32>,
    chunk: phirepass_common::protocol::sftp::SFTPUploadChunk,
) {
    debug!("handle sftp upload request");

    let node_id = match get_node_id_by_cid(state, &cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPUpload {
            cid: cid.to_string(),
            path,
            sid,
            msg_id,
            chunk,
        })
        .await
    {
        Ok(_) => info!("sent sftp upload chunk to {node_id}"),
        Err(err) => warn!("failed to forward sftp upload to node {node_id}: {err}"),
    }
}

async fn handle_sftp_delete(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    msg_id: Option<u32>,
    data: phirepass_common::protocol::sftp::SFTPDelete,
) {
    debug!("handle sftp delete request");

    let node_id = match get_node_id_by_cid(state, &cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPDelete {
            cid: cid.to_string(),
            sid,
            msg_id,
            data,
        })
        .await
    {
        Ok(_) => info!("sent sftp delete request to {node_id}"),
        Err(err) => warn!("failed to forward sftp delete to node {node_id}: {err}"),
    }
}

async fn handle_web_resize(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    cols: u32,
    rows: u32,
) {
    debug!("tunnel ssh resize received");

    let node_id = match get_node_id_by_cid(state, &cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SSHWindowResize {
            cid: cid.to_string(),
            sid,
            cols,
            rows,
        })
        .await
    {
        Ok(_) => info!("sent ssh window resize to {node_id}"),
        Err(err) => warn!("failed to forward resize to node {node_id}: {err}"),
    }
}

async fn handle_web_open_tunnel(
    state: &AppState,
    cid: Ulid,
    protocol: u8,
    node: String,
    msg_id: Option<u32>,
    username: Option<String>,
    password: Option<String>,
) {
    info!(
        "received open tunnel message protocol={:?} target={:?}",
        protocol, node
    );

    let node_id = match Ulid::from_string(&node) {
        Ok(id) => id,
        Err(err) => {
            warn!("invalid node id {node}: {err}");
            return;
        }
    };

    let node_tx = {
        let nodes = state.nodes.read().await;
        nodes.get(&node_id).map(|info| info.tx.clone())
    };

    let Some(tx) = node_tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    let Some(username) = username else {
        warn!("username not found");
        let _ = send_requires_username_password_error(&state, cid, msg_id).await;
        return;
    };

    let Some(password) = password else {
        warn!("password not found");
        let _ = send_requires_password_error(&state, cid, msg_id).await;
        return;
    };

    if tx
        .send(NodeFrameData::OpenTunnel {
            protocol,
            cid: cid.to_string(),
            username,
            password,
            msg_id,
        })
        .await
        .is_err()
    {
        warn!("failed to forward open tunnel to node {node_id}");
    } else {
        debug!(
            "forwarded open tunnel to node {node_id} (protocol {})",
            protocol
        );
    }
}

async fn send_requires_username_password_error(
    state: &AppState,
    cid: Ulid,
    msg_id: Option<u32>,
) -> anyhow::Result<()> {
    let connections = state.connections.read().await;
    if let Some(wc) = connections.get(&cid) {
        wc.tx
            .send(WebFrameData::Error {
                kind: FrameError::RequiresUsernamePassword,
                message: "Credentials are required".to_string(),
                msg_id,
            })
            .await?;
    } else {
        warn!("failed to find connection {cid}");
    }

    Ok(())
}

async fn send_requires_password_error(
    state: &AppState,
    cid: Ulid,
    msg_id: Option<u32>,
) -> anyhow::Result<()> {
    let connections = state.connections.read().await;
    if let Some(wc) = connections.get(&cid) {
        wc.tx
            .send(WebFrameData::Error {
                kind: FrameError::RequiresPassword,
                message: "Password is required".to_string(),
                msg_id,
            })
            .await?;
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
            .send(NodeFrameData::ConnectionDisconnect {
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
