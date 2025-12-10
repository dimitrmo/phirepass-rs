use std::fmt::Display;

use crate::stats::Stats;
use rmp_serde::{decode, encode};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebControlErrorType {
    Generic = 0,
    RequiresPassword = 100,
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
        Ok(Self::from_u8(value))
    }
}

impl TryFrom<u8> for WebControlErrorType {
    type Error = ();

    fn try_from(value: u8) -> anyhow::Result<Self, Self::Error> {
        Ok(WebControlErrorType::from_u8(value))
    }
}

impl WebControlErrorType {
    pub fn from_u8(n: u8) -> Self {
        match n {
            100 => Self::RequiresPassword,
            _ => Self::Generic,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WebControlMessage {
    Heartbeat, // send from web to server to keep connection alive970705
    OpenTunnel {
        protocol: u8,
        target: String,
        password: Option<String>,
    }, // open a tunnel to target ( by name ) - send form web to server
    TunnelData {
        protocol: u8,
        target: String,
        data: Vec<u8>,
    }, // allow user to send data to specific tunnel
    Resize {
        target: String,
        cols: u32,
        rows: u32,
    }, // resize a tunnel's pty
    Error {
        kind: WebControlErrorType,
        message: String,
    }, // error message
    Ok,        // ack
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
    }, // authentication of a node to the server
    Heartbeat {
        stats: Stats,
    }, // heartbeat with stats that node sends to server
    OpenTunnel {
        protocol: u8,
        cid: String,
        password: Option<String>,
    }, // open a tunnel to target ( by name ) - send from server to daemon
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
    ClientDisconnect {
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

pub const HEADER_LEN: usize = 5; // 1 + sizeof(u32)

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Control = 0,
    SSH = 1,
}

impl Protocol {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(Self::Control),
            1 => Some(Self::SSH),
            _ => None,
        }
    }
}

impl TryFrom<u8> for Protocol {
    type Error = ();

    fn try_from(value: u8) -> anyhow::Result<Self, Self::Error> {
        Protocol::from_u8(value).ok_or(())
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

        let protocol = Protocol::from_u8(data[0])?;
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
