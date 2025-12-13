use crate::env::Env;
use crate::ssh::{SSHConfig, SSHConfigAuth, SSHConnection};
use anyhow::anyhow;
use futures_util::{SinkExt, StreamExt};
use log::{info, warn};
use phirepass_common::env::Mode;
use phirepass_common::protocol::{
    NodeControlMessage, Protocol, WebControlMessage, decode_node_control,
    encode_node_control, encode_web_control_to_frame, generic_web_error,
};
use phirepass_common::stats::Stats;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, channel};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[derive(Clone, Debug)]
pub(crate) enum SSHCommand {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

struct SSHSessionHandle {
    stop: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
    stdin: Sender<SSHCommand>,
}

impl SSHSessionHandle {
    async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Err(err) = self.join.await {
            warn!("ssh session join error: {err}");
        }
    }
}

pub(crate) struct WSConnection {
    writer: UnboundedSender<Vec<u8>>,
    reader: UnboundedReceiver<Vec<u8>>,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
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

impl WSConnection {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        Self {
            reader: rx,
            writer: tx,
            ssh_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn connect(self, config: Arc<Env>) -> anyhow::Result<()> {
        info!("connecting ws...");

        let ping_interval = config.ping_interval;
        let endpoint = format!(
            "{}/nodes/ws",
            generate_server_endpoint(
                config.mode.clone(),
                config.server_host.to_string(),
                config.server_port,
            )
        );

        info!("trying {endpoint}");

        let (stream, _) = connect_async(endpoint).await?;
        let (mut write, mut read) = stream.split();

        let auth_frame = encode_node_control(&NodeControlMessage::Auth {
            token: config.token.clone(),
        })?;

        write.send(Message::Binary(auth_frame.into())).await?;

        let tx_writer = self.writer.clone();
        let hb_tx = self.writer.clone();
        let mut rx = self.reader;
        let ssh_sessions = self.ssh_sessions.clone();

        let write_task = tokio::spawn(async move {
            while let Some(frame) = rx.recv().await {
                if let Err(err) = write.send(Message::Binary(frame.into())).await {
                    warn!("failed to send frame: {}", err);
                    break;
                }
            }
        });

        info!("writer setup");

        let reader_task = tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Binary(data)) => match decode_node_control(&data) {
                        Ok(msg) => {
                            handle_control_from_server(
                                msg,
                                &tx_writer,
                                ssh_sessions.clone(),
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

        info!("reader setup");

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
                        if hb_tx.send(raw).is_err() {
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
                        if ping_tx.send(raw).is_err() {
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

async fn handle_control_from_server(
    msg: NodeControlMessage,
    tx: &UnboundedSender<Vec<u8>>,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    config: Arc<Env>,
) {
    info!("received control message from server");

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
                Ok(protocol) => warn!("unsupported protocol for tunnel: {}", protocol),
                Err(err) => warn!("invalid protocol value {}: {:?}", protocol, err),
            }
        }
        NodeControlMessage::ConnectionDisconnect { cid } => {
            close_ssh_tunnel(ssh_sessions.clone(), cid).await;
        }
        NodeControlMessage::Resize { cid, cols, rows } => {
            if let Err(err) = forward_resize(ssh_sessions.clone(), cid, cols, rows).await {
                warn!("{err}");
            }
        }
        NodeControlMessage::TunnelData {
            protocol,
            cid,
            data,
        } => {
            if protocol == Protocol::SSH as u8 {
                if let Err(err) = tunnel_data(ssh_sessions.clone(), cid, data).await {
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
                    if tx.send(raw).is_err() {
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

async fn open_ssh_tunnel(
    tx: &UnboundedSender<Vec<u8>>,
    cid: String,
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    config: Arc<Env>,
    username: String,
    password: String,
) {
    info!("opening ssh tunnel for connection {cid}...");

    // Check if authentication mode requires password
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
    tx: &UnboundedSender<Vec<u8>>,
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
    let ssh_task = tokio::spawn(async move {
        info!("ssh task started for connection {cid_for_task}");

        let conn = SSHConnection::new(SSHConfig {
            host: config.ssh_host.clone(),
            port: config.ssh_port,
            credentials,
        });

        match conn
            .connect(&sender, cid_for_task.clone(), stdin_rx, stop_rx)
            .await
        {
            Ok(()) => {
                info!("ssh connection {cid_for_task} ended");

                if let Err(err) = send_data_to_connection(
                    &sender,
                    cid.as_str(),
                    &WebControlMessage::TunnelClosed {
                        protocol: Protocol::SSH as u8,
                    },
                ) {
                    warn!("failed to notify cid {cid_for_task} for ssh connection closure: {err}");
                }
            }
            Err(err) => {
                warn!("ssh connection error for {cid_for_task}: {err}");
                if let Err(err) = send_data_to_connection(
                    &sender,
                    cid.as_str(),
                    &generic_web_error("SSH authentication failed. Please check your password."),
                ) {
                    warn!("failed to notify connection {cid} about authentication failure: {err}");
                }
            }
        }
    });

    let handle = SSHSessionHandle {
        stop: Some(stop_tx),
        join: ssh_task,
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
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    cid: String,
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

async fn tunnel_data(
    ssh_sessions: Arc<Mutex<HashMap<String, SSHSessionHandle>>>,
    cid: String,
    data: Vec<u8>,
) -> anyhow::Result<(), String> {
    let stdin = {
        let sessions = ssh_sessions.lock().await;
        sessions.get(&cid).map(|s| s.stdin.clone())
    };

    let Some(stdin) = stdin else {
        return Err(format!("no ssh tunnel found for connection {cid}"));
    };

    stdin
        .try_send(SSHCommand::Data(data))
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
        .try_send(SSHCommand::Resize { cols, rows })
        .map_err(|err| format!("failed to queue resize to ssh tunnel for {cid}: {err}"))
}

fn send_data_to_connection(
    tx: &UnboundedSender<Vec<u8>>,
    cid: &str,
    data: &WebControlMessage,
) -> anyhow::Result<()> {
    let frame = encode_web_control_to_frame(data)?;

    let node_msg = NodeControlMessage::Frame {
        frame,
        cid: cid.to_string(),
    };

    let raw = encode_node_control(&node_msg)?;
    tx.send(raw)
        .map_err(|err| anyhow!("failed to send data to connection: {err}"))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
