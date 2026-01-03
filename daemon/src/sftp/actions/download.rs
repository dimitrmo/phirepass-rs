use log::{debug, info, warn};
use russh_sftp::client::SftpSession;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::Sender;
use phirepass_common::protocol::common::{Frame, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::sftp::SFTPFileChunk;
use phirepass_common::protocol::web::WebFrameData;
use crate::sftp::CHUNK_SIZE;

pub async fn send_file_chunks(
    tx: &Sender<Frame>,
    sftp_session: &SftpSession,
    path: &str,
    filename: &str,
    sid: u32,
    msg_id: Option<u32>,
) {
    // Build the full file path
    let file_path = if path.ends_with('/') {
        format!("{}{}", path, filename)
    } else {
        format!("{}/{}", path, filename)
    };

    info!("starting file download: {file_path}");

    // Get file metadata to determine size
    let metadata = match sftp_session.metadata(&file_path).await {
        Ok(meta) => meta,
        Err(err) => {
            warn!("failed to get file metadata for {file_path}: {err}");
            let _ = tx
                .send(
                    NodeFrameData::WebFrame {
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: format!("Failed to get file metadata: {}", err),
                            msg_id,
                        },
                        sid,
                    }
                        .into(),
                )
                .await;
            return;
        }
    };

    let total_size = metadata.size.unwrap_or(0);
    let total_chunks = ((total_size as f64) / (CHUNK_SIZE as f64)).ceil() as u32;

    info!("file size: {total_size} bytes, will send {total_chunks} chunks");

    // Open the file
    let mut file = match sftp_session.open(&file_path).await {
        Ok(f) => f,
        Err(err) => {
            warn!("failed to open file {file_path}: {err}");
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
    };

    // Read and send file in chunks
    let mut chunk_index = 0;
    let mut buffer = vec![0u8; CHUNK_SIZE];

    loop {
        match file.read(&mut buffer).await {
            Ok(0) => {
                // EOF reached
                info!("file download complete: {file_path}, sent {chunk_index} chunks");
                break;
            }
            Ok(bytes_read) => {
                let chunk_data = buffer[..bytes_read].to_vec();
                let chunk = SFTPFileChunk {
                    filename: filename.to_string(),
                    chunk_index,
                    total_chunks,
                    total_size,
                    chunk_size: bytes_read as u32,
                    data: chunk_data,
                };

                debug!(
                    "sending chunk {}/{} ({} bytes) for {file_path}",
                    chunk_index + 1,
                    total_chunks,
                    bytes_read
                );

                if let Err(err) = tx
                    .send(
                        NodeFrameData::WebFrame {
                            frame: WebFrameData::SFTPFileChunk { sid, msg_id, chunk },
                            sid,
                        }
                            .into(),
                    )
                    .await
                {
                    warn!("failed to send chunk {chunk_index} for {file_path}: {err}");
                    break;
                }

                chunk_index += 1;
            }
            Err(err) => {
                warn!("error reading file {file_path} at chunk {chunk_index}: {err}");
                let _ = tx
                    .send(
                        NodeFrameData::WebFrame {
                            frame: WebFrameData::Error {
                                kind: FrameError::Generic,
                                message: format!("Error reading file: {}", err),
                                msg_id,
                            },
                            sid,
                        }
                            .into(),
                    )
                    .await;
                break;
            }
        }
    }
}