use std::path::Path;
use log::{debug, warn};
use russh_sftp::client::SftpSession;
use tokio::sync::mpsc::Sender;
use phirepass_common::protocol::common::{Frame, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::sftp::{SFTPListItem, SFTPListItemAttributes, SFTPListItemKind};
use phirepass_common::protocol::web::WebFrameData;

pub async fn send_directory_listing(
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
            // Send error to web client
            if let Err(send_err) = tx
                .send(
                    NodeFrameData::WebFrame {
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: format!("Failed to list directory: {}", err),
                            msg_id,
                        },
                        sid,
                    }
                        .into(),
                )
                .await
            {
                warn!("failed to send error frame for directory listing: {send_err}");
            }
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
            size: attributes.size.map(|x| x).unwrap_or(0),
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
                size: entry.metadata().size.map(|x| x).unwrap_or(0),
            },
        });
    }

    Ok(root)
}