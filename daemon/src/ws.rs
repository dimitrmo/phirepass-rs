use crate::env::{Env, SSHAuthMethod};
use crate::ssh::{SSHCommand, SSHConfig, SSHConfigAuth, SSHConnection, SSHSessionHandle};
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
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};

type WebSocketReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

static SESSION_ID: AtomicU32 = AtomicU32::new(1);

enum SessionHandle {
    SSH(SSHSessionHandle),
}

impl SessionHandle {
    pub fn get_id(&self) -> u32 {
        match self {
            SessionHandle::SSH(ssh_handle) => ssh_handle.id,
        }
    }

    pub fn get_stdin(&self) -> Sender<SSHCommand> {
        match self {
            SessionHandle::SSH(ssh_handle) => ssh_handle.stdin.clone(),
        }
    }

    pub async fn shutdown(self) {
        match self {
            SessionHandle::SSH(ssh_handle) => {
                ssh_handle.shutdown().await;
            }
        }
    }
}

pub(crate) struct WebSocketConnection {
    writer: Sender<Frame>,
    reader: Receiver<Frame>,
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
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
        )
        .await;

        /*

        let reader_task = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Binary(data)) => {
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

                        info!("received node frame: {node_frame:?}");
                    }
                    /*
                    Ok(Message::Binary(data)) => match decode_node_control(&data) {
                        Ok(msg) => {
                            handle_control_from_server(
                                msg,
                                &tx_writer,
                                config.clone(),
                                sessions.clone(),
                            )
                            .await
                        }
                        Err(err) => warn!("failed to code node control: {err}"),
                    },
                     */
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
        });*/

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
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
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

                    handle_message(&target, data, &sender, &config, &sessions).await;
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
    sessions: &Arc<Mutex<HashMap<String, SessionHandle>>>,
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
                Ok(Protocol::SSH) => {
                    match &config.ssh_auth_mode {
                        SSHAuthMethod::CredentialsPrompt => {
                            start_ssh_tunnel(
                                sender,
                                node_id,
                                &cid,
                                config,
                                SSHConfigAuth::UsernamePassword(username, password),
                                sessions,
                                msg_id,
                            )
                            .await;
                        }
                    }
                }
                Err(err) => warn!("invalid protocol value {}: {:?}", protocol, err),
            }
        }
        NodeFrameData::Pong { sent_at } => {
            let now = now_millis();
            let rtt = now.saturating_sub(sent_at);
            info!("received pong; round-trip={}ms (sent_at={sent_at})", rtt);
        }
        NodeFrameData::ConnectionDisconnect { cid } => {
            close_ssh_tunnel(cid, sessions.clone()).await;
        }
        NodeFrameData::SSHWindowResize {
            cid,
            sid,
            cols,
            rows,
        } => {
            if let Err(err) = send_ssh_forward_resize(cid, sid, cols, rows, &sessions).await {
                warn!("failed to forward resize: {err}");
            }
        }
        NodeFrameData::TunnelData { cid, sid, data } => {
            if let Err(err) = send_ssh_tunnel_data(cid, sid, data, &sessions).await {
                warn!("failed to forward tunnel data: {err}");
            }
        }
        o => warn!("not implemented yet: {o:?}"),
    }
}

async fn send_ssh_tunnel_data(
    cid: String,
    _sid: u32,
    data: Vec<u8>,
    sessions: &Arc<Mutex<HashMap<String, SessionHandle>>>,
) -> anyhow::Result<(), String> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&cid).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        return Err(format!("no ssh tunnel found for connection {cid}"));
    };

    stdin
        .send(SSHCommand::Data(data))
        .await
        .map_err(|err| format!("failed to queue data to ssh tunnel for {cid}: {err}"))
}

async fn send_ssh_forward_resize(
    cid: String,
    _sid: u32,
    cols: u32,
    rows: u32,
    sessions: &Arc<Mutex<HashMap<String, SessionHandle>>>,
) -> anyhow::Result<(), String> {
    let stdin = {
        let sessions = sessions.lock().await;
        sessions.get(&cid).map(|s| s.get_stdin())
    };

    let Some(stdin) = stdin else {
        return Err(format!("no ssh tunnel found for connection {cid}"));
    };

    stdin
        .send(SSHCommand::Resize { cols, rows })
        .await
        .map_err(|err| format!("failed to queue resize to ssh tunnel for {cid}: {err}"))
}

async fn close_ssh_tunnel(cid: String, sessions: Arc<Mutex<HashMap<String, SessionHandle>>>) {
    let handle = {
        let mut sessions = sessions.lock().await;
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

async fn send_frame_data(sender: &Sender<Frame>, data: NodeFrameData) {
    if let Err(err) = sender.send(data.into()).await {
        warn!("failed to send frame: {err}");
    } else {
        debug!("frame response sent");
    }
}

async fn start_ssh_tunnel(
    tx: &Sender<Frame>,
    node_id: &String,
    cid: &String,
    config: &Arc<Env>,
    credentials: SSHConfigAuth,
    sessions: &Arc<Mutex<HashMap<String, SessionHandle>>>,
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

    let ssh_task = tokio::spawn(async move {
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

    let sessions_for_cleanup = sessions.clone();
    let cid_for_cleanup = cid_for_connection.clone();
    let cleanup_task = tokio::spawn(async move {
        if let Err(err) = ssh_task.await {
            warn!("ssh session join error for {cid_for_cleanup}: {err}");
        }

        let mut sessions = sessions_for_cleanup.lock().await;
        let should_remove = sessions
            .get(&cid_for_cleanup)
            .map(|handle| handle.get_id() == session_id)
            .unwrap_or(false);

        if should_remove {
            sessions.remove(&cid_for_cleanup);
        }
    });

    let handle = SessionHandle::SSH(SSHSessionHandle {
        id: session_id.clone(),
        stop: Some(stop_tx),
        join: cleanup_task,
        stdin: stdin_tx,
    });

    info!("handle {session_id} created");

    let previous = {
        let mut sessions = sessions.lock().await;
        sessions.insert(cid_for_connection, handle)
    };

    if let Some(prev) = previous {
        prev.shutdown().await;
    }
}
