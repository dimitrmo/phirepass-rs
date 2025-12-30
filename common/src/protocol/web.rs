use crate::protocol::common::{FrameData, FrameError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum WebFrameData {
    Heartbeat = 1, // send from web to server to keep connection alive

    OpenTunnel {
        protocol: u8,
        target: String,
        msg_id: Option<u64>, // custom web user supplied. easier to track responses and map them to requests
        username: Option<String>, // optional username for auth
        password: Option<String>, // optional password for auth
    } = 20, // open a tunnel to target ( by name ) - send form web to server

    TunnelOpened {
        protocol: u8,
        sid: u64,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 21, // notify web that tunnel is opened

    TunnelData {
        target: String,
        sid: u64,
        data: Vec<u8>,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 22, // bidirectioanal tunnel data

    TunnelClosed {
        sid: u64,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 23, // notify web that tunnel is closed

    SSHWindowResize {
        // send by client
        target: String,
        sid: u64,
        cols: u32,
        rows: u32,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 30, // resize a tunnel's pty ( only for SSH tunnel ) - request sent from web to server

    Error {
        kind: FrameError,
        message: String,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 50, // error message
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
            WebFrameData::Error { .. } => 50,
        }
    }
}

impl Into<FrameData> for WebFrameData {
    fn into(self) -> FrameData {
        FrameData::Web(self)
    }
}
