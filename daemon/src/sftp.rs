use log::{debug, info, warn};
use phirepass_common::protocol::{Frame, NodeControlMessage, Protocol, encode_node_control};
use russh::client::{Handle, Msg};
use russh::keys::*;
use russh::*;
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub(crate) enum SFTPCommand {
    Data(Vec<u8>),
}

pub(crate) struct SFTPSessionHandle {
    pub id: u64,
    pub stop: Option<oneshot::Sender<()>>,
    pub join: JoinHandle<()>,
    pub stdin: Sender<SFTPCommand>,
}

impl SFTPSessionHandle {
    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Err(err) = self.join.await {
            warn!("sftp session join error: {err}");
        }
    }
}

#[derive(Clone)]
pub(crate) enum SFTPConfigAuth {
    UsernamePassword(String, String),
}

#[derive(Clone)]
pub(crate) struct SFTPConfig {
    pub host: String,
    pub port: u16,
    pub credentials: SFTPConfigAuth,
}

pub(crate) struct SFTPConnection {
    cid: String,
    sender: Sender<Vec<u8>>,
}

impl client::Handler for SFTPConnection {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let message = NodeControlMessage::Frame {
            frame: Frame::new(Protocol::SFTP, data.to_vec()),
            cid: self.cid.clone(),
        };

        match encode_node_control(&message) {
            Ok(result) => match self.sender.send(result).await {
                Ok(_) => debug!("sftp response sent back to {}", self.cid),
                Err(err) => {
                    warn!("failed to send: {err}; closing sftp channel");
                }
            },
            Err(err) => warn!("failed to encode node control: {}", err),
        }

        Ok(())
    }
}

impl SFTPConnection {
    async fn create_client(
        cid: String,
        config: SFTPConfig,
        sender: Sender<Vec<u8>>,
    ) -> anyhow::Result<Handle<Self>> {
        let sftp_config: SFTPConfig = config.clone();

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

        let sh = Self {
            cid,
            sender,
        };

        let mut client_handler =
            client::connect(config, (sftp_config.host, sftp_config.port), sh).await?;

        let auth_res = match sftp_config.credentials {
            SFTPConfigAuth::UsernamePassword(username, password) => {
                client_handler.authenticate_password(username, password)
            }
        }
        .await?;

        if !auth_res.success() {
            anyhow::bail!("SFTP authentication failed. Please check your password.");
        }

        Ok(client_handler)
    }

    async fn listen(
        cid: String,
        channel: &Channel<Msg>,
        mut cmd_rx: Receiver<SFTPCommand>,
        mut shutdown_rx: oneshot::Receiver<()>,
        current_dir: Arc<tokio::sync::Mutex<String>>,
    ) {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for sftp tunnel {cid}");
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SFTPCommand::Data(buf) => {
                            // Parse custom commands
                            if let Ok(command_str) = String::from_utf8(buf.clone()) {
                                let command_str = command_str.trim();
                                debug!("sftp command received: {}", command_str);
                                
                                if let Some(response) = Self::handle_custom_command(
                                    command_str,
                                    channel,
                                    current_dir.clone()
                                ).await {
                                    if let Err(err) = channel.data(response.as_bytes()).await {
                                        warn!("failed to send response to sftp channel {cid}: {err}");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    async fn handle_custom_command(
        command: &str,
        channel: &Channel<Msg>,
        current_dir: Arc<tokio::sync::Mutex<String>>,
    ) -> Option<String> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "ls" => {
                // Execute ls command
                let path = if parts.len() > 1 {
                    parts[1].to_string()
                } else {
                    let dir = current_dir.lock().await;
                    dir.clone()
                };
                
                let cmd = format!("ls -la {}\n", path);
                if let Err(err) = channel.data(cmd.as_bytes()).await {
                    warn!("failed to execute ls: {err}");
                    return Some(format!("Error executing ls: {}\n", err));
                }
                None
            }
            "cd" => {
                // Change directory command
                if parts.len() < 2 {
                    return Some("cd: missing directory argument\n".to_string());
                }
                
                let new_dir = parts[1];
                let cmd = format!("cd {} && pwd\n", new_dir);
                
                if let Err(err) = channel.data(cmd.as_bytes()).await {
                    warn!("failed to execute cd: {err}");
                    return Some(format!("Error executing cd: {}\n", err));
                }
                
                // Update current directory
                let mut dir = current_dir.lock().await;
                *dir = new_dir.to_string();
                
                Some(format!("Changed directory to {}\n", new_dir))
            }
            "pwd" => {
                let dir = current_dir.lock().await;
                Some(format!("{}\n", dir.as_str()))
            }
            _ => {
                // Unknown command, forward as-is
                if let Err(err) = channel.data(format!("{}\n", command).as_bytes()).await {
                    warn!("failed to send data to sftp channel: {err}");
                    return Some(format!("Error: {}\n", err));
                }
                None
            }
        }
    }

    pub async fn connect(
        cid: String,
        config: SFTPConfig,
        tx: &Sender<Vec<u8>>,
        cmd_rx: Receiver<SFTPCommand>,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        debug!("connecting sftp...");

        let session = Self::create_client(cid.clone(), config, tx.clone()).await?;

        debug!("sftp connected");

        let channel = session.channel_open_session().await?;

        // Request sftp subsystem
        if let Err(err) = channel.request_subsystem(true, "sftp").await {
            warn!("failed to request sftp subsystem: {err}, falling back to shell");
            // Fallback to shell mode for custom commands
            channel
                .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
                .await?;
            channel.request_shell(true).await?;
        }

        let connection_id = cid.clone();
        let current_dir = Arc::new(tokio::sync::Mutex::new("/".to_string()));
        debug!("sftp ready");

        Self::listen(cid, &channel, cmd_rx, shutdown_rx, current_dir).await;

        if let Err(err) = channel.close().await {
            warn!("failed to close sftp channel for {connection_id}: {err}");
        }

        session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;

        Ok(())
    }
}
