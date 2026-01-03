use crate::sftp::{
    CHUNK_SIZE, FileDownload, SFTPActiveDownloads, cleanup_abandoned_downloads,
    generate_download_id,
};
use log::{debug, info, warn};
use phirepass_common::protocol::common::{Frame, FrameError};
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::sftp::{
    SFTPDownloadChunk, SFTPDownloadStart, SFTPDownloadStartResponse,
};
use phirepass_common::protocol::web::WebFrameData;
use russh_sftp::client::SftpSession;
use std::time::SystemTime;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc::Sender;
use ulid::Ulid;

pub async fn start_download(
    tx: &Sender<Frame>,
    sftp_session: &SftpSession,
    download: &SFTPDownloadStart,
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    downloads: &SFTPActiveDownloads,
) {
    cleanup_abandoned_downloads(downloads).await;

    let file_path = if download.path.ends_with('/') {
        format!("{}{}", download.path, download.filename)
    } else {
        format!("{}/{}", download.path, download.filename)
    };

    info!("starting download: {file_path}");

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

    debug!("file size: {total_size} bytes, will send {total_chunks} chunks");

    // Open the file
    let file = match sftp_session.open(&file_path).await {
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

    // Generate unique download ID
    let download_id = generate_download_id();
    let now = SystemTime::now();

    {
        // Store the file handle and metadata for subsequent chunks
        let mut downloads = downloads.lock().await;
        downloads.insert(
            (cid, download_id),
            FileDownload {
                filename: download.filename.clone(),
                total_size,
                total_chunks,
                sftp_file: file,
                started_at: now,
                last_updated: now,
            },
        );
        info!(
            "opened file on SFTP for download: {} (download_id: {})",
            file_path, download_id
        );
    }

    // Send download start response with download_id
    let _ = tx
        .send(
            NodeFrameData::WebFrame {
                frame: WebFrameData::SFTPDownloadStartResponse {
                    sid,
                    msg_id,
                    response: SFTPDownloadStartResponse {
                        download_id,
                        total_size,
                        total_chunks,
                    },
                },
                sid,
            }
            .into(),
        )
        .await;
}

pub async fn download_file_chunk(
    tx: &Sender<Frame>,
    cid: Ulid,
    sid: u32,
    msg_id: Option<u32>,
    download_id: u32,
    chunk_index: u32,
    downloads: &SFTPActiveDownloads,
) {
    let mut downloads = downloads.lock().await;
    let key = (cid, download_id);

    match downloads.get_mut(&key) {
        Some(download) => {
            let mut buffer = vec![0u8; CHUNK_SIZE];

            match download.sftp_file.read(&mut buffer).await {
                Ok(0) => {
                    // EOF reached
                    info!(
                        "file download complete: {} (download_id: {}), sent {} chunks",
                        download.filename, download_id, chunk_index
                    );
                    // Remove the download entry
                    downloads.remove(&key);
                }
                Ok(bytes_read) => {
                    let chunk_data = buffer[..bytes_read].to_vec();
                    let chunk = SFTPDownloadChunk {
                        download_id,
                        chunk_index,
                        chunk_size: bytes_read as u32,
                        data: chunk_data,
                    };

                    // Update last_updated timestamp
                    download.last_updated = SystemTime::now();

                    debug!(
                        "sending chunk {}/{} ({} bytes) for download_id {}",
                        chunk_index + 1,
                        download.total_chunks,
                        bytes_read,
                        download_id
                    );

                    if let Err(err) = tx
                        .send(
                            NodeFrameData::WebFrame {
                                frame: WebFrameData::SFTPDownloadChunk { sid, msg_id, chunk },
                                sid,
                            }
                            .into(),
                        )
                        .await
                    {
                        warn!(
                            "failed to send chunk {chunk_index} for download_id {download_id}: {err}"
                        );
                    }
                }
                Err(err) => {
                    warn!(
                        "error reading file for download_id {download_id} at chunk {chunk_index}: {err}"
                    );
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
                    downloads.remove(&key);
                }
            }
        }
        None => {
            warn!("download not found: {:?}", key);
            let _ = tx
                .send(
                    NodeFrameData::WebFrame {
                        frame: WebFrameData::Error {
                            kind: FrameError::Generic,
                            message: "Download not found or expired".to_string(),
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
