use crate::env::{Env, SSHAuthMethod};
use crate::sftp::connection::{SFTPConfig, SFTPConfigAuth, SFTPConnection};
use crate::sftp::session::{SFTPCommand, SFTPSessionHandle};
use crate::sftp::{SFTPActiveDownloads, SFTPActiveUploads};
use crate::ssh::connection::{SSHConfig, SSHConfigAuth, SSHConnection};
use crate::ssh::session::{SSHCommand, SSHSessionHandle};
use anyhow::anyhow;
use futures_util::stream::SplitStream;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use phirepass_common::env::Mode;
use phirepass_common::protocol::Protocol;
use phirepass_common::protocol::common::{Frame, FrameData, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::web::WebFrameData;
use phirepass_common::stats::Stats;
use phirepass_common::time::now_millis;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};
use ulid::Ulid;

type TunnelSessions = Arc<Mutex<HashMap<(Ulid, u32), SessionHandle>>>;

type WebSocketReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

static SESSION_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
enum SessionHandle {
    SSH(SSHSessionHandle),
    SFTP(SFTPSessionHandle),
}

impl Display for SessionHandle {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionHandle::SSH(_) => write!(f, "SSHSessionHandle"),
            SessionHandle::SFTP(_) => write!(f, "SFTPSessionHandle"),
        }
    }
}

enum SessionCommand {
    SSH(Sender<SSHCommand>),
    SFTP(Sender<SFTPCommand>),
}

impl SessionHandle {
    pub fn get_stdin(&self) -> SessionCommand {
        match self {
            SessionHandle::SSH(ssh_handle) => SessionCommand::SSH(ssh_handle.stdin.clone()),
            SessionHandle::SFTP(sftp_handle) => SessionCommand::SFTP(sftp_handle.stdin.clone()),
        }
    }

    pub async fn shutdown(self) {
        match self {
            SessionHandle::SSH(ssh_handle) => {
                info!("shutting down ssh handle");
                ssh_handle.shutdown().await;
            }
            SessionHandle::SFTP(sftp_handle) => {
                info!("shutting down sftp handle");
                sftp_handle.shutdown().await;
            }
        }
    }
}

pub(crate) struct WebSocketConnection {
    writer: Sender<Frame>,
    reader: Receiver<Frame>,
    sessions: TunnelSessions,
    uploads: SFTPActiveUploads,
    downloads: SFTPActiveDownloads,
}

fn generate_server_endpoint(mode: Mode, server_host: String, server_port: u16) -> String {
    match mode {
        Mode::Development => {
            if server_port == 80 {
                format!("ws://{}", server_host)
            } else {
                format!("ws://{}:{}", server_host, server_port)
            }
        }
        Mode::Production => {
            if server_port == 443 {
                format!("wss://{}", server_host)
            } else {
                format!("wss://{}:{}", server_host, server_port)
            }
        }
    }
}

impl WebSocketConnection {
    pub fn new() -> Self {
        // Cap the outbound queue to avoid unbounded memory use when the socket is back-pressured.
        let (tx, rx) = channel::<Frame>(1024);
        Self {
            reader: rx,
            writer: tx,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            uploads: Arc::new(Mutex::new(HashMap::new())),
            downloads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn connect(self, config: Arc<Env>) -> anyhow::Result<()> {
        info!("connecting ws...");

        let endpoint = format!(
            "{}/api/nodes/ws",
            generate_server_endpoint(
                config.mode.clone(),
                config.server_host.to_string(),
                config.server_port,
            )
        );

        info!("trying {endpoint}");

        let (stream, _) = connect_async(endpoint).await?;
        let (mut write, mut read) = stream.split();

        let frame: Frame = NodeFrameData::Auth {
            token: config.token.clone(),
        }
        .into();

        write
            .send(Message::Binary(frame.to_bytes()?.into()))
            .await?;

        let (node_id, version) = read_auth_response(&mut read).await?;
        info!("daemon authenticated successfully {node_id} with server version {version}");
        // todo: proper authentication
        // todo: compare version for system compatibility

        let mut rx = self.reader;
        let write_task = tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if let Ok(data) = frame.to_bytes() {
                    if let Err(err) = write.send(Message::Binary(data.into())).await {
                        warn!("failed to send frame: {}", err);
                    }
                }
            }
        });

        let reader_task = spawn_reader_task(
            &node_id,
            read,
            self.writer.clone(),
            config.clone(),
            self.sessions.clone(),
            self.uploads.clone(),
            self.downloads.clone(),
        )
        .await;

