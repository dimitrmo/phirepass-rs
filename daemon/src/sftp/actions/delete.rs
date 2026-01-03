use crate::sftp::SFTPActiveUploads;
use log::{info, warn};
use phirepass_common::protocol::common::{Frame, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::sftp::SFTPDelete;
use phirepass_common::protocol::web::WebFrameData;
use russh_sftp::client::SftpSession;
use tokio::sync::mpsc::Sender;

pub async fn delete_file(
    tx: &Sender<Frame>,
    sftp_session: &SftpSession,
    data: &SFTPDelete,
    cid: &String,
    sid: u32,
    msg_id: Option<u32>,
    uploads: &SFTPActiveUploads,
) {
    let file_path = format!(
        "{}{}",
        data.path,
        if data.path.ends_with('/') { "" } else { "/" }
    )
    .trim_end_matches('/')
    .to_string()
        + "/"
        + &data.filename;

    info!("starting file delete: {file_path}");

    // Cancel any active uploads for this file
    let temp_path = format!("{}.tmp", file_path);
    {
        let mut uploads = uploads.lock().await;
        // Remove all uploads for this cid that match the temp_path
        uploads.retain(|(upload_cid, _), file_upload| {
            !(upload_cid == cid && file_upload.temp_path == temp_path)
        });
    }

    // Attempt to delete the file
    match sftp_session.remove_file(&file_path).await {
        Ok(_) => {
            info!("file deleted successfully: {file_path}");
            // No need to send response, UI will refresh the directory listing
        }
        Err(err) => {
            warn!("failed to delete file {file_path}: {err}");
            // Send error response to web client
            let _ = tx
                .send(
                    NodeFrameData::WebFrame {
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: format!("Failed to delete file: {}", err),
                            msg_id,
                        },
                        sid,
                    }
                    .into(),
                )
                .await;
        }
    }
}
