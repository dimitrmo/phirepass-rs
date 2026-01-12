#[cfg(not(target_arch = "wasm32"))]
use crate::protocol::node::NodeFrameData;

use crate::protocol::web::WebFrameData;
use anyhow::anyhow;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Display;

const HEADER_SIZE: usize = 8;
const FRAME_VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Frame {
    pub version: u8,
    pub encoding: FrameEncoding,
    pub data: FrameData,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<NodeFrameData> for Frame {
    fn from(data: NodeFrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::MessagePack,
            data: FrameData::Node(data),
        }
    }
}

impl From<WebFrameData> for Frame {
    fn from(data: WebFrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::MessagePack,
            data: FrameData::Web(data),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FrameData {
    #[serde(rename = "web")]
    Web(WebFrameData),
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(rename = "node")]
    Node(NodeFrameData),
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[repr(u8)]
pub enum FrameEncoding {
    JSON = 0,
    MessagePack = 1,
}

impl Display for FrameEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameEncoding::JSON => write!(f, "JSON"),
            FrameEncoding::MessagePack => write!(f, "MessagePack"),
        }
    }
}

impl TryFrom<u8> for FrameEncoding {
    type Error = anyhow::Error;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        match code {
            0 => Ok(FrameEncoding::JSON),
            1 => Ok(FrameEncoding::MessagePack),
            _ => Err(anyhow!("unknown frame encoding: {}", code)),
        }
    }
}

impl Frame {
    pub fn version() -> u8 {
        FRAME_VERSION
    }
    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        if data.len() < HEADER_SIZE {
            anyhow::bail!("invalid frame size")
        }

        let version = data[0];
        let encoding = FrameEncoding::try_from(data[1])?;
        let frame_kind = data[2]; // web or node 0 for web 1 for node
        let _frame_code = data[3]; // remains unused when decoding
        let len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;

        if data.len() < HEADER_SIZE + len {
            anyhow::bail!("corrupt frame data")
        }

        let payload = &data[HEADER_SIZE..HEADER_SIZE + len];

        let data = match frame_kind {
            0 => {
                let web = match encoding {
                    FrameEncoding::JSON => serde_json::from_slice::<WebFrameData>(payload)?,
                    FrameEncoding::MessagePack => rmp_serde::from_slice::<WebFrameData>(payload)?,
                };
                FrameData::Web(web)
            }
            #[cfg(not(target_arch = "wasm32"))]
            1 => {
                let node = match encoding {
                    FrameEncoding::JSON => serde_json::from_slice::<NodeFrameData>(payload)?,
                    FrameEncoding::MessagePack => rmp_serde::from_slice::<NodeFrameData>(payload)?,
                };
                FrameData::Node(node)
            }

            #[cfg(target_arch = "wasm32")]
            1_u8..=u8::MAX => anyhow::bail!("invalid frame type"),

            #[cfg(not(target_arch = "wasm32"))]
            2_u8..=u8::MAX => anyhow::bail!("invalid frame type"),
        };

        Ok(Self {
            encoding,
            version,
            data,
        })
    }

    pub fn encode(frame: &Frame) -> anyhow::Result<Vec<u8>> {
        let frame_encoding = frame.encoding;

        let (data, kind, code) = match &frame.data {
            FrameData::Web(data) => {
                let raw = match frame_encoding {
                    FrameEncoding::JSON => serde_json::to_vec(&data)?,
                    FrameEncoding::MessagePack => rmp_serde::to_vec(&data)?,
                };
                (raw, 0, data.code())
            }
            #[cfg(not(target_arch = "wasm32"))]
            FrameData::Node(data) => {
                let raw = match frame_encoding {
                    FrameEncoding::JSON => serde_json::to_vec(&data)?,
                    FrameEncoding::MessagePack => rmp_serde::to_vec(&data)?,
                };
                (raw, 1, data.code())
            }
        };

        let mut buf = Vec::with_capacity(HEADER_SIZE + data.len());
        buf.push(FRAME_VERSION); // version - 1
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
    RequiresUsername = 100,
    RequiresPassword = 110,
    RequiresUsernamePassword = 120,
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
            100 => Self::RequiresUsername,
            110 => Self::RequiresPassword,
            120 => Self::RequiresUsernamePassword,
            _ => Self::Generic,
        }
    }
}
