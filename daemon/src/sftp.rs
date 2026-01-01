use log::{debug, info, warn};
use phirepass_common::protocol::common::Frame;
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::sftp::{SFTPListItem, SFTPListItemAttributes, SFTPListItemKind};
use phirepass_common::protocol::web::WebFrameData;
use russh::client::Handle;
use russh::keys::PublicKey;
use russh::{Preferred, client, kex};
use russh_sftp::client::SftpSession;
use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone, Debug)]
pub(crate) enum SFTPCommand {
    List(String, Option<u32>),
}

pub(crate) struct SFTPSessionHandle {
    pub id: u32,
    pub join: JoinHandle<()>,
    pub stdin: Sender<SFTPCommand>,
    pub stop: Option<oneshot::Sender<()>>,
}

impl SFTPSessionHandle {
    pub async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Err(err) = self.join.await {
            warn!("ssh session join error: {err}");
        }
    }
}

struct SFTPClient {}

impl client::Handler for SFTPClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> anyhow::Result<bool, Self::Error> {
        Ok(true)
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
    config: SFTPConfig,
}

impl SFTPConnection {
    pub fn new(config: SFTPConfig) -> Self {
        Self { config }
    }

    async fn create_client(&self) -> anyhow::Result<Handle<SFTPClient>> {
        let sftp_config: SFTPConfig = self.config.clone();

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

        let sh = SFTPClient {};

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

    pub async fn connect(
        &self,
        _node_id: String,
        cid: String,
        sid: u32,
        tx: &Sender<Frame>,
        mut cmd_rx: Receiver<SFTPCommand>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        debug!("connecting sftp...");

        let client = self.create_client().await?;

        debug!("sftp connected");

        let channel = client.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let stream = channel.into_stream();
        let sftp = SftpSession::new(stream).await?;

        send_directory_listing(&tx, &sftp, ".", sid, None).await;

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for ssh tunnel {cid}");
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SFTPCommand::List(folder, msg_id) => {
                            info!("sftp list command received for folder {folder}: {msg_id:?}");
                            send_directory_listing(&tx, &sftp, &folder, sid, msg_id).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

async fn send_directory_listing(
    tx: &Sender<Frame>,
    sftp_session: &SftpSession,
    path: &str,
    sid: u32,
    msg_id: Option<u32>,
) {
    let dir = match list_dir(sftp_session, path).await {
        Ok(dir) => dir,
        Err(err) => {
            warn!("failed to list directory {path}: {err}");
            return;
        }
    };

    match tx
        .send(
            NodeFrameData::WebFrame {
                frame: WebFrameData::SFTPListItems {
                    path: path.to_string(),
                    sid,
                    msg_id,
                    dir,
                },
                sid,
            }
            .into(),
        )
        .await
    {
        Ok(_) => {
            debug!("sftp sent directory listing for {path}");
        }
        Err(err) => {
            warn!("sftp failed to send directory listing for {path}: {err}");
        }
    }
}

async fn list_dir(sftp_session: &SftpSession, path: &str) -> anyhow::Result<SFTPListItem> {
    let abs_path = sftp_session.canonicalize(path).await?;
    let attributes = sftp_session.metadata(path).await?;
    let name = Path::new(&abs_path)
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .last();

    let mut root = SFTPListItem {
        name: name.unwrap_or(path).to_string(),
        path: abs_path.clone(),
        kind: SFTPListItemKind::Folder,
        items: vec![],
        attributes: SFTPListItemAttributes {
            size: attributes.size.map(|x| x as u32).unwrap_or(0),
        },
    };

    for entry in sftp_session.read_dir(path).await? {
        let kind = {
            if entry.file_type().is_dir() {
                SFTPListItemKind::Folder
            } else {
                SFTPListItemKind::File
            }
        };

        root.items.push(SFTPListItem {
            name: entry.file_name(),
            path: abs_path.clone(),
            kind,
            items: vec![],
            attributes: SFTPListItemAttributes {
                size: entry.metadata().size.map(|x| x as u32).unwrap_or(0),
            },
        });
    }

    Ok(root)
}
