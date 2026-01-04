use crate::ssh::client::SSHClient;
use crate::ssh::session::SSHCommand;
use log::{debug, info, warn};
use phirepass_common::protocol::Protocol;
use phirepass_common::protocol::common::Frame;
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::web::WebFrameData;
use russh::client::Handle;
use russh::{ChannelMsg, Disconnect, Preferred, client, kex};
use std::borrow::Cow;
use std::io::Cursor;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use ulid::Ulid;

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
        node_id: Ulid,
        cid: Ulid,
        sid: u32,
        tx: &Sender<Frame>,
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

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for ssh tunnel {cid}");
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SSHCommand::Data(buf) => {
                            let bytes = Cursor::new(buf);
                            if let Err(err) = channel.data(bytes).await {
                                warn!("failed to send data to ssh channel {cid}: {err}");
                                break;
                            }
                        }
                        SSHCommand::Resize { cols, rows } => {
                            if let Err(err) = channel.window_change(cols, rows, 0, 0).await {
                                warn!("failed to resize ssh channel {cid}: {err}");
                            }
                        }
                    }
                }
                msg = channel.wait() => {
                    let Some(msg) = msg else {
                        info!("ssh channel closed for {cid}");
                        break;
                    };

                    match msg {
                        ChannelMsg::Data { ref data } => {
                            if let Err(err) = tx
                                .send(
                                    NodeFrameData::WebFrame {
                                        frame: WebFrameData::TunnelData {
                                            protocol: Protocol::SSH as u8,
                                            node_id: node_id.to_string(),
                                            sid,
                                            data: data.to_vec(),
                                        },
                                        sid,
                                    }
                                    .into(),
                                )
                                .await
                            {
                                warn!("failed to send frame from ssh to server to web: {err}");
                            } else {
                                debug!("frame response sent");
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
                        ChannelMsg::Close => {
                            debug!("ssh channel closed");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Err(err) = channel.close().await {
            warn!("failed to close ssh channel for {cid}: {err}");
        }

        client
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;

        Ok(())
    }
}
