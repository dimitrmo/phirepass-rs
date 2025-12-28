use log::{debug, info, warn};
use phirepass_common::protocol::{Frame, NodeControlMessage, Protocol, encode_node_control};
use russh::client::Handle;
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

struct SSHClient {}

impl client::Handler for SSHClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
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
    config: SSHConfig,
}

impl SSHConnection {
    pub fn new(config: SSHConfig) -> Self {
        Self { config }
    }

    async fn create_client(&self) -> anyhow::Result<Handle<SSHClient>> {
        let ssh_config: SSHConfig = self.config.clone();

        let config = Arc::new(client::Config {
            inactivity_timeout: None,
            preferred: Preferred {
                kex: Cow::Owned(vec![
                    kex::CURVE25519_PRE_RFC_8731,
                    kex::EXTENSION_SUPPORT_AS_CLIENT,
                ]),
                ..Default::default()
            },
            ..<_>::default()
        });

        let sh = SSHClient {};

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

    pub async fn connect(
        &self,
        cid: String,
        tx: &Sender<Vec<u8>>,
        mut cmd_rx: Receiver<SSHCommand>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        debug!("connecting ssh...");

        let client = self.create_client().await?;

        debug!("ssh connected");

        let mut channel = client.channel_open_session().await?;

        // Allocate a PTY so bash runs in interactive mode and emits a prompt.
        channel
            .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;

        let connection_id = cid.clone();
        let sender = tx.clone();

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for ssh tunnel {connection_id}");
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SSHCommand::Data(buf) => {
                            let bytes = Cursor::new(buf);
                            if let Err(err) = channel.data(bytes).await {
                                warn!("failed to send data to ssh channel {connection_id}: {err}");
                                break;
                            }
                        }
                        SSHCommand::Resize { cols, rows } => {
                            if let Err(err) = channel.window_change(cols, rows, 0, 0).await {
                                warn!("failed to resize ssh channel {connection_id}: {err}");
                            }
                        }
                    }
                }
                msg = channel.wait() => {
                    let Some(msg) = msg else {
                        info!("ssh channel closed for {connection_id}");
                        break;
                    };

                    match msg {
                        ChannelMsg::Data { ref data } => {
                            let message = NodeControlMessage::Frame {
                                frame: Frame::new(Protocol::SSH, data.to_vec()),
                                cid: connection_id.clone(),
                            };

                            match encode_node_control(&message) {
                                Ok(result) => match sender.send(result).await {
                                    Ok(_) => debug!("ssh response sent back to {connection_id}"),
                                    Err(err) => {
                                        warn!("failed to send: {err}; closing ssh channel");
                                        break;
                                    }
                                },
                                Err(err) => warn!("failed to encode node control: {}", err),
                            }
                        }
                        ChannelMsg::Eof => {
                            debug!("ssh channel received EOF");
                            break;
                        }
                        ChannelMsg::ExitStatus { exit_status } => {
                            warn!("ssh channel exited with status {}", exit_status);
                            if let Err(err) = channel.eof().await {
                                warn!("failed to send EOF to ssh channel: {err}");
                            }

                            break;
                        }
                        ChannelMsg::Close { .. } => {
                            debug!("ssh channel closed");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Err(err) = channel.close().await {
            warn!("failed to close ssh channel for {connection_id}: {err}");
        }

        client
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;

        Ok(())
    }
}