        let heartbeat_task =
            spawn_heartbeat_task(self.writer.clone(), config.stats_refresh_interval as u64).await;

        let ping_task = spawn_ping_task(self.writer.clone(), config.ping_interval as u64).await;

        tokio::select! {
            _ = ping_task => warn!("ping task ended"),
            _ = write_task => warn!("write task ended"),
            _ = reader_task => warn!("read task ended"),
            _ = heartbeat_task => warn!("heartbeat task ended"),
        }

        Ok(())
    }
}

async fn spawn_reader_task(
    target: &String,
    mut reader: WebSocketReader,
    sender: Sender<Frame>,
    config: Arc<Env>,
    sessions: TunnelSessions,
    uploads: SFTPActiveUploads,
    downloads: SFTPActiveDownloads,
) -> tokio::task::JoinHandle<()> {
    let target = target.clone();
    tokio::spawn(async move {
        while let Some(frame) = reader.next().await {
            match frame {
                Ok(Message::Binary(data)) => {
                    let frame = match Frame::decode(&data) {
                        Ok(frame) => frame,
                        Err(err) => {
                            warn!("received malformed frame: {err}");
                            return;
                        }
                    };

                    let data = match frame.data {
                        FrameData::Node(data) => data,
                        FrameData::Web(_) => {
                            warn!("received web frame, but expected a node frame");
                            return;
                        }
                    };

                    debug!("received node frame: {data:?}");

                    handle_message(
                        &target, data, &sender, &config, &sessions, &uploads, &downloads,
                    )
                    .await;
                }
                Ok(Message::Close(reason)) => {
                    info!("received close message: {reason:?}");
                    break;
                }
                Err(err) => error!("error receiving frame: {err:?}"),
                _ => warn!("received unsupported socket frame"),
            }
        }
    })
}

async fn spawn_ping_task(sender: Sender<Frame>, interval: u64) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            interval.tick().await;
            let sent_at = now_millis();
            send_frame_data(&sender, NodeFrameData::Ping { sent_at }).await;
        }
    })
}

async fn spawn_heartbeat_task(sender: Sender<Frame>, interval: u64) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval));
        loop {
            interval.tick().await;

            let Some(stats) = Stats::gather() else {
                warn!("failed to gather stats for heartbeat");
                continue;
            };

            send_frame_data(&sender, NodeFrameData::Heartbeat { stats }).await;
        }
    })
}

async fn read_auth_response(reader: &mut WebSocketReader) -> anyhow::Result<(String, String)> {
    match read_next_frame(reader).await {
        None => anyhow::bail!("failed to read auth response"),
        Some(frame) => {
            let NodeFrameData::AuthResponse {
                node_id,
                success,
                version,
            } = frame
            else {
                anyhow::bail!(
                    "wrong frame type, expected NodeFrameData::Auth, got {:?}",
                    frame
                )
            };

            if !success {
                anyhow::bail!("failed to authenticate node")
            }

            Ok((node_id, version))
        }
    }
}

async fn read_next_frame(reader: &mut WebSocketReader) -> Option<NodeFrameData> {
    match reader.next().await {
        Some(Ok(Message::Binary(data))) => {
            let frame = match Frame::decode(&data) {
                Ok(frame) => frame,
                Err(err) => {
                    warn!("received malformed frame: {err}");
                    return None;
                }
            };

            let node_frame = match frame.data {
                FrameData::Node(data) => data,
                FrameData::Web(_) => {
                    warn!("received web frame, but expected a node frame");
                    return None;
                }
            };

            info!("received node frame: {node_frame:?}");

            return Some(node_frame);
        }
        _ => {}
    }

    None
}

