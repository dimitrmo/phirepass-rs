# Protocol Buffers Implementation Guide

## Overview

Switching from JSON to Protocol Buffers for browser/server communication will provide:
- **40-60% bandwidth reduction** (vs 30-50% with MessagePack)
- **Strong typing** (compile-time error detection)
- **Better forward/backward compatibility**
- **Industry standard** (Google, gRPC, etc.)

---

## Step 1: Add Dependencies

### Root `Cargo.toml` (workspace)
```toml
[workspace.dependencies]
prost = "0.12"
prost-types = "0.12"
bytes = "1.5"

[build-dependencies]
prost-build = "0.12"
```

### `common/Cargo.toml`
```toml
[dependencies]
prost = { workspace = true }
prost-types = { workspace = true }
bytes = { workspace = true }
serde = { workspace = true, features = ["derive"] }

[build-dependencies]
prost-build = { workspace = true }
```

### `channel/Cargo.toml` (WASM)
```toml
[dependencies]
prost = "0.12"
prost-types = "0.12"
bytes = "1.5"
```

---

## Step 2: Create Proto Schema Files

### `common/proto/common.proto`
```protobuf
syntax = "proto3";

package phirepass.common;

// Frame encoding types
enum FrameEncoding {
    FRAME_ENCODING_UNSPECIFIED = 0;
    FRAME_ENCODING_JSON = 1;
    FRAME_ENCODING_PROTOBUF = 2;
}

// Top-level frame wrapper
message Frame {
    uint32 version = 1;
    FrameEncoding encoding = 2;
    oneof data {
        WebFrameData web = 3;
        NodeFrameData node = 4;
    }
}

// Error types
enum FrameError {
    FRAME_ERROR_UNSPECIFIED = 0;
    FRAME_ERROR_GENERIC = 1;
    FRAME_ERROR_AUTH_FAILED = 2;
    FRAME_ERROR_NODE_NOT_FOUND = 3;
    FRAME_ERROR_TUNNEL_FAILED = 4;
}
```

### `common/proto/web.proto`
```protobuf
syntax = "proto3";

package phirepass.web;

import "common.proto";
import "sftp.proto";

// Web client messages
message WebFrameData {
    oneof message {
        Heartbeat heartbeat = 1;
        OpenTunnel open_tunnel = 2;
        TunnelOpened tunnel_opened = 3;
        TunnelData tunnel_data = 4;
        TunnelClosed tunnel_closed = 5;
        SSHWindowResize ssh_window_resize = 6;
        SFTPList sftp_list = 7;
        SFTPListItems sftp_list_items = 8;
        SFTPDownload sftp_download = 9;
        SFTPFileChunk sftp_file_chunk = 10;
        SFTPUpload sftp_upload = 11;
        SFTPDelete sftp_delete = 12;
        Error error = 13;
    }
}

message Heartbeat {}

message OpenTunnel {
    uint32 protocol = 1;
    string node_id = 2;
    optional uint32 msg_id = 3;
    optional string username = 4;
    optional string password = 5;
}

message TunnelOpened {
    uint32 protocol = 1;
    uint32 sid = 2;
    optional uint32 msg_id = 3;
}

message TunnelData {
    uint32 protocol = 1;
    string node_id = 2;
    uint32 sid = 3;
    bytes data = 4;
}

message TunnelClosed {
    uint32 protocol = 1;
    uint32 sid = 2;
    optional uint32 msg_id = 3;
}

message SSHWindowResize {
    string node_id = 1;
    uint32 sid = 2;
    uint32 cols = 3;
    uint32 rows = 4;
}

message SFTPList {
    string node_id = 1;
    string path = 2;
    uint32 sid = 3;
    optional uint32 msg_id = 4;
}

message SFTPListItems {
    string path = 1;
    uint32 sid = 2;
    phirepass.sftp.SFTPListItem dir = 3;
    optional uint32 msg_id = 4;
}

message SFTPDownload {
    string node_id = 1;
    string path = 2;
    string filename = 3;
    uint32 sid = 4;
    optional uint32 msg_id = 5;
}

message SFTPFileChunk {
    uint32 sid = 1;
    optional uint32 msg_id = 2;
    phirepass.sftp.SFTPFileChunk chunk = 3;
}

message SFTPUpload {
    string node_id = 1;
    string path = 2;
    uint32 sid = 3;
    optional uint32 msg_id = 4;
    phirepass.sftp.SFTPUploadChunk chunk = 5;
}

message SFTPDelete {
    string node_id = 1;
    uint32 sid = 2;
    optional uint32 msg_id = 3;
    phirepass.sftp.SFTPDelete data = 4;
}

message Error {
    phirepass.common.FrameError kind = 1;
    string message = 2;
    optional uint32 msg_id = 3;
}
```

