use crate::env::Env;
use crate::sftp2;
// use crate::sftp::{SFTPCommand, SFTPConfig, SFTPConfigAuth, SFTPConnection, SFTPSessionHandle};
use crate::ssh::{SSHCommand, SSHConfig, SSHConfigAuth, SSHConnection, SSHSessionHandle};
use anyhow::anyhow;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use log::{debug, info, warn};
use phirepass_common::env::Mode;
use phirepass_common::protocol::{
    NodeControlMessage, Protocol, WebControlMessage, decode_node_control, encode_node_control,
    encode_web_control_to_frame, generic_web_error,
};
use phirepass_common::stats::Stats;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};

type WebSocketReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

type WebSocketWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

static SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct WebSocketConnection {
    writer: Sender<Vec<u8>>,
    reader: Receiver<Vec<u8>>,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    sftp_sessions: Arc<Mutex<HashMap<String, sftp2::SessionHandle>>>,
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
        let (tx, rx) = channel::<Vec<u8>>(1024);
        Self {
            reader: rx,
            writer: tx,
            ssh_sessions: Arc::new(Mutex::new(HashMap::new())),
            sftp_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn connect(self, config: Arc<Env>) -> anyhow::Result<()> {
        info!("connecting ws...");
        // let mut connection_id: Option<String> = None;

        let ping_interval = config.ping_interval;
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
        let (mut writer, mut reader) = stream.split();

        let _ = write_next_auth(&mut writer, config.token.clone()).await?;
        info!("daemon sent auth request with token");

        let (cid, version) = read_next_auth_response(&mut reader).await?;
        info!("daemon authenticated successfully {cid} with server version {version}");

        let tx_writer = self.writer.clone();
        let hb_writer = self.writer.clone();
        let mut rx = self.reader;
        let ssh_sessions = self.ssh_sessions.clone();
        let sftp_sessions = self.sftp_sessions.clone();

        let write_task = tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if let Err(err) = writer.send(Message::Binary(frame.into())).await {
                    warn!("failed to send frame: {}", err);
                    break;
                }
            }
        });

        debug!("writer setup");

        let reader_task = tokio::spawn(async move {
            while let Some(msg) = reader.next().await {
                match msg {
                    Ok(Message::Binary(data)) => match decode_node_control(&data) {
                        Ok(msg) => {
                            handle_control_from_server(
                                msg,
                                &tx_writer,
                                ssh_sessions.clone(),
                                sftp_sessions.clone(),
                                config.clone(),
                            )
                            .await
                        }
                        Err(err) => warn!("failed to code node control: {err}"),
                    },
                    Ok(Message::Close(reason)) => {
                        match reason {
                            None => warn!("connection closed"),
                            Some(reason) => warn!("connection closed: {:?}", reason),
                        }
                        break;
                    }
                    Ok(other) => warn!("received unexpected message: {:?}", other),
                    Err(err) => {
                        warn!("failed to read frame: {}", err);
                        break;
                    }
                }
            }
        });

        debug!("reader setup");

        let heartbeat_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;

                let Some(stats) = Stats::gather() else {
                    warn!("failed to gather stats for heartbeat");
                    continue;
                };

                match encode_node_control(&NodeControlMessage::Heartbeat { stats }) {
                    Ok(raw) => {
                        if hb_writer.send(raw).await.is_err() {
                            warn!("failed to queue heartbeat: channel closed");
                            break;
                        }
                    }
                    Err(err) => warn!("failed to encode heartbeat: {}", err),
                }
            }
        });

        let ping_tx = self.writer.clone();
        let ping_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(ping_interval as u64));
            loop {
                interval.tick().await;

                let sent_at = now_millis();
                match encode_node_control(&NodeControlMessage::Ping { sent_at }) {
                    Ok(raw) => {
                        if ping_tx.send(raw).await.is_err() {
                            warn!("failed to queue ping: channel closed");
                            break;
                        }
                        info!("ping sent at {sent_at}");
                    }
                    Err(err) => warn!("failed to encode ping: {err}"),
                }
            }
        });

        info!("connected");

        tokio::select! {
            _ = ping_task => warn!("ping task ended"),
            _ = write_task => warn!("write task ended"),
            _ = reader_task => warn!("read task ended"),
            _ = heartbeat_task => warn!("heartbeat task ended"),
        }

        Ok(())
    }
}

