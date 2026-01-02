#[cfg(not(target_arch = "wasm32"))]
use crate::protocol::node::NodeFrameData;

use crate::protocol::web::WebFrameData;
use anyhow::anyhow;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::Display;

// Protobuf imports
use prost::Message;
use crate::protocol::generated::phirepass;

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
            encoding: FrameEncoding::JSON,
            data: FrameData::Node(data),
        }
    }
}

impl From<WebFrameData> for Frame {
    fn from(data: WebFrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::JSON,
            data: FrameData::Web(data),
        }
    }
}

impl Frame {
    /// Create a new Frame with JSON encoding (for backward compatibility)
    pub fn new_json(data: FrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::JSON,
            data,
        }
    }

    /// Create a new Frame with Protobuf encoding (optimized)
    pub fn new_protobuf(data: FrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::Protobuf,
            data,
        }
    }

    /// Create a Frame from WebFrameData with Protobuf encoding
    pub fn from_web_protobuf(data: WebFrameData) -> Self {
        Self::new_protobuf(FrameData::Web(data))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Create a Frame from NodeFrameData with Protobuf encoding
    pub fn from_node_protobuf(data: NodeFrameData) -> Self {
        Self::new_protobuf(FrameData::Node(data))
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(u8)]
pub enum FrameEncoding {
    JSON = 0,
    Protobuf = 1,
}

impl Display for FrameEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameEncoding::JSON => write!(f, "JSON"),
            FrameEncoding::Protobuf => write!(f, "Protobuf"),
        }
    }
}

impl TryFrom<u8> for FrameEncoding {
    type Error = anyhow::Error;

    fn try_from(code: u8) -> Result<Self, Self::Error> {
        match code {
            0 => Ok(FrameEncoding::JSON),
            1 => Ok(FrameEncoding::Protobuf),
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

        let payload = data[HEADER_SIZE..HEADER_SIZE + len].to_vec();

        let data = match (frame_kind, encoding.clone()) {
            (0, FrameEncoding::JSON) => {
                let web = serde_json::from_slice::<WebFrameData>(&payload)?;
                FrameData::Web(web)
            }
            (0, FrameEncoding::Protobuf) => {
                let proto_frame = phirepass::frame::Frame::decode(&payload[..])?;
                let web = proto_frame.data
                    .ok_or_else(|| anyhow!("empty protobuf frame data"))?
                    .try_into()?;
                FrameData::Web(web)
            }
            #[cfg(not(target_arch = "wasm32"))]
            (1, FrameEncoding::JSON) => {
                let node = serde_json::from_slice::<NodeFrameData>(&payload)?;
                FrameData::Node(node)
            }
            #[cfg(not(target_arch = "wasm32"))]
            (1, FrameEncoding::Protobuf) => {
                let proto_frame = phirepass::frame::Frame::decode(&payload[..])?;
                let node = proto_frame.data
                    .ok_or_else(|| anyhow!("empty protobuf frame data"))?
                    .try_into()?;
                FrameData::Node(node)
            }

            #[cfg(target_arch = "wasm32")]
            (1_u8..=u8::MAX, _) => anyhow::bail!("invalid frame type"),

            #[cfg(not(target_arch = "wasm32"))]
            (2_u8..=u8::MAX, _) => anyhow::bail!("invalid frame type"),
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
            FrameData::Web(web_data) => match frame_encoding {
                FrameEncoding::JSON => {
                    let raw = serde_json::to_vec(&web_data)?;
                    (raw, 0, web_data.code())
                }
                FrameEncoding::Protobuf => {
                    let proto_data: phirepass::frame::frame::Data = web_data.clone().try_into()?;
                    let proto_frame = phirepass::frame::Frame {
                        data: Some(proto_data),
                    };
                    let mut buf = Vec::new();
                    proto_frame.encode(&mut buf)?;
                    (buf, 0, web_data.code())
                }
            },
            #[cfg(not(target_arch = "wasm32"))]
            FrameData::Node(node_data) => match frame_encoding {
                FrameEncoding::JSON => {
                    let raw = serde_json::to_vec(&node_data)?;
                    (raw, 1, node_data.code())
                }
                FrameEncoding::Protobuf => {
                    let proto_data: phirepass::frame::frame::Data = node_data.clone().try_into()?;
                    let proto_frame = phirepass::frame::Frame {
                        data: Some(proto_data),
                    };
                    let mut buf = Vec::new();
                    proto_frame.encode(&mut buf)?;
                    (buf, 1, node_data.code())
                }
            },
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