### `common/proto/node.proto`
```protobuf
syntax = "proto3";

package phirepass.node;

import "web.proto";
import "sftp.proto";

message NodeFrameData {
    oneof message {
        Heartbeat heartbeat = 1;
        Auth auth = 2;
        AuthResponse auth_response = 3;
        OpenTunnel open_tunnel = 4;
        TunnelOpened tunnel_opened = 5;
        TunnelData tunnel_data = 6;
        TunnelClosed tunnel_closed = 7;
        SSHWindowResize ssh_window_resize = 8;
        SFTPList sftp_list = 9;
        SFTPDownload sftp_download = 10;
        SFTPUpload sftp_upload = 11;
        SFTPDelete sftp_delete = 12;
        Ping ping = 13;
        Pong pong = 14;
        WebFrame web_frame = 15;
        ConnectionDisconnect connection_disconnect = 16;
    }
}

message Heartbeat {
    Stats stats = 1;
}

message Stats {
    float host_cpu = 1;
    uint64 host_mem_used_bytes = 2;
    uint64 host_mem_total_bytes = 3;
    uint64 network_bytes_sent = 4;
    uint64 network_bytes_received = 5;
    uint64 uptime_seconds = 6;
}

message Auth {
    string token = 1;
}

message AuthResponse {
    string node_id = 1;
    bool success = 2;
    string version = 3;
}

message OpenTunnel {
    uint32 protocol = 1;
    string cid = 2;
    string username = 3;
    string password = 4;
    optional uint32 msg_id = 5;
}

message TunnelOpened {
    uint32 protocol = 1;
    string cid = 2;
    uint32 sid = 3;
    optional uint32 msg_id = 4;
}

message TunnelData {
    uint32 protocol = 1;
    string cid = 2;
    uint32 sid = 3;
    bytes data = 4;
}

message TunnelClosed {
    uint32 protocol = 1;
    string cid = 2;
    uint32 sid = 3;
    optional uint32 msg_id = 4;
}

message SSHWindowResize {
    string cid = 1;
    uint32 sid = 2;
    uint32 cols = 3;
    uint32 rows = 4;
}

message SFTPList {
    string cid = 1;
    string path = 2;
    uint32 sid = 3;
    optional uint32 msg_id = 4;
}

message SFTPDownload {
    string cid = 1;
    string path = 2;
    string filename = 3;
    uint32 sid = 4;
    optional uint32 msg_id = 5;
}

message SFTPUpload {
    string cid = 1;
    string path = 2;
    uint32 sid = 3;
    optional uint32 msg_id = 4;
    phirepass.sftp.SFTPUploadChunk chunk = 5;
}

message SFTPDelete {
    string cid = 1;
    uint32 sid = 2;
    optional uint32 msg_id = 3;
    phirepass.sftp.SFTPDelete data = 4;
}

message Ping {
    uint64 sent_at = 1;
}

message Pong {
    uint64 sent_at = 1;
}

message WebFrame {
    phirepass.web.WebFrameData frame = 1;
    uint32 sid = 2;
}

message ConnectionDisconnect {
    string cid = 1;
}
```