async fn write_next_auth(writer: &mut WebSocketWriter, token: String) -> anyhow::Result<()> {
    let auth_frame = encode_node_control(&NodeControlMessage::Auth { token })?;

    writer
        .send(Message::Binary(auth_frame.into()))
        .await
        .map_err(Into::into)
}

async fn read_next_auth_response(reader: &mut WebSocketReader) -> anyhow::Result<(String, String)> {
    if let Some(msg) = read_next_control(reader).await? {
        if let NodeControlMessage::AuthResponse {
            cid,
            version,
            success,
        } = msg
        {
            if success {
                Ok((cid, version))
            } else {
                anyhow::bail!("daemon failed to authenticated")
            }
        } else {
            anyhow::bail!("unexpected authentication response")
        }
    } else {
        anyhow::bail!("failed to read next control message")
    }
}

async fn read_next_control(
    reader: &mut WebSocketReader,
) -> anyhow::Result<Option<NodeControlMessage>> {
    while let Some(msg) = reader.next().await {
        match msg {
            Ok(Message::Binary(data)) => match decode_node_control(&data) {
                Ok(msg) => return Ok(Some(msg)),
                Err(err) => warn!("failed to decode node control: {err}"),
            },
            Ok(Message::Close(reason)) => {
                return Err(anyhow!("connection closed: {:?}", reason));
            }
            Ok(other) => warn!("received unexpected message: {:?}", other),
            Err(err) => return Err(anyhow!("failed to read frame: {}", err)),
        }
    }

    Ok(None)
}

async fn handle_control_from_server(
    msg: NodeControlMessage,
    tx: &Sender<Vec<u8>>,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    sftp_sessions: Arc<Mutex<HashMap<String, sftp2::SessionHandle>>>,
    config: Arc<Env>,
) {
    debug!("received control message from server");

    match msg {
        NodeControlMessage::OpenTunnel {
            protocol,
            cid,
            username,
            password,
        } => {
            info!("received open tunnel with protocol {:?}", protocol);
            match Protocol::try_from(protocol) {
                Ok(Protocol::SSH) => {
                    open_ssh_tunnel(tx, cid, ssh_sessions.clone(), config, username, password).await
                }
                Ok(Protocol::SFTP) => {
                    open_sftp_tunnel(tx, cid, sftp_sessions.clone(), config, username, password)
                        .await
                }
                Ok(protocol) => warn!("unsupported protocol for tunnel: {}", protocol),
                Err(err) => warn!("invalid protocol value {}: {:?}", protocol, err),
            }
        }
        NodeControlMessage::ConnectionDisconnect { cid } => {
            close_ssh_tunnel(cid, ssh_sessions.clone()).await;
        }
        NodeControlMessage::Resize { cid, cols, rows } => {
            if let Err(err) = forward_resize(ssh_sessions.clone(), cid, cols, rows).await {
                warn!("failed to forward resize: {err}");
            }
        }
        NodeControlMessage::TunnelData {
            protocol,
            cid,
            data,
        } => {
            if protocol == Protocol::SSH as u8 {
                // a message from user -> server -> daemon
                // we need to handle this and respond
                if let Err(err) = handle_ssh_tunnel_data(cid, data, ssh_sessions.clone()).await {
                    warn!("{err}");
                }
            } else {
                warn!("unsupported tunnel protocol {protocol} for connection {cid}");
            }
        }
        NodeControlMessage::Ping { sent_at } => {
            let now = now_millis();
            let latency = now.saturating_sub(sent_at);
            info!("received ping from server; latency={}ms", latency);
            let pong = NodeControlMessage::Pong { sent_at: now };
            match encode_node_control(&pong) {
                Ok(raw) => {
                    if tx.send(raw).await.is_err() {
                        warn!("failed to queue pong: channel closed");
                    }
                }
                Err(err) => warn!("failed to encode pong: {err}"),
            }
        }
        NodeControlMessage::Pong { sent_at } => {
            let now = now_millis();
            let rtt = now.saturating_sub(sent_at);
            info!("received pong; round-trip={}ms (sent_at={sent_at})", rtt);
        }
        o => warn!("received unsupported control message: {:?}", o),
    }
}

