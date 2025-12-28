use log::{debug, info, warn};
use phirepass_common::protocol::{Frame, NodeControlMessage, Protocol, encode_node_control};
use russh::client::{Handle, Msg};
use russh::keys::*;
use russh::*;
use std::borrow::Cow;
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub(crate) enum SSHCommand {
    Data(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

pub(crate) struct SSHSessionHandle {
    pub id: u64,
    pub stop: Option<oneshot::Sender<()>>,
    pub join: JoinHandle<()>,
    pub stdin: Sender<SSHCommand>,
}

impl SSHSessionHandle {
    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Err(err) = self.join.await {
            warn!("ssh session join error: {err}");
        }
    }
}

#[derive(Clone)]
pub(crate) enum SSHConfigAuth {
    UsernamePassword(String, String),
}

#[derive(Clone)]
pub(crate) struct SSHConfig {
    pub host: String,
    pub port: u16,
    pub credentials: SSHConfigAuth,
}

pub(crate) struct SSHConnection {
    cid: String,
    sender: Sender<Vec<u8>>,
    disconnect_notify: Option<oneshot::Sender<()>>,
}

impl client::Handler for SSHConnection {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
    }

    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        info!("ssh disconnected for {}: {:?}", self.cid, reason);
        if let Some(tx) = self.disconnect_notify.take() {
            let _ = tx.send(());
        }
        Ok(())
    }

    async fn channel_failure(
        &mut self,
        channel: ChannelId,
        session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        warn!("ssh channel failure for {}", self.cid);
        if let Some(tx) = self.disconnect_notify.take() {
            let _ = tx.send(());
        }
        session.close(channel)
    }

    async fn window_adjusted(
        &mut self,
        channel: ChannelId,
        new_size: u32,
        session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        info!(
            "ssh window adjusted for {}: new size {}",
            self.cid, new_size
        );
        Ok(())
    }

    async fn exit_signal(
        &mut self,
        channel: ChannelId,
        signal_name: Sig,
        core_dumped: bool,
        error_message: &str,
        lang_tag: &str,
        session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        warn!(
            "ssh exit signal for {}: signal_name={:?}, core_dumped={}, error_message={}, lang_tag={}",
            self.cid, signal_name, core_dumped, error_message, lang_tag
        );
        if let Some(tx) = self.disconnect_notify.take() {
            let _ = tx.send(());
        }
        session.close(channel)
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        info!("ssh channel closed for {}", self.cid);
        if let Some(tx) = self.disconnect_notify.take() {
            let _ = tx.send(());
        }
        session.close(channel)
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        debug!(
            "data received from ssh for connection {} ({} bytes)",
            self.cid,
            data.len()
        );

        let message = NodeControlMessage::Frame {
            frame: Frame::new(Protocol::SSH, data.to_vec()),
            cid: self.cid.clone(),
        };

        match encode_node_control(&message) {
            Ok(result) => {
                // CRITICAL: Use ONLY try_send to avoid blocking the russh event loop.
                // If the channel is full, we're under backpressure and must disconnect immediately.
                // Any .await here will freeze the entire SSH connection.
                match self.sender.try_send(result) {
                    Ok(_) => {
                        debug!("ssh data forwarded to ws writer (non-blocking)");
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        warn!(
                            "ws writer channel FULL for {}; disconnecting immediately to prevent freeze",
                            self.cid
                        );
                        if let Some(tx) = self.disconnect_notify.take() {
                            let _ = tx.send(());
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        warn!(
                            "ws writer channel closed for {}; signaling disconnect",
                            self.cid
                        );
                        if let Some(tx) = self.disconnect_notify.take() {
                            let _ = tx.send(());
                        }
                    }
                }
            }
            Err(err) => warn!("failed to encode node control: {}", err),
        }

        Ok(())
    }
}

impl SSHConnection {
    async fn create_client(
        cid: String,
        config: SSHConfig,
        sender: Sender<Vec<u8>>,
        disconnect_notify: oneshot::Sender<()>,
    ) -> anyhow::Result<Handle<Self>> {
        let ssh_config: SSHConfig = config.clone();

        let config = Arc::new(client::Config {
            // Detect long idle periods proactively to avoid silent hangs on NAT/TCP drops.
            inactivity_timeout: Some(std::time::Duration::from_secs(300)),
            preferred: Preferred {
                kex: Cow::Owned(vec![
                    kex::CURVE25519_PRE_RFC_8731,
                    kex::EXTENSION_SUPPORT_AS_CLIENT,
                ]),
                ..Default::default()
            },
            ..<_>::default()
        });

        let sh = Self {
            cid,
            // config: ssh_config.clone(),
            sender,
            disconnect_notify: Some(disconnect_notify),
        };

        let mut client_handler =
            client::connect(config, (ssh_config.host, ssh_config.port), sh).await?;

        let auth_res = match ssh_config.credentials {
            SSHConfigAuth::UsernamePassword(username, password) => {
                client_handler.authenticate_password(username, password)
            }
        }
        .await?;

        if !auth_res.success() {
            anyhow::bail!("SSH authentication failed. Please check your password.");
        }

        Ok(client_handler)
    }

    async fn listen(
        cid: String,
        channel: &Channel<Msg>,
        mut cmd_rx: Receiver<SSHCommand>,
        mut shutdown_rx: oneshot::Receiver<()>,
        mut disconnect_rx: oneshot::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                biased;
                // listen for shutdown signal
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for ssh tunnel {cid}");
                    break;
                }
                // listen for disconnect notification from SSH handler
                _ = &mut disconnect_rx => {
                    info!("remote ssh disconnect detected for tunnel {cid}");
                    break;
                }
                // listen for user issued commands
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SSHCommand::Data(buf) => {
                            // any SSHCommand::Data received from the web user is forwared to the SSH channel
                            let bytes = Cursor::new(buf);
                            if let Err(err) = channel.data(bytes).await {
                                warn!("failed to send data to ssh channel {cid}: {err}");
                                break;
                            }
                        }
                        SSHCommand::Resize { cols, rows } => {
                            // web user sends a resize request
                            if let Err(err) = channel.window_change(cols, rows, 0, 0).await {
                                warn!("failed to resize ssh channel {cid}: {err}");
                            }
                        }
                    }
                }
            }
        }
    }

    pub async fn connect(
        cid: String,
        config: SSHConfig,
        tx: &Sender<Vec<u8>>,
        cmd_rx: Receiver<SSHCommand>,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        debug!("connecting ssh...");

        // create client and provide a notify channel to surface remote disconnects
        let (handler_disconnect_tx, handler_disconnect_rx) = oneshot::channel();
        let session =
            Self::create_client(cid.clone(), config, tx.clone(), handler_disconnect_tx).await?;

        debug!("ssh connected");

        let channel = session.channel_open_session().await?;

        // Allocate a PTY so bash runs in interactive mode and emits a prompt.
        channel
            .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;

        let connection_id = cid.clone();
        debug!("ssh ready");

        Self::listen(cid, &channel, cmd_rx, shutdown_rx, handler_disconnect_rx).await;

        if let Err(err) = channel.close().await {
            warn!("failed to close ssh channel for {connection_id}: {err}");
        }

        session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;

        Ok(())
    }
}
