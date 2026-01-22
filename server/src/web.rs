use crate::connection::WebConnection;
use crate::http::AppState;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum_client_ip::ClientIp;
use bytes::Bytes;
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
        state
            .connections
            .insert(cid, WebConnection::new(ip, tx.clone()));
        let total = state.connections.len();
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

    // Handle messages in separate function to ensure cleanup always happens
    handle_web_messages(&mut ws_rx, &state, cid).await;

    // Always abort write task regardless of how we exited message loop
    drop(tx); // Close sender first to wake write task
    write_task.abort();
    disconnect_web_client(&state, cid).await;
}

/// Handles incoming WebSocket messages. Always returns to parent for cleanup.
async fn handle_web_messages(
    ws_rx: &mut futures_util::stream::SplitStream<WebSocket>,
    state: &AppState,
    cid: Ulid,
) {
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
                        update_web_heartbeat(&state, &cid).await;
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
                    WebFrameData::SFTPDownloadStart {
                        sid,
                        node_id,
                        msg_id,
                        download,
                    } => {
                        handle_sftp_download_start(&state, cid, sid, node_id, msg_id, download)
                            .await;
                    }
                    WebFrameData::SFTPDownloadChunkRequest {
                        sid,
                        node_id,
                        msg_id,
                        download_id,
                        chunk_index,
                    } => {
                        handle_sftp_download_chunk_request(
                            &state,
                            cid,
                            sid,
                            node_id,
                            msg_id,
                            download_id,
                            chunk_index,
                        )
                        .await;
                    }
                    WebFrameData::SFTPDownloadChunk { msg_id: _, .. } => {
                        // Download chunks are sent from agent to web client, not web client to agent
                        warn!(
                            "received sftp download chunk which is invalid if sent by web client"
                        );
                        break;
                    }
                    WebFrameData::SFTPUploadStart {
                        sid,
                        node_id,
                        msg_id,
                        upload,
                    } => {
                        handle_sftp_upload_start(&state, cid, sid, node_id, msg_id, upload).await;
                    }
                    WebFrameData::SFTPUpload {
                        sid,
                        node_id,
                        msg_id,
                        chunk,
                    } => {
                        handle_sftp_upload(&state, cid, sid, node_id, msg_id, chunk).await;
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
                    WebFrameData::SFTPUploadChunkAck { .. } => {
                        warn!(
                            "received sftp upload chunk ack which is invalid if sent by web client"
                        );
                        break;
                    }
                    WebFrameData::SFTPUploadStartResponse { .. } => {
                        warn!(
                            "received sftp upload start response which is invalid if sent by web client"
                        );
                        break;
                    }
                    WebFrameData::SFTPDownloadStartResponse { .. } => {
                        warn!(
                            "received sftp download start response which is invalid if sent by web client"
                        );
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
                return; // Cleanup handled by caller
            }
            _ => {
                info!("unknown message: {:?}", msg);
            }
        }
    }
}

async fn disconnect_web_client(state: &AppState, cid: Ulid) {
    if let Some((_, info)) = state.connections.remove(&cid) {
        let alive = info.connected_at.elapsed();
        let total = state.connections.len();
        info!(
            "web client {cid} ({}) removed after {:.1?} (total: {})",
            info.ip, alive, total
        );
    }

    notify_nodes_client_disconnect(state, cid).await;
}

async fn update_web_heartbeat(state: &AppState, cid: &Ulid) {
    if let Some(mut info) = state.connections.get_mut(cid) {
        let since_last = info
            .last_heartbeat
            .elapsed()
            .unwrap_or(Duration::from_secs(0));
        info.last_heartbeat = SystemTime::now();
        info!(
            "heartbeat from web {cid} ({}) after {:.1?}",
            info.ip, since_last
        );
    } else {
        warn!("received heartbeat for unknown web client {cid}");
    }
}

