use dashmap::DashMap;
use log::info;
use russh_sftp::client::fs::File;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};
use ulid::Ulid;

pub const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks

pub struct FileUpload {
    pub filename: String,
    pub remote_path: String,
    pub total_chunks: u32,
    #[allow(dead_code)]
    pub total_size: u64,
    pub sftp_file: File,
    pub temp_path: String,
    #[allow(dead_code)]
    pub started_at: SystemTime,
    pub last_updated: SystemTime,
}

pub struct FileDownload {
    pub filename: String,
    #[allow(dead_code)]
    pub total_size: u64,
    pub total_chunks: u32,
    pub sftp_file: File,
    #[allow(dead_code)]
    pub started_at: SystemTime,
    pub last_updated: SystemTime,
}

pub type SFTPActiveUploads = Arc<DashMap<(Ulid, u32), FileUpload>>;
pub type SFTPActiveDownloads = Arc<DashMap<(Ulid, u32), FileDownload>>;

static ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub fn generate_id() -> u32 {
    ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

pub async fn cleanup_abandoned_uploads(uploads: &SFTPActiveUploads) {
    info!("cleaning up abandoned uploads");

    const TIMEOUT: Duration = Duration::from_secs(15 * 60); // 15 minutes

    let now = SystemTime::now();
    let keys_to_remove: Vec<(Ulid, u32)> = uploads
        .iter()
        .filter_map(|entry| {
            let upload = entry.value();
            if let Ok(elapsed) = now.duration_since(upload.last_updated)
                && elapsed > TIMEOUT
            {
                return Some(*entry.key());
            }
            None
        })
        .collect();

    if !keys_to_remove.is_empty() {
        for key in keys_to_remove {
            info!("cleaning up abandoned upload: {:?}", key);
            if let Some((_, file_upload)) = uploads.remove(&key) {
                let _ = file_upload.sftp_file.sync_all().await;
            }
        }
    }
}

pub async fn cleanup_abandoned_downloads(downloads: &SFTPActiveDownloads) {
    info!("cleaning up abandoned downloads");

    const TIMEOUT: Duration = Duration::from_secs(15 * 60); // 15 minutes

    let now = SystemTime::now();
    let keys_to_remove: Vec<(Ulid, u32)> = downloads
        .iter()
        .filter_map(|entry| {
            let download = entry.value();
            if let Ok(elapsed) = now.duration_since(download.last_updated)
                && elapsed > TIMEOUT
            {
                return Some(*entry.key());
            }
            None
        })
        .collect();

    if !keys_to_remove.is_empty() {
        for key in keys_to_remove {
            info!("cleaning up abandoned download: {:?}", key);
            if let Some((_, file_download)) = downloads.remove(&key) {
                let _ = file_download.sftp_file.sync_all().await;
            }
        }
    }
}

pub mod actions;
pub mod client;
pub mod connection;
pub mod session;