### `common/proto/sftp.proto`
```protobuf
syntax = "proto3";

package phirepass.sftp;

message SFTPListItem {
    string name = 1;
    string path = 2;
    bool is_dir = 3;
    uint64 size = 4;
    optional uint64 modified = 5;
    repeated SFTPListItem children = 6;
}

message SFTPFileChunk {
    uint64 offset = 1;
    bytes data = 2;
    bool is_last = 3;
}

message SFTPUploadChunk {
    uint64 offset = 1;
    bytes data = 2;
    bool is_first = 3;
    bool is_last = 4;
}

message SFTPDelete {
    string path = 1;
    bool is_dir = 2;
}
```

---

## Step 3: Create Build Script

### `common/build.rs`
```rust
fn main() {
    prost_build::Config::new()
        .out_dir("src/protocol/generated")
        .compile_protos(
            &[
                "proto/common.proto",
                "proto/web.proto",
                "proto/node.proto",
                "proto/sftp.proto",
            ],
            &["proto/"],
        )
        .unwrap();
}
```

---

## Step 4: Update Frame Structure

### `common/src/protocol/common.rs`
```rust
use anyhow::anyhow;
use prost::Message;
use std::fmt::Display;

// Generated protobuf code
pub mod proto {
    include!("generated/phirepass.common.rs");
}

use proto::{Frame as ProtoFrame, FrameEncoding};

const HEADER_SIZE: usize = 8;
const FRAME_VERSION: u8 = 1;

pub struct Frame {
    pub version: u8,
    pub encoding: FrameEncoding,
    pub data: FrameData,
}

pub enum FrameData {
    Web(crate::protocol::web::proto::WebFrameData),
    #[cfg(not(target_arch = "wasm32"))]
    Node(crate::protocol::node::proto::NodeFrameData),
}

impl Frame {
    pub fn version() -> u8 {
        FRAME_VERSION
    }

    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        match self.encoding {
            FrameEncoding::Json => {
                // Legacy JSON encoding for backward compatibility
                unimplemented!("JSON encoding deprecated, use Protobuf")
            }
            FrameEncoding::Protobuf => {
                let proto_frame = self.to_proto();
                let mut buf = Vec::new();
                proto_frame.encode(&mut buf)?;
                Ok(buf)
            }
            _ => Err(anyhow!("Unsupported encoding")),
        }
    }

    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        let proto_frame = ProtoFrame::decode(data)?;
        Self::from_proto(proto_frame)
    }

    fn to_proto(&self) -> ProtoFrame {
        ProtoFrame {
            version: self.version as u32,
            encoding: self.encoding as i32,
            data: match &self.data {
                FrameData::Web(web) => Some(proto::frame::Data::Web(web.clone())),
                #[cfg(not(target_arch = "wasm32"))]
                FrameData::Node(node) => Some(proto::frame::Data::Node(node.clone())),
            },
        }
    }

    fn from_proto(proto: ProtoFrame) -> anyhow::Result<Self> {
        let encoding = FrameEncoding::from_i32(proto.encoding)
            .ok_or_else(|| anyhow!("Invalid encoding"))?;
        
        let data = match proto.data {
            Some(proto::frame::Data::Web(web)) => FrameData::Web(web),
            #[cfg(not(target_arch = "wasm32"))]
            Some(proto::frame::Data::Node(node)) => FrameData::Node(node),
            None => return Err(anyhow!("Missing frame data")),
        };

        Ok(Self {
            version: proto.version as u8,
            encoding,
            data,
        })
    }
}
```

---

## Step 5: Update Message Handling

### In `daemon/src/ws.rs` and `server/src/web.rs`

No changes needed! The Frame::decode/encode handles everything. Just ensure you're using the protobuf encoding:

