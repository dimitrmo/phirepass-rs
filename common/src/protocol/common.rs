use std::fmt::Display;
use crate::protocol::node::NodeFrameData;
use crate::protocol::web::WebFrameData;
use anyhow::anyhow;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use log::info;

const HEADER_SIZE: usize = 8;
const VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Frame {
    pub version: u8,
    pub encoding: FrameEncoding,
    pub data: FrameData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FrameData {
    Web(WebFrameData),
    Node(NodeFrameData),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum FrameEncoding {
    JSON = 0,
}

impl Display for FrameEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameEncoding::JSON => write!(f, "JSON"),
        }
    }
}

impl TryFrom<u8> for FrameEncoding {
    type Error = anyhow::Error;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        match code {
            0 => Ok(FrameEncoding::JSON),
            _ => Err(anyhow!("unknown frame type")),
        }
    }
}

impl Frame {
    pub fn version() -> u8 {
        VERSION
    }
    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < HEADER_SIZE {
            anyhow::bail!("invalid frame size")
        }

        let version = data[0];
        info!("\tversion: {}", version);
        let encoding = FrameEncoding::try_from(data[1])?;
        info!("\tencoding: {}", encoding);
        let frame_kind = data[2]; // web or node 0 for web 1 for node
        info!("\tframe kind: {}", frame_kind);
        let _frame_code = data[3]; // remains unused when decoding
        info!("\tframe code: {}", _frame_code);
        let len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        info!("\tlength: {}", len);

        if data.len() < HEADER_SIZE + len {
            info!("\tdata length: {}", data.len());
            anyhow::bail!("corrupt frame data")
        }

        let payload = data[HEADER_SIZE..HEADER_SIZE + len].to_vec();

        let data = match frame_kind {
            0 => {
                let web = serde_json::from_slice::<WebFrameData>(&payload)?;
                FrameData::Web(web)
            },
            1 => {
                let node = serde_json::from_slice::<NodeFrameData>(&payload)?;
                FrameData::Node(node)
            }
            1_u8..=u8::MAX => panic!("invalid frame type"),
        };

        Ok(Self {
            encoding,
            version,
            data,
        })
    }
    pub fn encode(frame: &Frame) -> anyhow::Result<Vec<u8>> {
        let frame_encoding = frame.encoding.clone();

        let (data, kind, code) = match &frame.data {
            FrameData::Web(data) => match frame_encoding {
                FrameEncoding::JSON => {
                    let raw = serde_json::to_vec(&data)?;
                    (raw, 0, data.code())
                }
            },
            FrameData::Node(data) => match frame_encoding {
                FrameEncoding::JSON => {
                    let raw = serde_json::to_vec(&data)?;
                    (raw, 1, data.code())
                }
            },
        };

        let mut buf = Vec::with_capacity(HEADER_SIZE + data.len());
        buf.push(VERSION); // version - 1
        buf.push(frame_encoding as u8); // encoding - 1
        buf.push(kind); // web or node - 1
        buf.push(code); // code - 1 - heartbeat, open tunnel etc
        buf.extend_from_slice(&(data.len() as u32).to_be_bytes()); // data size - 4
        buf.extend_from_slice(&data); // data payload - variable
        Ok(buf)
    }

    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        Self::encode(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameError {
    Generic = 0,
    RequiresPassword = 100,
    RequiresUsernamePassword = 110,
}

impl Serialize for FrameError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for FrameError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Ok(Self::from(value))
    }
}

impl From<u8> for FrameError {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Generic,
            100 => Self::RequiresPassword,
            110 => Self::RequiresUsernamePassword,
            _ => Self::Generic,
        }
    }
}
