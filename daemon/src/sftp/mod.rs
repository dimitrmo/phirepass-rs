use russh_sftp::client::fs::File;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;

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

pub type SFTPActiveUploads = Arc<Mutex<HashMap<(String, u32), FileUpload>>>;

static UPLOAD_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub fn generate_upload_id() -> u32 {
    UPLOAD_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

pub async fn cleanup_abandoned_uploads(uploads: &SFTPActiveUploads) {
    const TIMEOUT: Duration = Duration::from_secs(15 * 60); // 15 minutes

    let now = SystemTime::now();
    let keys_to_remove: Vec<(String, u32)> = {
        let entries = uploads.lock().await;
        entries
            .iter()
            .filter_map(|(key, upload)| {
                if let Ok(elapsed) = now.duration_since(upload.last_updated) {
                    if elapsed > TIMEOUT {
                        return Some(key.clone());
                    }
                }
                None
            })
            .collect()
    };

    if !keys_to_remove.is_empty() {
        let mut uploads = uploads.lock().await;
        for key in keys_to_remove {
            log::info!("cleaning up abandoned upload: {:?}", key);
            if let Some(file_upload) = uploads.remove(&key) {
                let _ = file_upload.sftp_file.sync_all().await;
            }
        }
    }
}

pub mod actions;
pub mod client;
pub mod connection;
pub mod session;
