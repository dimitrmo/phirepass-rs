use crate::protocol::sftp::{
    SFTPDelete, SFTPDownloadChunk, SFTPDownloadStart, SFTPUploadChunk, SFTPUploadStart,
};
use crate::protocol::web::WebFrameData;
use crate::stats::Stats;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum WebFrameId {
    SessionId(u32),
    ConnectionId(Ulid),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum NodeFrameData {
    Heartbeat {
        stats: Stats,
    },

    Auth {
        token: String,
        node_id: Option<Ulid>,
        version: String,
    },

    AuthResponse {
        node_id: Ulid,
        success: bool,
        version: String,
    },

    OpenTunnel {
        protocol: u8,
        cid: Ulid,
        username: Option<String>,
        password: Option<String>,
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
    },

    TunnelOpened {
        protocol: u8,
        cid: Ulid,
        sid: u32,            // tunnel session id. exists only after we have a tunnel opened
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
    },

    TunnelData {
        protocol: u8,
        cid: Ulid,
        sid: u32,
        data: Bytes,
    },

    TunnelClosed {
        protocol: u8,
        cid: Ulid,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    }, // notify web that tunnel is closed

    SSHWindowResize {
        cid: Ulid,
        sid: u32,
        cols: u32,
        rows: u32,
    },

    SFTPList {
        cid: Ulid,
        path: String,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    },

    SFTPDownloadStart {
        cid: Ulid,
        sid: u32,
        msg_id: Option<u32>,
        download: SFTPDownloadStart,
    },

    SFTPDownloadChunkRequest {
        cid: Ulid,
        sid: u32,
        msg_id: Option<u32>,
        download_id: u32,
        chunk_index: u32,
    },

    SFTPDownloadChunk {
        cid: Ulid,
        sid: u32,
        msg_id: Option<u32>,
        chunk: SFTPDownloadChunk,
    },

    SFTPUploadStart {
        cid: Ulid,
        sid: u32,
        msg_id: Option<u32>,
        upload: SFTPUploadStart,
    },

    SFTPUpload {
        cid: Ulid,
        sid: u32,
        msg_id: Option<u32>,
        chunk: SFTPUploadChunk,
    },

    SFTPDelete {
        cid: Ulid,
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
        id: WebFrameId,
    },

    ConnectionDisconnect {
        cid: Ulid,
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
            NodeFrameData::SFTPDownloadStart { .. } => 32,
            NodeFrameData::SFTPDownloadChunkRequest { .. } => 33,
            NodeFrameData::SFTPDownloadChunk { .. } => 34,
            NodeFrameData::SFTPUploadStart { .. } => 35,
            NodeFrameData::SFTPUpload { .. } => 36,
            NodeFrameData::SFTPDelete { .. } => 37,
            NodeFrameData::Ping { .. } => 40,
            NodeFrameData::Pong { .. } => 41,
            NodeFrameData::WebFrame { .. } => 50,
            NodeFrameData::ConnectionDisconnect { .. } => 60,
        }
    }
}
