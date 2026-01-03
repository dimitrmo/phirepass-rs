use log::{debug, info, warn};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender;
use phirepass_common::protocol::common::{Frame, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::sftp::SFTPUploadChunk;
use phirepass_common::protocol::web::WebFrameData;
use crate::sftp::SFTPActiveUploads;

pub async fn upload_file_chunk(
    tx: &Sender<Frame>,
    sftp_session: &SftpSession,
    chunk: &SFTPUploadChunk,
    sid: u32,
    msg_id: Option<u32>,
    uploads: &SFTPActiveUploads,
) {
    // Build the full file path
    let file_path = if chunk.remote_path.ends_with('/') {
        format!("{}{}", chunk.remote_path, chunk.filename)
    } else {
        format!("{}/{}", chunk.remote_path, chunk.filename)
    };

    debug!(
        "uploading chunk {}/{} for file {file_path} ({} bytes)",
        chunk.chunk_index + 1,
        chunk.total_chunks,
        chunk.data.len()
    );

    // Use a temporary path for the upload in progress
    let temp_path = format!("{}.tmp", file_path);

    // For the first chunk, open the file on SFTP with WRITE | CREATE | APPEND
    if chunk.chunk_index == 0 {
        match sftp_session
            .open_with_flags(
                &temp_path,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::APPEND,
            )
            .await
        {
            Ok(mut file) => {
                // Write the first chunk
                if let Err(err) = file.write_all(&chunk.data).await {
                    warn!("failed to write chunk 0 to SFTP file: {err}");
                    let _ = tx
                        .send(
                            NodeFrameData::WebFrame {
                                frame: WebFrameData::Error {
                                    kind: FrameError::Generic,
                                    message: format!("Failed to write chunk: {}", err),
                                    msg_id,
                                },
                                sid,
                            }
                                .into(),
                        )
                        .await;
                    return;
                }

                {
                    // Store the file handle for subsequent chunks
                    let mut uploads = uploads.lock().await;
                    uploads.insert((temp_path.clone(), sid), file);
                    debug!("opened file on SFTP for appending: {}", temp_path);
                }
            }
            Err(err) => {
                warn!("failed to open file on SFTP: {err}");
                let _ = tx
                    .send(
                        NodeFrameData::WebFrame {
                            frame: WebFrameData::Error {
                                kind: FrameError::Generic,
                                message: format!("Failed to open file: {}", err),
                                msg_id,
                            },
                            sid,
                        }
                            .into(),
                    )
                    .await;
                return;
            }
        }
    } else if chunk.chunk_index + 1 < chunk.total_chunks {
        let mut uploads = uploads.lock().await;
        let key = (temp_path.clone(), sid);
        if let Some(file) = uploads.get_mut(&key) {
            if let Err(err) = file.write_all(&chunk.data).await {
                warn!(
                    "failed to write chunk {}/{} to SFTP file: {err}",
                    chunk.chunk_index + 1,
                    chunk.total_chunks
                );
                uploads.remove(&key);
                let _ = tx
                    .send(
                        NodeFrameData::WebFrame {
                            frame: WebFrameData::Error {
                                kind: FrameError::Generic,
                                message: format!("Failed to write chunk: {}", err),
                                msg_id,
                            },
                            sid,
                        }
                            .into(),
                    )
                    .await;
                return;
            }
            debug!(
                "appended chunk {}/{} to SFTP file",
                chunk.chunk_index + 1,
                chunk.total_chunks
            );
        } else {
            warn!("file handle not found for {}", temp_path);
            let _ = tx
                .send(
                    NodeFrameData::WebFrame {
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: "File handle not found".to_string(),
                            msg_id,
                        },
                        sid,
                    }
                        .into(),
                )
                .await;
            return;
        }
    } else {
        // Last chunk: write final chunk and close
        let file = {
            let mut uploads = uploads.lock().await;
            uploads.remove(&(temp_path.clone(), sid))
        };

        if let Some(mut file) = file {
            if let Err(err) = file.write_all(&chunk.data).await {
                warn!("failed to write final chunk to SFTP file: {err}");
                let _ = tx
                    .send(
                        NodeFrameData::WebFrame {
                            frame: WebFrameData::Error {
                                kind: FrameError::Generic,
                                message: format!("Failed to write final chunk: {}", err),
                                msg_id,
                            },
                            sid,
                        }
                            .into(),
                    )
                    .await;
                return;
            }

            // Close the file (file goes out of scope)
            drop(file);
            debug!("closed file on SFTP after final chunk");

            // Rename from temp to final path
            match sftp_session.rename(&temp_path, &file_path).await {
                Ok(_) => {
                    info!("file upload complete: {}", file_path);
                }
                Err(err) => {
                    warn!("failed to rename file on SFTP: {}", err);
                    let _ = tx
                        .send(
                            NodeFrameData::WebFrame {
                                frame: WebFrameData::Error {
                                    kind: FrameError::Generic,
                                    message: format!("Failed to rename file: {}", err),
                                    msg_id,
                                },
                                sid,
                            }
                                .into(),
                        )
                        .await;
                }
            }
        } else {
            warn!("file handle not found for final chunk of {temp_path}");

            let _ = tx
                .send(
                    NodeFrameData::WebFrame {
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: "File handle not found for final chunk".to_string(),
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