use crate::protocol::web::WebFrameData;
use crate::stats::Stats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[repr(u8)]
pub enum NodeFrameData {
    Heartbeat {
        stats: Stats,
    } = 1,

    Auth {
        token: String,
    } = 10,

    AuthResponse {
        node_id: String,
        success: bool,
        version: String,
    } = 11,

    OpenTunnel {
        protocol: u8,
        cid: String,
        username: String,
        password: String,
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
    } = 20,

    TunnelOpened {
        protocol: u8,
        cid: String,
        sid: u32,
        msg_id: Option<u32>, // custom web user supplied. easier to track responses and map them to requests
    } = 21,

    TunnelData {
        cid: String,
        sid: u32,
        data: Vec<u8>,
    } = 22,

    TunnelClosed {
        cid: String,
        sid: u32,
        msg_id: Option<u32>, // echo back the user supplied msg_id
    } = 23, // notify web that tunnel is closed

    SSHWindowResize {
        cid: String,
        sid: u32,
        cols: u32,
        rows: u32,
    } = 30,

    Ping {
        sent_at: u64,
    } = 40,

    Pong {
        sent_at: u64,
    } = 41,

    WebFrame {
        frame: WebFrameData,
        sid: u32,
    } = 50,

    ConnectionDisconnect {
        cid: String,
    } = 60,
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
            NodeFrameData::Ping { .. } => 40,
            NodeFrameData::Pong { .. } => 41,
            NodeFrameData::WebFrame { .. } => 50,
            NodeFrameData::ConnectionDisconnect { .. } => 60,
        }
    }
}