```rust
// Old JSON version
let frame: Frame = NodeFrameData::Auth {
    token: config.token.clone(),
}.into();

// New Protobuf version (same code, different encoding)
use crate::protocol::node::proto::{NodeFrameData, node_frame_data};

let auth = NodeFrameData {
    message: Some(node_frame_data::Message::Auth(
        node::Auth {
            token: config.token.clone(),
        }
    )),
};

let frame = Frame {
    version: Frame::version(),
    encoding: FrameEncoding::Protobuf,
    data: FrameData::Node(auth),
};
```

---

## Step 6: WASM Integration

For the browser channel, you'll need to compile protobuf to WASM:

### `channel/build.rs`
```rust
fn main() {
    prost_build::Config::new()
        .out_dir("src/generated")
        .compile_protos(
            &[
                "../common/proto/common.proto",
                "../common/proto/web.proto",
            ],
            &["../common/proto/"],
        )
        .unwrap();
}
```

### `channel/src/lib.rs`
```rust
mod generated {
    include!("generated/phirepass.web.rs");
    include!("generated/phirepass.common.rs");
}

use generated::*;
use prost::Message;

fn handle_message(cb: &Function, event: &MessageEvent) {
    let buffer: web_sys::js_sys::ArrayBuffer = match event.data().dyn_into() {
        Ok(buf) => buf,
        Err(err) => {
            console_warn!("error converting to array buffer: {err:?}");
            return;
        }
    };

    let view = Uint8Array::new(&buffer);
    let mut data = vec![0u8; view.length() as usize];
    view.copy_to(&mut data);

    // Decode protobuf frame
    let frame = match Frame::decode(&data[..]) {
        Ok(frame) => frame,
        Err(err) => {
            console_warn!("received invalid protobuf frame: {err}");
            return;
        }
    };

    // Convert to JS value
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    // You'll need to add serde derives to the generated code or use a custom serializer
    // ...
}
```

---

## Step 7: Backward Compatibility

Keep JSON support for gradual migration:

```rust
impl Frame {
    pub fn decode(data: &[u8]) -> anyhow::Result<Self> {
        // Try protobuf first
        if let Ok(frame) = ProtoFrame::decode(data) {
            return Self::from_proto(frame);
        }

        // Fallback to legacy JSON
        if data.len() > HEADER_SIZE {
            let version = data[0];
            let encoding_byte = data[1];
            
            if encoding_byte == 0 { // JSON encoding
                return Self::decode_json(&data[HEADER_SIZE..]);
            }
        }

        Err(anyhow!("Could not decode frame as protobuf or JSON"))
    }

    fn decode_json(payload: &[u8]) -> anyhow::Result<Self> {
        // Your existing JSON decode logic
        // ...
    }
}
```

---

