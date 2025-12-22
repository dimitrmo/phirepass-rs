use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use log::{debug, info, warn};
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
    Hello,
}

// ======= SessionHandle =======

pub(crate) struct SessionHandle {
    pub(crate) id: u64,
    // pub(crate) join: JoinHandle<()>,
    pub(crate) stdin: Sender<SFTPCommand>,
    stop: Option<oneshot::Sender<()>>,
    session: SftpSession,
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
                warn!("sftp session for session {} closed with error: {err}", self.id);
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
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
) {
    let mut sessions = sftp_sessions.lock().await;
    if let Some(session_handle) = sessions.get_mut(&cid) {
        session_handle.shutdown().await;
    }
}

pub(crate) async fn open_sftp_tunnel(
    cid: String,
    tx: Sender<Vec<u8>>,
    env_config: Arc<Env>,
    auth_config: Arc<AuthConfig>,
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
) {
    info!("A");
    tokio::spawn(async move {
        info!("B");
        let _ = start_sftp_tunnel(cid, tx, env_config, auth_config, sftp_sessions).await;
        info!("C");
    });
    info!("D");
}

pub(crate) async fn start_sftp_tunnel(
    cid: String,
    tx: Sender<Vec<u8>>,
    env_config: Arc<Env>,
    auth_config: Arc<AuthConfig>,
    sftp_sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
) -> anyhow::Result<()> {
    info!("D1");

    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let mut stop_rx = stop_rx;
    // tokio::pin!(stop_rx);

    let connection_id = cid.clone();

    let addrs = (env_config.ssh_host.clone(), env_config.ssh_port);
    info!("D1.1");
    let sftp = connect(cid.clone(), tx, addrs, auth_config.as_ref()).await?;
    info!("D1.2");
    let (stdin_tx, mut stdin_rx) = channel::<SFTPCommand>(512);

    let session_id = SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let handle = SessionHandle { id: session_id, stdin: stdin_tx, session: sftp, stop: Some(stop_tx) };

    info!("D2");

    {
        let mut sessions = sftp_sessions.lock().await;
        sessions.insert(cid.clone(), handle);
    }

    info!("E");

    loop {
        tokio::select! {
            biased;

            _ = &mut stop_rx => {
                debug!("sftp tunnel stop signal received for connection {connection_id}");
                break;
            }

            Some(cmd) = stdin_rx.recv() => {
                match cmd {
                    SFTPCommand::Hello => {
                        info!("hello has arrived");
                    },
                }
            }
        }
    }

    info!("F");

    // cleanup

    {
        if let Some(mut session_handle) = sftp_sessions.lock().await.remove(&cid) {
            session_handle.shutdown().await;
        }
    }

    info!("G");

    Ok(())
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
    let client = Client {
        cid,
        sender,
    };
    let config = Arc::new(config);
    info!("D1.1.0.0 {addrs:?} - {config:?}");
    let mut session = match client::connect(config, addrs, client).await {
        Ok(s) => s,
        Err(e) => anyhow::bail!("failed to connect to SFTP server: {e}")
    };
    info!("D1.1.0.1");
    let auth_res = match auth_config {
        AuthConfig::UsernamePassword(username, password) => {
            session.authenticate_password(username, password)
        }
    }
    .await?;

    info!("D1.1.1");

    if !auth_res.success() {
        anyhow::bail!("SFTP authentication failed. Please check your password.");
    }

    info!("D1.1.2");

    let channel = session.channel_open_session().await?;
    debug!("channel opened");

    info!("D1.1.3");

    channel.request_subsystem(true, "sftp").await?;
    debug!("channel ftp subsystem acquired");

    info!("D1.1.4");

    let stream = channel.into_stream();
    info!("D1.1.5");
    let sftp = SftpSession::new(stream).await?;
    debug!("sftp session established");

    info!("D1.1.6");

    Ok(sftp)
}