async fn handle_message(
    node_id: &String,
    data: NodeFrameData,
    sender: &Sender<Frame>,
    config: &Arc<Env>,
    sessions: &TunnelSessions,
    uploads: &SFTPActiveUploads,
    downloads: &SFTPActiveDownloads,
) {
    debug!("handling message: {data:?}");

    match data {
        NodeFrameData::OpenTunnel {
            protocol,
            cid,
            username,
            password,
            msg_id,
        } => {
            info!("received open tunnel with protocol {protocol}");
            match Protocol::try_from(protocol) {
                Ok(Protocol::SFTP) => match &config.ssh_auth_mode {
                    SSHAuthMethod::CredentialsPrompt => {
                        start_sftp_tunnel(
                            sender,
                            node_id,
                            cid,
                            config,
                            SFTPConfigAuth::UsernamePassword(username, password),
                            sessions,
                            uploads,
                            downloads,
                            msg_id,
                        )
                        .await;
                    }
                },
                Ok(Protocol::SSH) => match &config.ssh_auth_mode {
                    SSHAuthMethod::CredentialsPrompt => {
                        start_ssh_tunnel(
                            sender,
                            node_id,
                            cid,
                            config,
                            SSHConfigAuth::UsernamePassword(username, password),
                            sessions,
                            msg_id,
                        )
                        .await;
                    }
                },
                Err(err) => warn!("invalid protocol value {protocol}: {err:?}"),
            }
        }
        NodeFrameData::Pong { sent_at } => {
            let now = now_millis();
            let rtt = now.saturating_sub(sent_at);
            info!("received pong; round-trip={}ms (sent_at={sent_at})", rtt);
        }
        NodeFrameData::ConnectionDisconnect { cid } => {
            info!("received connection disconnect for {cid}");
            close_tunnels_for_cid(cid, sessions).await;
            close_uploads_for_cid(cid, uploads).await;
        }
        NodeFrameData::SSHWindowResize {
            cid,
            sid,
            cols,
            rows,
        } => {
            if let Err(err) = send_ssh_forward_resize(cid, sid, cols, rows, sessions).await {
                warn!("failed to forward resize: {err}");
            }
        }
        NodeFrameData::TunnelData {
            cid,
            protocol,
            sid,
            data,
        } => {
            if protocol == Protocol::SSH as u8 {
                if let Err(err) = send_ssh_tunnel_data(cid, sid, data, sessions).await {
                    warn!("failed to forward tunnel data: {err}");
                }
            } else {
                warn!("unsupported tunnel data for {protocol}: {sid:?}");
            }
        }
        NodeFrameData::SFTPList {
            cid,
            path,
            sid,
            msg_id,
        } => {
            if let Err(err) = send_sftp_list_data(cid, sid, path, sessions, msg_id).await {
                warn!("failed to forward sftp list data: {err}");
            }
        }
        NodeFrameData::SFTPUploadStart {
            cid,
            sid,
            msg_id,
            upload,
        } => {
            if let Err(err) = send_sftp_upload_start_data(cid, sid, msg_id, upload, sessions).await
            {
                warn!("failed to forward sftp upload start data: {err}");
            }
        }
        NodeFrameData::SFTPUpload {
            cid,
            sid,
            msg_id,
            chunk,
        } => {
            if let Err(err) = send_sftp_upload_data(cid, sid, msg_id, chunk, sessions).await {
                warn!("failed to forward sftp upload data: {err}");
            }
        }
        NodeFrameData::SFTPDownloadStart {
            cid,
            sid,
            msg_id,
            download,
        } => {
            if let Err(err) =
                send_sftp_download_start_data(cid, sid, msg_id, download, sessions).await
            {
                warn!("failed to forward sftp download start data: {err}");
            }
        }
        NodeFrameData::SFTPDownloadChunkRequest {
            cid,
            sid,
            msg_id,
            download_id,
            chunk_index,
        } => {
            if let Err(err) = send_sftp_download_chunk_request(
                cid,
                sid,
                msg_id,
                download_id,
                chunk_index,
                sessions,
            )
            .await
            {
                warn!("failed to handle sftp download chunk request: {err}");
            }
        }
        NodeFrameData::SFTPDownloadChunk {
            cid,
            sid,
            msg_id,
            chunk,
        } => {
            if let Err(err) =
                send_sftp_download_chunk_data(cid, sid, msg_id, chunk, sessions).await
            {
                warn!("failed to forward sftp download chunk data: {err}");
            }
        }
        NodeFrameData::SFTPDelete {
            cid,
            sid,
            msg_id,
            data,
        } => {
            if let Err(err) = send_sftp_delete_data(cid, sid, msg_id, data, sessions).await {
                warn!("failed to forward sftp delete data: {err}");
            }
        }
        o => warn!("not implemented yet: {o:?}"),
    }
}