async fn open_sftp_tunnel(
    tx: &Sender<Vec<u8>>,
    cid: String,
    sftp_sessions: Arc<Mutex<HashMap<String, sftp2::SessionHandle>>>,
    config: Arc<Env>,
    username: String,
    password: String,
) {
    info!("opening sftp tunnel for connection {cid}...");

    match &config.ssh_auth_mode {
        crate::env::SSHAuthMethod::CredentialsPrompt => {
            sftp2::open_sftp_tunnel(
                cid,
                tx.clone(),
                config,
                Arc::new(sftp2::AuthConfig::UsernamePassword(
                    username,
                    password,
                )),
                sftp_sessions,
            )
            .await;
        }
    }
}

async fn open_ssh_tunnel(
    tx: &Sender<Vec<u8>>,
    cid: String,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    config: Arc<Env>,
    username: String,
    password: String,
) {
    info!("opening ssh tunnel for connection {cid}...");

    match &config.ssh_auth_mode {
        crate::env::SSHAuthMethod::CredentialsPrompt => {
            start_ssh_tunnel(
                tx,
                cid,
                ssh_sessions,
                config,
                SSHConfigAuth::UsernamePassword(username, password),
            )
            .await;
        }
    }
}

async fn start_ssh_tunnel(
    tx: &Sender<Vec<u8>>,
    cid: String,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    config: Arc<Env>,
    credentials: SSHConfigAuth,
) {
    let (stdin_tx, stdin_rx) = channel::<SSHCommand>(512);
    let (stop_tx, stop_rx) = oneshot::channel();
    let sender = tx.clone();
    let cid_for_task = cid.clone();
    let cid_for_connection = cid.clone();
    let session_id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let ssh_sessions_for_task = ssh_sessions.clone();

    let ssh_task = tokio::spawn(async move {
        info!("ssh task started for connection {cid_for_task}");

        let conn = SSHConnection::new(SSHConfig {
            host: config.ssh_host.clone(),
            port: config.ssh_port,
            credentials,
        });

        match conn
            .connect(cid_for_task.clone(), &sender, stdin_rx, stop_rx)
            .await
        {
            Ok(()) => {
                info!("ssh connection {cid_for_task} ended");

                if let Err(err) = send_ssh_data_to_connection(
                    &sender,
                    ssh_sessions_for_task.clone(),
                    cid.as_str(),
                    &WebControlMessage::TunnelClosed {
                        protocol: Protocol::SSH as u8,
                    },
                )
                .await
                {
                    warn!("failed to notify cid {cid_for_task} for ssh connection closure: {err}");
                }
            }
            Err(err) => {
                warn!("ssh connection error for {cid_for_task}: {err}");
                if let Err(err) = send_ssh_data_to_connection(
                    &sender,
                    ssh_sessions_for_task.clone(),
                    cid.as_str(),
                    &generic_web_error(
                        Protocol::SSH as u8,
                        "SSH authentication failed. Please check your password.",
                    ),
                )
                .await
                {
                    warn!("failed to notify connection {cid} about authentication failure: {err}");
                }
            }
        }
    });

    let sessions_for_cleanup = ssh_sessions.clone();
    let cid_for_cleanup = cid_for_connection.clone();
    let cleanup_task = tokio::spawn(async move {
        if let Err(err) = ssh_task.await {
            warn!("ssh session join error for {cid_for_cleanup}: {err}");
        }

        let mut sessions = sessions_for_cleanup.lock().await;
        let should_remove = sessions
            .get(&cid_for_cleanup)
            .map(|handle| handle.id == session_id)
            .unwrap_or(false);

        if should_remove {
            sessions.remove(&cid_for_cleanup);
        }
    });

    let handle = SSHSessionHandle {
        id: session_id,
        stop: Some(stop_tx),
        join: cleanup_task,
        stdin: stdin_tx,
    };

    let previous = {
        let mut sessions = ssh_sessions.lock().await;
        sessions.insert(cid_for_connection, handle)
    };

    if let Some(prev) = previous {
        prev.shutdown().await;
    }
}

