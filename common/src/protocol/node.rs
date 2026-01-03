use crate::protocol::sftp::{SFTPDelete, SFTPUploadChunk, SFTPUploadStart};
use crate::protocol::web::WebFrameData;
use crate::stats::Stats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum NodeFrameData {
    Heartbeat {
        stats: Stats,
    },

    Auth {
        token: String,
    },

    AuthResponse {
        node_id: String,
        success: bool,
        version: String,
    },

    OpenTunnel {
        protocol: u8,
        cid: String,
        username: String,
        password: String,
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
    },

    TunnelOpened {
        protocol: u8,
        cid: String,
        sid: u32,
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
    },

    TunnelData {
        protocol: u8,
        cid: String,
        sid: u32,
        data: Vec<u8>,
    },

    TunnelClosed {
        protocol: u8,
        cid: String,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    }, // notify web that tunnel is closed

    SSHWindowResize {
        cid: String,
        sid: u32,
        cols: u32,
        rows: u32,
    },

    SFTPList {
        cid: String,
        path: String,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    },

    SFTPDownload {
        cid: String,
        path: String,
        filename: String,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    },

    SFTPUploadStart {
        cid: String,
        sid: u32,
        msg_id: Option<u32>,
        upload: SFTPUploadStart,
    },

    SFTPUpload {
        cid: String,
        sid: u32,
        msg_id: Option<u32>,
        chunk: SFTPUploadChunk,
    },

    SFTPDelete {
        cid: String,
        sid: u32,
        msg_id: Option<u32>,
        data: SFTPDelete,
    },

    Ping {
        sent_at: u64,
    },

    Pong {
        sent_at: u64,
    },

    WebFrame {
        frame: WebFrameData,
        sid: u32,
    },

    ConnectionDisconnect {
        cid: String,
    },
}

impl NodeFrameData {
    pub fn code(&self) -> u8 {
        match self {
            NodeFrameData::Heartbeat { .. } => 1,
            NodeFrameData::Auth { .. } => 10,
            NodeFrameData::AuthResponse { .. } => 11,
            NodeFrameData::OpenTunnel { .. } => 20,
            NodeFrameData::TunnelOpened { .. } => 21,
            NodeFrameData::TunnelData { .. } => 22,
            NodeFrameData::TunnelClosed { .. } => 23,
            NodeFrameData::SSHWindowResize { .. } => 30,
            NodeFrameData::SFTPList { .. } => 31,
            NodeFrameData::SFTPDownload { .. } => 32,
            NodeFrameData::SFTPUploadStart { .. } => 33,
            NodeFrameData::SFTPUpload { .. } => 34,
            NodeFrameData::SFTPDelete { .. } => 35,
            NodeFrameData::Ping { .. } => 40,
            NodeFrameData::Pong { .. } => 41,
            NodeFrameData::WebFrame { .. } => 50,
            NodeFrameData::ConnectionDisconnect { .. } => 60,
        }
    }
}