async fn send_sftp_list_data(
    cid: Ulid,
    sid: u32,
    path: String,
    sessions: &TunnelSessions,
    msg_id: Option<u32>,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SFTPCommand::List(path, msg_id))
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_sftp_upload_start_data(
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    upload: phirepass_common::protocol::sftp::SFTPUploadStart,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SFTPCommand::UploadStart { upload, msg_id })
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_sftp_upload_data(
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    chunk: phirepass_common::protocol::sftp::SFTPUploadChunk,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SFTPCommand::Upload { chunk, msg_id })
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_sftp_delete_data(
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    data: phirepass_common::protocol::sftp::SFTPDelete,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SFTPCommand::Delete { data, msg_id })
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_sftp_download_start_data(
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    download: phirepass_common::protocol::sftp::SFTPDownloadStart,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SFTPCommand::DownloadStart { download, msg_id })
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_sftp_download_chunk_request(
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    download_id: u32,
    chunk_index: u32,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    let chunk = phirepass_common::protocol::sftp::SFTPDownloadChunk {
        download_id,
        chunk_index,
        chunk_size: 0,
        data: vec![],
    };

    stdin
        .send(SFTPCommand::DownloadChunk { chunk, msg_id })
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_sftp_download_chunk_data(
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    chunk: phirepass_common::protocol::sftp::SFTPDownloadChunk,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SFTP(stdin) = stdin else {
        anyhow::bail!(format!(
            "no sftp tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SFTPCommand::DownloadChunk { chunk, msg_id })
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_ssh_tunnel_data(
    cid: Ulid,
    sid: u32,
    data: Vec<u8>,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let connection_id = cid.clone();

    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {connection_id}"))
    };

    let SessionCommand::SSH(stdin) = stdin else {
        anyhow::bail!(format!(
            "no ssh tunnel found for connection {connection_id}"
        ))
    };

    stdin
        .send(SSHCommand::Data(data))
        .await
        .map_err(|err| anyhow!(err))
}

async fn send_ssh_forward_resize(
    cid: Ulid,
    sid: u32,
    cols: u32,
    rows: u32,
    sessions: &TunnelSessions,
) -> anyhow::Result<()> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&(cid, sid)).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        anyhow::bail!(format!("no session found for connection {cid}"))
    };

    let SessionCommand::SSH(stdin) = stdin else {
        anyhow::bail!(format!(
            "no ssh tunnel found for connection {cid}"
        ))
    };

    stdin
        .send(SSHCommand::Resize { cols, rows })
        .await
        .map_err(|err| anyhow!(err))
}

async fn close_uploads_for_cid(cid: Ulid, uploads: &SFTPActiveUploads) {
    info!("closing uploads for connection {cid}");

    let keys_to_remove: Vec<(Ulid, u32)> = {
        let entries = uploads.lock().await;
        entries
            .iter()
            .filter(|entry| entry.0.0.eq(&cid))
            .map(|entry| entry.0.clone())
            .collect()
    };

    // Remove and shutdown each session
    let mut uploads = uploads.lock().await;
    for key in keys_to_remove {
        info!("removing sftp upload by key {:?}", key);
        if let Some(file_upload) = uploads.remove(&key) {
            let _ = file_upload.sftp_file.sync_all().await;
        }
    }
}

async fn close_tunnels_for_cid(cid: Ulid, sessions: &TunnelSessions) {
    info!("closing tunnels for connection {cid}");

    let keys_to_remove: Vec<(Ulid, u32)> = {
        let sessions = sessions.lock().await;
        sessions
            .iter()
            .filter(|entry| entry.0.0.eq(&cid))
            .map(|entry| entry.0.clone())
            .collect()
    };

    // Remove and shutdown each session
    let mut sessions = sessions.lock().await;
    for key in keys_to_remove {
        info!("removing tunnel by key {:?}", key);
        if let Some(handle) = sessions.remove(&key) {
            handle.shutdown().await;
        }
    }
}

async fn send_frame_data(sender: &Sender<Frame>, data: NodeFrameData) {
    if let Err(err) = sender.send(data.into()).await {
        warn!("failed to send frame: {err}");
    } else {
        debug!("frame response sent");
    }
}

async fn start_sftp_tunnel(
    tx: &Sender<Frame>,
    node_id: &String,
    cid: Ulid,
    config: &Arc<Env>,
    credentials: SFTPConfigAuth,
    sessions: &TunnelSessions,
    uploads: &SFTPActiveUploads,
    downloads: &SFTPActiveDownloads,
    msg_id: Option<u32>,
) {
    let (stdin_tx, stdin_rx) = channel::<SFTPCommand>(512);
    let (stop_tx, stop_rx) = oneshot::channel();
    let sender = tx.clone();
    let cid_for_task = cid.clone();
    let cid_for_connection = cid.clone();
    let session_id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
    // let sessions_for_task = sessions.clone();
    let tx_for_opened = tx.clone();
    let cid_for_opened = cid.clone();
    // let config_for_task = config.clone();
    let node_id_for_task = node_id.clone();

    let conn = SFTPConnection::new(SFTPConfig {
        host: config.ssh_host.clone(),
        port: config.ssh_port,
        credentials,
    });

    info!(
        "connecting sftp for connection {cid_for_task}: {}:{}",
        config.ssh_host, config.ssh_port
    );

    let uploads = uploads.clone();
    let downloads = downloads.clone();
    let _sftp_task = tokio::spawn(async move {
        info!("sftp task started for connection {cid_for_task}");

        send_frame_data(
            &sender,
            NodeFrameData::TunnelOpened {
                protocol: Protocol::SFTP as u8,
                cid: cid_for_task.clone(),
                sid: session_id,
                msg_id,
            },
        )
        .await;

        match conn
            .connect(
                node_id_for_task,
                cid_for_task,
                session_id,
                &sender,
                &uploads,
                &downloads,
                stdin_rx,
                stop_rx,
            )
            .await
        {
            Ok(_) => {
                info!("sftp connection {cid_for_opened} ended");
                send_frame_data(
                    &sender,
                    NodeFrameData::TunnelClosed {
                        protocol: Protocol::SFTP as u8,
                        cid: cid_for_opened,
                        sid: session_id,
                        msg_id,
                    },
                )
                .await;
            }
            Err(err) => {
                warn!("sftp connection error for {cid_for_opened}: {err}");
                send_frame_data(
                    &tx_for_opened,
                    NodeFrameData::WebFrame {
                        sid: session_id,
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: err.to_string(),
                            msg_id,
                        },
                    },
                )
                .await;
            }
        }
    });

    let handle = SessionHandle::SFTP(SFTPSessionHandle {
        stop: Some(stop_tx),
        stdin: stdin_tx,
    });

    info!("sftp session handle {session_id} created");

    let previous = {
        let mut sessions = sessions.lock().await;
        sessions.insert((cid_for_connection.clone(), session_id), handle)
    };

    if let Some(prev) = previous {
        info!("removing previous sftp session {cid_for_connection}");
        prev.shutdown().await;
    }
}

