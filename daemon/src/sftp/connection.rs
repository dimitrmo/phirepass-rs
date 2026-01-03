use crate::sftp::SFTPActiveUploads;
use crate::sftp::actions::delete::delete_file;
use crate::sftp::actions::download::send_file_chunks;
use crate::sftp::actions::list_dir::send_directory_listing;
use crate::sftp::actions::upload::{start_upload, upload_file_chunk};
use crate::sftp::client::SFTPClient;
use crate::sftp::session::SFTPCommand;
use log::{debug, info};
use phirepass_common::protocol::common::Frame;
use russh::client::Handle;
use russh::{Preferred, client, kex};
use russh_sftp::client::SftpSession;
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::oneshot;

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
        uploads: &SFTPActiveUploads,
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

        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => {
                    info!("shutdown signal received for sftp tunnel {cid}");
                    break;
                }
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SFTPCommand::List(folder, msg_id) => {
                            debug!("sftp list command received for folder {folder}: {msg_id:?}");
                            send_directory_listing(&tx, &sftp, &folder, sid, msg_id).await;
                        }
                        SFTPCommand::Download { path, filename, msg_id } => {
                            debug!("sftp download command received for {path}/{filename}: {msg_id:?}");
                            send_file_chunks(&tx, &sftp, &path, &filename, sid, msg_id).await;
                        }
                        SFTPCommand::UploadStart { upload, msg_id } => {
                            debug!("sftp upload start command received for {}/{}: {msg_id:?}", upload.remote_path, upload.filename);
                            start_upload(&tx, &sftp, &upload, &cid, sid, msg_id, uploads).await;
                        }
                        SFTPCommand::Upload { chunk, msg_id } => {
                            debug!("sftp upload chunk command received for upload_id {}: {msg_id:?}", chunk.upload_id);
                            upload_file_chunk(&tx, &sftp, &chunk, &cid, sid, msg_id, uploads).await;
                        }
                        SFTPCommand::Delete { data, msg_id } => {
                            debug!("sftp delete command received for {}/{}: {msg_id:?}", data.path, data.filename);
                            delete_file(&tx, &sftp, &data, &cid, sid, msg_id, uploads).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
