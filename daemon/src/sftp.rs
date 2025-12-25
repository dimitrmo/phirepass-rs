use futures_util::FutureExt;
use log::{debug, info, warn};
use russh::client::Session;
use russh::keys::PublicKey;
use russh::{ChannelId, Disconnect, Preferred, client, kex};
use russh_sftp::client::SftpSession;
use std::borrow::Cow;
use std::sync::Arc;
use envconfig::Envconfig;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub(crate) enum SFTPCommand {
    Data(Vec<u8>),
}

pub(crate) struct SFTPSessionHandle {
    pub(crate) id: u64,
    pub(crate) join: JoinHandle<()>,
    pub(crate) stdin: Sender<SFTPCommand>,
    pub(crate) stop: Option<oneshot::Sender<()>>,
}

impl SFTPSessionHandle {
    pub(crate) async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Err(err) = self.join.await {
            warn!("sftp session join error: {err}");
        }
    }
}

// -------------------------------------------------------------------------------------------------

struct SFTPClient {
    // send_to_client_channel: Sender
    // connection: Connection
    // connection.id: String
    cid: String,                // connection id
    sender: Sender<Vec<u8>>,    // sender to client
    config: SFTPConfig,         // config
}

impl SFTPClient {
    fn new() -> Self {
        Self {}
    }
}

impl client::Handler for SFTPClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
    }

    fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        // send_user_raw_data(&sender, connection_id.clone(), data.to_vec()).await;
        async { Ok(()) }
    }

    fn channel_close(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        // connection.close
        async { Ok(()) }
    }
}

struct SFTPHandler {
    handle: client::Handle<SFTPClient>,
}

impl SFTPHandler {
    async fn close(&mut self) -> anyhow::Result<()> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;
        Ok(())
    }

    async fn connect(sftp_config: SFTPConfig) -> anyhow::Result<Self> {
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

        let config = Arc::new(config);
        let sh = SFTPClient {};
        let mut session = client::connect(config, (sftp_config.host, sftp_config.port), sh).await?;

        let auth_res = match sftp_config.credentials {
            SFTPConfigAuth::UsernamePassword(username, password) => {
                session.authenticate_password(username, password)
            }
        }
        .await?;

        if !auth_res.success() {
            anyhow::bail!("SFTP authentication failed. Please check your password.");
        }

        Ok(Self { handle: session })
    }
}

pub(crate) enum SFTPConfigAuth {
    UsernamePassword(String, String),
}

pub(crate) struct SFTPConfig {
    pub host: String,
    pub port: u16,
    pub credentials: SFTPConfigAuth,
}

pub(crate) struct SFTPConnection {
    cid: String,
    sender: Sender<Vec<u8>>,
    config: SFTPConfig,
}

impl SFTPConnection {
    pub fn new(cid: String, sender: Sender<Vec<u8>>, config: SFTPConfig) -> Self {
        Self {
            cid,
            sender,
            config,
        }
    }

    pub async fn connect(
        self,
        mut cmd_rx: Receiver<SFTPCommand>, // received from user
        mut shutdown_rx: oneshot::Receiver<()>, // channel from app
    ) -> anyhow::Result<()> {
        debug!("connecting sftp...");

        let mut connection = SFTPHandler::connect(self.config).await?;

        debug!("sftp connected");

        let mut channel = connection.handle.channel_open_session().await?;

        channel.request_subsystem(true, "sftp").await?;
        let stream = channel.into_stream();
        let sftp = SftpSession::new(stream).await?;
        let connection_id = self.cid.clone();
        let sender = self.sender.clone();
        debug!("sftp ready");

        // info!("current path: {:?}", sftp.canonicalize(".").await?);
        // info!("dir info: {:?}", sftp.metadata(".").await?);
        // info!("symlink info: {:?}", sftp.symlink_metadata(".").await?);

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for sftp tunnel {connection_id}");
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    // commands coming from user
                    match cmd {
                        SFTPCommand::Data(_) => {
                            // sftp commands
                        },
                        // SFTPCommand::Rename file / folder
                    }
                },
            }
        }

        Ok(())
    }
}
