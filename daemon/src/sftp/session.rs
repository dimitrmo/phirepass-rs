use log::{debug, info};
use phirepass_common::protocol::sftp::{SFTPDelete, SFTPUploadChunk, SFTPUploadStart};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

#[derive(Clone, Debug)]
pub(crate) enum SFTPCommand {
    List(String, Option<u32>),
    Download {
        path: String,
        filename: String,
        msg_id: Option<u32>,
    },
    UploadStart {
        upload: SFTPUploadStart,
        msg_id: Option<u32>,
    },
    Upload {
        chunk: SFTPUploadChunk,
        msg_id: Option<u32>,
    },
    Delete {
        data: SFTPDelete,
        msg_id: Option<u32>,
    },
}

#[derive(Debug)]
pub(crate) struct SFTPSessionHandle {
    pub stdin: Sender<SFTPCommand>,
    pub stop: Option<oneshot::Sender<()>>,
}

impl SFTPSessionHandle {
    pub async fn shutdown(mut self) {
        info!("shutting down sftp session");
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
            debug!("sftp self stopped sent");
        }
    }
}
