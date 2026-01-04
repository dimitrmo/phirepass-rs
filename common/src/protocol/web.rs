use crate::protocol::common::FrameError;
use crate::protocol::sftp::{
    SFTPDelete, SFTPDownloadChunk, SFTPDownloadStart, SFTPDownloadStartResponse, SFTPListItem,
    SFTPUploadChunk, SFTPUploadStart, SFTPUploadStartResponse,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WebFrameData {
    Heartbeat, // send from web to server to keep connection alive

    OpenTunnel {
        protocol: u8,
        node_id: String,
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
        username: Option<String>, // optional username for auth
        password: Option<String>, // optional password for auth
    }, // open a tunnel to node by id - send form web to server

    TunnelOpened {
        protocol: u8,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    }, // notify web that tunnel is opened

    TunnelData {
        protocol: u8,
        node_id: String,
        sid: u32,
        data: Vec<u8>,
    }, // bidirectioanal tunnel data

    TunnelClosed {
        protocol: u8,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    }, // notify web that tunnel is closed

    SSHWindowResize {
        node_id: String,
        sid: u32,
        cols: u32,
        rows: u32,
    }, // resize a tunnel's pty ( only for SSH tunnel ) - request sent from web to server

    SFTPList {
        node_id: String,
        path: String,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    },

    SFTPListItems {
        path: String,
        sid: u32,
        dir: SFTPListItem,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    },

    SFTPDownloadStart {
        node_id: String,
        sid: u32,
        msg_id: Option<u32>,
        download: SFTPDownloadStart,
    },

    SFTPDownloadStartResponse {
        sid: u32,
        msg_id: Option<u32>,
        response: SFTPDownloadStartResponse,
    },

    SFTPDownloadChunkRequest {
        node_id: String,
        sid: u32,
        msg_id: Option<u32>,
        download_id: u32,
        chunk_index: u32,
    },

    SFTPDownloadChunk {
        sid: u32,
        msg_id: Option<u32>,
        chunk: SFTPDownloadChunk,
    },

    SFTPUploadStart {
        node_id: String,
        sid: u32,
        msg_id: Option<u32>,
        upload: SFTPUploadStart,
    },

    SFTPUploadStartResponse {
        sid: u32,
        msg_id: Option<u32>,
        response: SFTPUploadStartResponse,
    },

    SFTPUpload {
        node_id: String,
        sid: u32,
        msg_id: Option<u32>,
        chunk: SFTPUploadChunk,
    },

    SFTPUploadChunkAck {
        sid: u32,
        upload_id: u32,
        chunk_index: u32,
    },

    SFTPDelete {
        node_id: String,
        sid: u32,
        msg_id: Option<u32>,
        data: SFTPDelete,
    },

    Error {
        kind: FrameError,
        message: String,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    }, // error message
}

impl WebFrameData {
    pub fn code(&self) -> u8 {
        match self {
            WebFrameData::Heartbeat => 10,
            WebFrameData::OpenTunnel { .. } => 20,
            WebFrameData::TunnelOpened { .. } => 21,
            WebFrameData::TunnelData { .. } => 22,
            WebFrameData::TunnelClosed { .. } => 23,
            WebFrameData::SSHWindowResize { .. } => 30,
            WebFrameData::SFTPList { .. } => 40,
            WebFrameData::SFTPListItems { .. } => 41,
            WebFrameData::SFTPDownloadStart { .. } => 42,
            WebFrameData::SFTPDownloadStartResponse { .. } => 43,
            WebFrameData::SFTPDownloadChunkRequest { .. } => 44,
            WebFrameData::SFTPDownloadChunk { .. } => 45,
            WebFrameData::SFTPUploadStart { .. } => 46,
            WebFrameData::SFTPUploadStartResponse { .. } => 47,
            WebFrameData::SFTPUpload { .. } => 48,
            WebFrameData::SFTPUploadChunkAck { .. } => 49,
            WebFrameData::SFTPDelete { .. } => 50,
            WebFrameData::Error { .. } => 51,
        }
    }
}
