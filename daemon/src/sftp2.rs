use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use log::{debug, info, warn};
use phirepass_common::protocol::{
    NodeControlMessage, Protocol, WebControlMessage, encode_node_control,
    encode_web_control_to_frame,
};
use anyhow::anyhow;
use russh::client::Session;
use russh::keys::PublicKey;
use russh::{ChannelId, Preferred, client, kex};
use russh_sftp::client::SftpSession;
use tokio::sync::mpsc::{Sender, channel};
use tokio::sync::{Mutex, oneshot};

use crate::env::Env;

// =============================

static SESSION_ID: AtomicU64 = AtomicU64::new(1);

// =============================

#[derive(Clone, Debug)]
pub enum SFTPCommand {
    Data(Vec<u8>),
}

// ======= SessionHandle =======

pub(crate) struct SessionHandle {
    pub(crate) id: u64,
    pub(crate) conn_id: String,
    // pub(crate) join: JoinHandle<()>,
    pub(crate) stdin: Sender<SFTPCommand>,
    stop: Option<oneshot::Sender<()>>,
    session: SftpSession,
}

impl std::fmt::Debug for SessionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionHandle")
            .field("session_id", &self.id)
            .field("connection_id", &self.conn_id)
            .finish()
    }
}

impl SessionHandle {
    pub(crate) async fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }

        match self.session.close().await {
            Ok(()) => {
                debug!("sftp session for session {} closed successfully", self.id);
            }
            Err(err) => {
                warn!(
                    "sftp session for session {} closed with error: {err}",
                    self.id
                );
            }
        }
    }
}

// ==========================

pub(crate) enum AuthConfig {
    UsernamePassword(String, String),
}

// ========= Client =========

struct Client {
    cid: String,
    sender: Sender<Vec<u8>>,
}

impl russh::client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
    }

    fn data(
        &mut self,
        _channel: ChannelId,
        _data: &[u8],
        _session: &mut Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        // must notify user here
        // create frame, use cid ke steilto sto kalo
        // probably via sender
        async { Ok(()) }
    }
}

pub(crate) async fn close_sftp_tunnel(
    cid: String,
    session_id: Option<u64>,
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
) {
    let mut sessions = sftp_sessions.lock().await;

    if let Some(session_id) = session_id {
        let key = format!("{}-{}", cid, session_id);
        if let Some(session_handle) = sessions.get_mut(&key) {
            session_handle.shutdown().await;
        }
    } else {
        let keys: Vec<String> = sessions
            .keys()
            .filter(|k| k.starts_with(&cid))
            .cloned()
            .collect();

        for key in keys {
            if let Some(mut handle) = sessions.remove(key.as_str()) {
                info!("closing ssh tunnel for connection {cid} (key: {key})");
                handle.shutdown().await;
            }
        }
    }
}

pub(crate) async fn open_sftp_tunnel(
    cid: String,
    tx: Sender<Vec<u8>>,
    env_config: Arc<Env>,
    auth_config: Arc<AuthConfig>,
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
) {
    tokio::spawn(async move {
        match start_sftp_tunnel(cid, tx, env_config, auth_config, sftp_sessions).await {
            Ok(session_id) => {
                info!("sftp tunnel terminated successfully for session_id={session_id}")
            }
            Err(err) => warn!("sftp tunnel crashed with error: {err}"),
        }
    });
}

pub(crate) async fn start_sftp_tunnel(
    cid: String,
    tx: Sender<Vec<u8>>,
    env_config: Arc<Env>,
    auth_config: Arc<AuthConfig>,
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
) -> anyhow::Result<u64> {
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let mut stop_rx = stop_rx;

    let connection_id = cid.clone();
    let tx_user = tx.clone();
    let addrs = (env_config.sftp_host.clone(), env_config.sftp_port);
    let sftp = connect(cid.clone(), tx, addrs, auth_config.as_ref()).await?;
    let (stdin_tx, mut stdin_rx) = channel::<SFTPCommand>(512);

    let session_id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let handle = SessionHandle {
        id: session_id,
        conn_id: connection_id.clone(),
        stdin: stdin_tx,
        session: sftp,
        stop: Some(stop_tx),
    };

    {
        let mut sessions = sftp_sessions.lock().await;
        let key = format!("{}-{}", cid, session_id);
        sessions.insert(key, handle);
    }

    let _ = send_sftp_data_to_connection(
        &tx_user,
        sftp_sessions.clone(),
        &cid,
        session_id,
        &WebControlMessage::TunnelOpened {
            protocol: Protocol::SFTP as u8,
            session_id: session_id,
        },
    )
    .await;

    loop {
        tokio::select! {
            biased;

            _ = &mut stop_rx => {
                warn!("sftp tunnel stop signal received for connection {connection_id}");
                break;
            }

            Some(cmd) = stdin_rx.recv() => {
                info!("message received ====== ");
                match cmd {
                    SFTPCommand::Data(data) => {
                        info!("hello has arrived {:?}", data);
                    },
                }
            }
        }
    }

    // cleanup

    {
        let key = format!("{}-{}", cid, session_id);
        if let Some(mut session_handle) = sftp_sessions.lock().await.remove(&key) {
            info!("sftp connection for {key} removed");
            session_handle.shutdown().await;
        }
    }

    Ok(session_id)
}

async fn send_sftp_data_to_connection(
    tx: &Sender<Vec<u8>>,
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
    cid: &str,
    session_id: u64,
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
        tokio::spawn(close_sftp_tunnel(
            cid.to_string(),
            Some(session_id),
            sftp_sessions,
        ));

        anyhow!("failed to send data to connection: {err}")
    })
}

pub(crate) async fn connect(
    cid: String,
    sender: Sender<Vec<u8>>,
    addrs: (String, u16),
    auth_config: &AuthConfig,
) -> anyhow::Result<SftpSession> {
    let config = client::Config {
        inactivity_timeout: None,
        preferred: Preferred {
            kex: Cow::Owned(vec![
                kex::CURVE25519_PRE_RFC_8731,
                kex::EXTENSION_SUPPORT_AS_CLIENT,
            ]),
            ..Default::default()
        },
        ..<_>::default()
    };
    let client = Client { cid, sender };
    let config = Arc::new(config);

    let mut session = match client::connect(config, addrs, client).await {
        Ok(client) => client,
        Err(err) => {
            anyhow::bail!("failed to connect to SFTP server: {err}")
        }
    };

    info!("XXXX");

    let auth_res = match auth_config {
        AuthConfig::UsernamePassword(username, password) => {
            session.authenticate_password(username, password)
        }
    }
    .await?;

    if !auth_res.success() {
        anyhow::bail!("SFTP authentication failed. Please check your password.");
    }

    info!("XYZ");

    let channel = session.channel_open_session().await?;
    debug!("channel opened");

    channel.request_subsystem(true, "sftp").await?;
    debug!("channel ftp subsystem acquired");

    let stream = channel.into_stream();
    let sftp = SftpSession::new(stream).await?;
    debug!("sftp session established");

    Ok(sftp)
}
