use std::fmt::Display;

use crate::stats::Stats;
use rmp_serde::{decode, encode};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WebControlErrorType {
    Generic = 0,
    RequiresPassword = 100,
    RequiresUsernamePassword = 110,
}

impl Serialize for WebControlErrorType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for WebControlErrorType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Ok(Self::from(value))
    }
}

impl From<u8> for WebControlErrorType {
    fn from(value: u8) -> Self {
        match value {
            100 => Self::RequiresPassword,
            110 => Self::RequiresUsernamePassword,
            _ => Self::Generic,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[repr(u8)]
pub enum WebControlMessage {
    Heartbeat = 10, // send from web to server to keep connection alive970705
    OpenTunnel {
        protocol: u8,
        target: String,
        username: Option<String>,
        password: Option<String>,
    } = 20, // open a tunnel to target ( by name ) - send form web to server
    TunnelData {
        protocol: u8,
        target: String,
        data: Vec<u8>,
    } = 21,
    TunnelOpened {
        protocol: u8,
        sid: u64,
    } = 22,
    TunnelClosed {
        protocol: u8,
        sid: u64,
    } = 23,
    Resize {
        target: String,
        cols: u32,
        rows: u32,
    } = 30, // resize a tunnel's pty
    Error {
        kind: WebControlErrorType,
        message: String,
    } = 40, // error message
    Ok = 50, // ack
}

impl From<WebControlErrorType> for u8 {
    fn from(value: WebControlErrorType) -> Self {
        value as u8
    }
}

pub fn generic_web_error(msg: impl Into<String>) -> WebControlMessage {
    WebControlMessage::Error {
        kind: WebControlErrorType::Generic,
        message: msg.into(),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum NodeControlMessage {
    Auth {
        token: String,
    },
    AuthResponse {
        cid: String,
        success: bool,
        version: String,
    },
    Heartbeat {
        stats: Stats,
    },
    OpenTunnel {
        protocol: u8,
        cid: String,
        username: String,
        password: String,
    },
    TunnelOpened {
        protocol: u8,
        cid: String,
        sid: u64,
    },
    TunnelData {
        protocol: u8,
        cid: String,
        data: Vec<u8>,
    },

    Resize {
        cid: String,
        cols: u32,
        rows: u32,
    },
    Ping {
        sent_at: u64,
    }, // ping request with send timestamp
    Pong {
        sent_at: u64,
    }, // pong response with send timestamp
    ConnectionDisconnect {
        cid: String,
    }, // notify node for client disconnect
    Error {
        message: String,
    }, // error message
    Frame {
        frame: Frame,
        cid: String,
    }, // any messages directed to client id
    Ok, // ack
}

/*

pub const HEADER_LEN: usize = 5; // 1 + sizeof(u32)

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Control = 0,
    SSH = 1,
}

impl From<u8> for Protocol {
    fn from(value: u8) -> Self {
        match value {
            0 => Protocol::Control,
            1 => Protocol::SSH,
            _ => Protocol::Control, // default to Control
        }
    }
}

impl Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::Control => write!(f, "Control"),
            Protocol::SSH => write!(f, "SSH"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Frame {
    pub protocol: Protocol,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(protocol: Protocol, payload: Vec<u8>) -> Self {
        Self { protocol, payload }
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_LEN {
            return None;
        }

        let protocol = Protocol::from(data[0]);
        let len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
        if data.len() < HEADER_LEN + len {
            return None;
        }

        let payload = data[HEADER_LEN..HEADER_LEN + len].to_vec();
        Some(Self { protocol, payload })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(HEADER_LEN + self.payload.len());
        buf.push(self.protocol as u8);
        buf.extend_from_slice(&(self.payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }
}

pub fn encode_node_control(data: &NodeControlMessage) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::new();
    encode::write(&mut buf, &data)?;
    Ok(buf.into())
}

pub fn decode_node_control(payload: &[u8]) -> anyhow::Result<NodeControlMessage> {
    let decoded: NodeControlMessage = decode::from_slice(&payload)?;
    Ok(decoded)
}

pub fn encode_web_control_to_frame(msg: &WebControlMessage) -> serde_json::Result<Frame> {
    serde_json::to_vec(msg).map(|payload| Frame::new(Protocol::Control, payload))
}

pub fn decode_web_control(payload: &[u8]) -> serde_json::Result<WebControlMessage> {
    serde_json::from_slice(payload)
}

*/

// --------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Frame {
    pub version: u8,
    pub encoding: FrameEncoding,
    pub data: FrameData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FrameData {
    Web(WebFrameData),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum FrameEncoding {
    MsgPack = 1,
    JSON = 0,
}

impl Frame {
    pub fn encode(frame: Frame) -> anyhow::Result<Vec<u8>> {
        let data = match &frame.data {
            FrameData::Web(data) => match frame.encoding {
                FrameEncoding::MsgPack => {
                    let mut buf = Vec::new();
                    encode::write(&mut buf, &data)?;
                    (buf, data.code())
                }
                FrameEncoding::JSON => {
                    let raw = serde_json::to_vec(&data)?;
                    (raw, data.code())
                }
            },
        };

        let mut buf = Vec::with_capacity(8 + data.0.len());
        buf.push(1u8); // version - 1
        buf.push(frame.encoding as u8); // encoding - 1
        buf.push(data.1); // kind - 1
        buf.push(0u8); // reserved - 1
        buf.extend_from_slice(&(data.0.len() as u32).to_be_bytes()); // data size - 4
        buf.extend_from_slice(&data.0); // data payload - variable
        Ok(buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum WebFrameData {
    Heartbeat = 10, // send from web to server to keep connection alive

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
        sid: u64,
        data: Vec<u8>,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 22, // bidirectioanal tunnel data

    TunnelClosed {
        sid: u64,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 23, // notify web that tunnel is closed

    SSHWindowResize {
        sid: u64,
        cols: u32,
        rows: u32,
    } = 30, // resize a tunnel's pty ( only for SSH tunnel ) - request sent from web to server

    Error {
        kind: WebControlErrorType,
        message: String,
        msg_id: Option<u64>, // echo back the user supplied msg_id
    } = 50, // error message
}

impl WebFrameData {
    pub fn encode(self) -> anyhow::Result<Vec<u8>> {
        let frame = Frame {
            version: 1,
            encoding: FrameEncoding::JSON,
            data: self.into(),
        };

        Frame::encode(frame)
    }

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

impl Into<u8> for WebFrameData {
    fn into(self) -> u8 {
        self.code()
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_frame_encode_heartbeat() {
        let frame = Frame {
            version: 1,
            encoding: FrameEncoding::JSON,
            data: FrameData::Web(WebFrameData::Heartbeat)
        };

        let encoded = Frame::encode(frame).unwrap();
        assert!(!encoded.is_empty());
        println!("Encoded heartbeat frame: {:?}", encoded);
        assert_eq!(encoded.len(), 19); // 8 bytes header + 11 bytes payload
    }

    #[test]
    fn test_webframe_encode_heartbeat() {
        let web_frame = WebFrameData::Heartbeat;
        let encoded = web_frame.encode().unwrap();
        assert!(!encoded.is_empty());
        println!("Encoded heartbeat web frame: {:?}", encoded);
        assert_eq!(encoded.len(), 19); // 8 bytes header + 11 bytes payload
    }

    #[test]
    fn test_frame_encode_heartbeat_msgpack() {
        let frame = Frame {
            version: 1,
            encoding: FrameEncoding::MsgPack,
            data: WebFrameData::Heartbeat.into(),
        };

        let encoded = Frame::encode(frame).unwrap();
        assert!(!encoded.is_empty());
        println!("Encoded heartbeat frame: {:?}", encoded);
        assert_eq!(encoded.len(), 18); // 8 bytes header + 7 bytes payload
    }
}