async fn close_ssh_tunnel(
    cid: String,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
) {
    let handle = {
        let mut sessions = ssh_sessions.lock().await;
        sessions.remove(&cid)
    };

    match handle {
        Some(handle) => {
            info!("closing ssh tunnel for connection {cid}");
            handle.shutdown().await;
        }
        None => info!("no ssh tunnel to close for connection {cid}"),
    }
}

async fn handle_ssh_tunnel_data(
    cid: String,
    data: Vec<u8>,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
) -> anyhow::Result<(), String> {
    // find the correct handle
    let stdin = {
        let sessions = ssh_sessions.lock().await;
        sessions.get(&cid).map(|s| s.stdin.clone())
    };

    // unwrap found handle
    let Some(stdin) = stdin else {
        return Err(format!("no ssh tunnel found for connection {cid}"));
    };

    // forrward data to handle
    stdin
        .send(SSHCommand::Data(data))
        .await
        .map_err(|err| format!("failed to queue data to ssh tunnel for {cid}: {err}"))
}

async fn forward_resize(
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    cid: String,
    cols: u32,
    rows: u32,
) -> anyhow::Result<(), String> {
    let stdin = {
        let sessions = ssh_sessions.lock().await;
        sessions.get(&cid).map(|s| s.stdin.clone())
    };

    let Some(stdin) = stdin else {
        return Err(format!("no ssh tunnel found for connection {cid}"));
    };

    stdin
        .send(SSHCommand::Resize { cols, rows })
        .await
        .map_err(|err| format!("failed to queue resize to ssh tunnel for {cid}: {err}"))
}

async fn _send_sftp_data_to_connection(
    tx: &Sender<Vec<u8>>,
    sftp_sessions: Arc<Mutex<HashMap<String, sftp2::SessionHandle>>>,
    cid: &str,
    data: &WebControlMessage,
) -> anyhow::Result<()> {
    let frame = encode_web_control_to_frame(data)?;

    let node_msg = NodeControlMessage::Frame {
        frame,
        cid: cid.to_string(),
    };

    let raw = encode_node_control(&node_msg)?;
    tx.send(raw).await.map_err(|err| {
        // Send failures here imply the channel is closed; clean up the SSH tunnel for this cid.
        tokio::spawn(sftp2::close_sftp_tunnel(cid.to_string(), sftp_sessions));
        anyhow!("failed to send data to connection: {err}")
    })
}

async fn send_ssh_data_to_connection(
    tx: &Sender<Vec<u8>>,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    cid: &str,
    data: &WebControlMessage,
) -> anyhow::Result<()> {
    let frame = encode_web_control_to_frame(data)?;

    let node_msg = NodeControlMessage::Frame {
        frame,
        cid: cid.to_string(),
    };

    let raw = encode_node_control(&node_msg)?;
    tx.send(raw).await.map_err(|err| {
        // Send failures here imply the channel is closed; clean up the SSH tunnel for this cid.
        tokio::spawn(close_ssh_tunnel(cid.to_string(), ssh_sessions));
        anyhow!("failed to send data to connection: {err}")
    })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