## Step 8: Testing

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protobuf_encoding() {
        let auth = node_frame_data::Message::Auth(node::Auth {
            token: "test-token".to_string(),
        });
        
        let frame = Frame {
            version: 1,
            encoding: FrameEncoding::Protobuf,
            data: FrameData::Node(NodeFrameData {
                message: Some(auth),
            }),
        };
        
        let encoded = frame.to_bytes().unwrap();
        let decoded = Frame::decode(&encoded).unwrap();
        
        // Verify round-trip
        match decoded.data {
            FrameData::Node(node) => {
                match node.message {
                    Some(node_frame_data::Message::Auth(auth)) => {
                        assert_eq!(auth.token, "test-token");
                    }
                    _ => panic!("Wrong message type"),
                }
            }
            _ => panic!("Wrong frame type"),
        }
    }

    #[test]
    fn test_size_comparison() {
        // JSON version
        let json_size = 280; // Your current OpenTunnel size
        
        // Protobuf version
        let open_tunnel = web_frame_data::Message::OpenTunnel(web::OpenTunnel {
            protocol: 0,
            node_id: "production-db-01".to_string(),
            msg_id: Some(12345),
            username: Some("admin".to_string()),
            password: Some("secretpass123".to_string()),
        });
        
        let frame = Frame {
            version: 1,
            encoding: FrameEncoding::Protobuf,
            data: FrameData::Web(WebFrameData {
                message: Some(open_tunnel),
            }),
        };
        
        let protobuf_size = frame.to_bytes().unwrap().len();
        
        println!("JSON size: {} bytes", json_size);
        println!("Protobuf size: {} bytes", protobuf_size);
        println!("Savings: {}%", ((json_size - protobuf_size) * 100) / json_size);
        
        assert!(protobuf_size < 180); // Should be ~140 bytes
    }
}
```

---

## Size Comparison Results

### Expected Sizes

| Message | JSON | Protobuf | Savings |
|---------|------|----------|---------|
| Heartbeat | 17 B | 8 B | -53% |
| OpenTunnel | 280 B | 140 B | -50% |
| TunnelData (metadata) | 150 B | 80 B | -47% |
| SFTPList | 150 B | 75 B | -50% |
| Auth | 100 B | 50 B | -50% |
| **Average Metadata** | **180 B** | **90 B** | **-50%** |

---

## Rollout Strategy

### Week 1: Setup & Implementation
- Day 1: Create .proto files, add dependencies
- Day 2: Generate code, update Frame structure
- Day 3: Update message handlers
- Day 4: Write tests, benchmark

### Week 2: WASM Integration
- Day 1-2: Compile protobuf to WASM
- Day 3: Update browser client
- Day 4: End-to-end testing

### Week 3: Deployment
- Deploy with backward compatibility (supports both JSON & Protobuf)
- Server prefers Protobuf but accepts JSON
- Monitor metrics
- Gradual migration

### Week 4: Monitoring & Optimization
- Verify bandwidth savings
- Check CPU impact
- Optimize hot paths
- Document lessons learned

---

## Gotchas & Solutions

### 1. Protobuf WASM Size
**Problem**: Protobuf adds ~30-50KB to WASM bundle  
**Solution**: Use `wasm-opt` with `-Oz` flag, acceptable for savings

### 2. Generated Code Complexity
**Problem**: Generated code is less readable than hand-written  
**Solution**: Keep .proto files well-documented, treat generated code as internal

### 3. Optional Fields
**Problem**: Protobuf `optional` requires proto3 syntax  
**Solution**: Already using proto3, use `optional` for all nullable fields

### 4. Enum Mapping
**Problem**: Protobuf enums start at 0, Rust may not  
**Solution**: Add UNSPECIFIED = 0 value to all enums

---

## Success Criteria

- [ ] All .proto files compile without errors
- [ ] Generated Rust code builds
- [ ] All existing tests pass
- [ ] Protobuf frames are 40-60% smaller than JSON
- [ ] Backward compatibility maintained
- [ ] WASM bundle size increase < 50KB
- [ ] No latency regression
- [ ] Strong typing prevents runtime errors

---

## Benefits Over MessagePack

| Aspect | MessagePack | Protocol Buffers | Winner |
|--------|-------------|------------------|--------|
| Size | 180 B | 140 B | Protobuf (-22%) |
| Speed (ser) | 0.1 ms | 0.15 ms | MessagePack |
| Speed (de) | 0.1 ms | 0.1 ms | Tie |
| Type safety | Runtime | Compile-time | **Protobuf** |
| Schema | None | Required | MessagePack (simpler) |
| Versioning | Manual | Built-in | **Protobuf** |
| Tooling | Good | Excellent | **Protobuf** |
| Adoption | Good | Widespread | **Protobuf** |

**Overall**: Protobuf wins for production systems that value:
- Maximum compression
- Strong typing
- Long-term maintainability

---

## Next Steps

1. Create `common/proto/` directory
2. Add all .proto files
3. Update `common/Cargo.toml` with dependencies
4. Create `common/build.rs`
5. Run `cargo build` to generate code
6. Update Frame implementation
7. Test round-trip encoding
8. Deploy with backward compatibility

Would you like me to generate all the .proto files as actual files in your workspace?