async fn handle_web_tunnel_data(
    state: &AppState,
    cid: Ulid,
    protocol: u8,
    sid: u32,
    node_id: String,
    data: Bytes,
) {
    debug!("tunnel data received: {} bytes", data.len());

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, node_id, sid).await {
        Ok(node_id) => node_id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    if tx
        .send(NodeFrameData::TunnelData {
            cid,
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

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPList {
            cid,
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

async fn handle_sftp_download_start(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    msg_id: Option<u32>,
    download: phirepass_common::protocol::sftp::SFTPDownloadStart,
) {
    debug!("handle sftp download start request");

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPDownloadStart {
            cid,
            sid,
            msg_id,
            download,
        })
        .await
    {
        Ok(_) => info!("sent sftp download start request to {node_id}"),
        Err(err) => warn!("failed to forward sftp download start to node {node_id}: {err}"),
    }
}

async fn handle_sftp_download_chunk_request(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    msg_id: Option<u32>,
    download_id: u32,
    chunk_index: u32,
) {
    debug!("handle sftp download chunk request");

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPDownloadChunkRequest {
            cid,
            sid,
            msg_id,
            download_id,
            chunk_index,
        })
        .await
    {
        Ok(_) => info!("sent sftp download chunk request to {node_id}"),
        Err(err) => {
            warn!("failed to forward sftp download chunk request to node {node_id}: {err}")
        }
    }
}

async fn handle_sftp_upload_start(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    msg_id: Option<u32>,
    upload: phirepass_common::protocol::sftp::SFTPUploadStart,
) {
    debug!("handle sftp upload start request");

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPUploadStart {
            cid,
            sid,
            msg_id,
            upload,
        })
        .await
    {
        Ok(_) => debug!("sent sftp upload start to {node_id}"),
        Err(err) => warn!("failed to forward sftp upload start to node {node_id}: {err}"),
    }
}

async fn handle_sftp_upload(
    state: &AppState,
    cid: Ulid,
    sid: u32,
    target: String,
    msg_id: Option<u32>,
    chunk: phirepass_common::protocol::sftp::SFTPUploadChunk,
) {
    debug!("handle sftp upload request");

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPUpload {
            cid,
            sid,
            msg_id,
            chunk,
        })
        .await
    {
        Ok(_) => debug!("sent sftp upload chunk to {node_id}"),
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

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SFTPDelete {
            cid,
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

    let node_id = match state.get_node_id_by_cid_and_sid(&cid, target, sid).await {
        Ok(id) => id,
        Err(err) => {
            warn!("error getting node id: {err}");
            return;
        }
    };

    let tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = tx else {
        warn!("tx for node not found {node_id}");
        return;
    };

    match tx
        .send(NodeFrameData::SSHWindowResize {
            cid,
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
    target: String,
    msg_id: Option<u32>,
    username: Option<String>,
    password: Option<String>,
) {
    info!("received open tunnel message protocol={protocol} node_id={target}");

    let node_id = match Ulid::from_string(&target) {
        Ok(id) => id,
        Err(err) => {
            warn!("invalid node id {target}: {err}");
            return;
        }
    };

    let node_tx = state.nodes.get(&node_id).map(|info| info.tx.clone());

    let Some(tx) = node_tx else {
        warn!("node not found {node_id}");

        if let Err(err) = state.notify_client_by_cid(
            cid,
            WebFrameData::Error {
                kind: FrameError::Generic,
                message: format!("Node[id={}] could not be found", node_id),
                msg_id,
            },
        )
        .await {
            warn!("error notifying clients by cid on node {node_id}: {err}");
        }

        return;
    };

    info!("notifying agent to open tunnel {protocol}");

    if tx
        .send(NodeFrameData::OpenTunnel {
            protocol,
            cid,
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

async fn notify_nodes_client_disconnect(state: &AppState, cid: Ulid) {
    for entry in state.nodes.iter() {
        let (node_id, conn) = entry.pair();
        match conn
            .tx
            .send(NodeFrameData::ConnectionDisconnect { cid })
            .await
        {
            Ok(..) => info!("notified node {node_id} about client {cid} disconnect"),
            Err(err) => {
                warn!("failed to notify node {node_id} about client {cid} disconnect: {err}")
            }
        }
    }
}