async fn start_ssh_tunnel(
    tx: &Sender<Frame>,
    node_id: &String,
    cid: Ulid,
    config: &Arc<Env>,
    credentials: SSHConfigAuth,
    sessions: &TunnelSessions,
    msg_id: Option<u32>,
) {
    let (stdin_tx, stdin_rx) = channel::<SSHCommand>(512);
    let (stop_tx, stop_rx) = oneshot::channel();
    let sender = tx.clone();
    let cid_for_task = cid.clone();
    let cid_for_connection = cid.clone();
    let session_id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
    // let sessions_for_task = sessions.clone();
    let tx_for_opened = tx.clone();
    let cid_for_opened = cid.clone();
    // let config_for_task = config.clone();
    let node_id_for_task = node_id.clone();

    let conn = SSHConnection::new(SSHConfig {
        host: config.ssh_host.clone(),
        port: config.ssh_port,
        credentials,
    });

    info!(
        "connecting ssh for connection {cid_for_task}: {}:{}",
        config.ssh_host, config.ssh_port
    );

    let _ssh_task = tokio::spawn(async move {
        info!("ssh task started for connection {cid_for_task}");

        send_frame_data(
            &sender,
            NodeFrameData::TunnelOpened {
                protocol: Protocol::SSH as u8,
                cid: cid_for_task.clone(),
                sid: session_id,
                msg_id,
            },
        )
        .await;

        match conn
            .connect(
                node_id_for_task,
                cid_for_task,
                session_id,
                &sender,
                stdin_rx,
                stop_rx,
            )
            .await
        {
            Ok(_) => {
                info!("ssh connection {cid_for_opened} ended");
                send_frame_data(
                    &sender,
                    NodeFrameData::TunnelClosed {
                        protocol: Protocol::SSH as u8,
                        cid: cid_for_opened,
                        sid: session_id,
                        msg_id,
                    },
                )
                .await;
            }
            Err(err) => {
                warn!("ssh connection error for {cid_for_opened}: {err}");
                send_frame_data(
                    &tx_for_opened,
                    NodeFrameData::WebFrame {
                        sid: session_id,
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: err.to_string(),
                            msg_id,
                        },
                    },
                )
                .await;
            }
        }
    });

    let handle = SessionHandle::SSH(SSHSessionHandle {
        stop: Some(stop_tx),
        stdin: stdin_tx,
    });

    info!("ssh session handle {session_id} created");

    let previous = {
        let mut sessions = sessions.lock().await;
        sessions.insert((cid_for_connection.clone(), session_id), handle)
    };

    if let Some(prev) = previous {
        info!("removing previous ssh session {cid_for_connection}");
        prev.shutdown().await;
    }
}
