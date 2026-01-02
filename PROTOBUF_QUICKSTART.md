# Protocol Buffers Quick Start Guide

## What We've Set Up

✅ Created `.proto` schema files in `common/proto/`:
- `common.proto` - Base types (FrameEncoding, FrameError)
- `sftp.proto` - SFTP message types
- `web.proto` - Browser/web client messages
- `node.proto` - Daemon node messages
- `frame.proto` - Top-level frame wrapper

✅ Created `common/build.rs` - Generates Rust code from .proto files

✅ Updated `Cargo.toml` files with Protocol Buffers dependencies:
- `prost` - Protocol Buffers runtime
- `prost-types` - Well-known protobuf types
- `prost-build` - Code generation

---

## Next Steps to Complete Implementation

### 1. Build and Generate Code (2 minutes)

```bash
cd /home/hellish/Projects/source/dim/phirepass/phirepass-rs/common
cargo build
```

This will:
- Compile the .proto files
- Generate Rust code in `src/protocol/generated/`
- Create: `phirepass.common.rs`, `phirepass.web.rs`, `phirepass.node.rs`, etc.

### 2. Create Protocol Module (5 minutes)

Create `common/src/protocol/generated/mod.rs`:
```rust
// Generated protobuf modules
pub mod phirepass {
    pub mod common {
        include!("phirepass.common.rs");
    }
    pub mod sftp {
        include!("phirepass.sftp.rs");
    }
    pub mod web {
        include!("phirepass.web.rs");
    }
    pub mod node {
        include!("phirepass.node.rs");
    }
    pub mod frame {
        include!("phirepass.frame.rs");
    }
}
```

### 3. Update `common/src/protocol/common.rs` (30 minutes)

Replace your existing Frame implementation with protobuf-based one:

```rust
use anyhow::{anyhow, Result};
use prost::Message;

// Import generated protobuf types
use crate::protocol::generated::phirepass;
use phirepass::common::FrameEncoding;
use phirepass::frame::Frame as ProtoFrame;

pub use phirepass::common::FrameError;

// Re-export for convenience
pub use phirepass::web::WebFrameData;
#[cfg(not(target_arch = "wasm32"))]
pub use phirepass::node::NodeFrameData;

pub struct Frame {
    pub version: u8,
    pub encoding: FrameEncoding,
    pub data: FrameData,
}

pub enum FrameData {
    Web(WebFrameData),
    #[cfg(not(target_arch = "wasm32"))]
    Node(NodeFrameData),
}

impl Frame {
    pub fn version() -> u8 {
        1
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let proto = self.to_proto();
        let mut buf = Vec::new();
        proto.encode(&mut buf)?;
        Ok(buf)
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let proto = ProtoFrame::decode(data)?;
        Self::from_proto(proto)
    }

    fn to_proto(&self) -> ProtoFrame {
        use phirepass::frame::frame;
        
        ProtoFrame {
            version: self.version as u32,
            encoding: self.encoding as i32,
            data: match &self.data {
                FrameData::Web(web) => Some(frame::Data::Web(web.clone())),
                #[cfg(not(target_arch = "wasm32"))]
                FrameData::Node(node) => Some(frame::Data::Node(node.clone())),
            },
        }
    }

    fn from_proto(proto: ProtoFrame) -> Result<Self> {
        use phirepass::frame::frame;
        
        let encoding = FrameEncoding::try_from(proto.encoding)
            .map_err(|_| anyhow!("Invalid encoding: {}", proto.encoding))?;

        let data = match proto.data {
            Some(frame::Data::Web(web)) => FrameData::Web(web),
            #[cfg(not(target_arch = "wasm32"))]
            Some(frame::Data::Node(node)) => FrameData::Node(node),
            None => return Err(anyhow!("Missing frame data")),
        };

        Ok(Self {
            version: proto.version as u8,
            encoding,
            data,
        })
    }
}

// Convenience conversions
impl From<WebFrameData> for Frame {
    fn from(data: WebFrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::Protobuf,
            data: FrameData::Web(data),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<NodeFrameData> for Frame {
    fn from(data: NodeFrameData) -> Self {
        Self {
            version: Self::version(),
            encoding: FrameEncoding::Protobuf,
            data: FrameData::Node(data),
        }
    }
}
```

### 4. Update Message Construction (1-2 hours)

**Old way (JSON-based enums)**:
```rust
// daemon/src/ws.rs
let frame: Frame = NodeFrameData::Auth {
    token: config.token.clone(),
}.into();
```

**New way (Protobuf)**:
```rust
use phirepass_common::protocol::generated::phirepass::node::{
    NodeFrameData,
    node_frame_data,
    Auth,
};

let auth_msg = NodeFrameData {
    message: Some(node_frame_data::Message::Auth(Auth {
        token: config.token.clone(),
    })),
};

let frame = Frame::from(auth_msg);
```

### 5. Update All Message Handlers (2-3 hours)

Pattern matching changes from:
```rust
match data {
    NodeFrameData::Auth { token } => { ... }
    NodeFrameData::Pong { sent_at } => { ... }
}
```

To:
```rust
use node_frame_data::Message;

match data.message {
    Some(Message::Auth(auth)) => {
        let token = auth.token;
        // ...
    }
    Some(Message::Pong(pong)) => {
        let sent_at = pong.sent_at;
        // ...
    }
    None => warn!("Empty node frame data"),
}
```

### 6. Testing Strategy

**Step 1**: Build and verify generation
```bash
cargo build --package phirepass-common
ls common/src/protocol/generated/
# Should see phirepass.*.rs files
```

**Step 2**: Create test
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_protobuf_roundtrip() {
        use phirepass::node::{node_frame_data, Auth, NodeFrameData};
        
        let original = NodeFrameData {
            message: Some(node_frame_data::Message::Auth(Auth {
                token: "test-token".to_string(),
            })),
        };
        
        let frame = Frame::from(original.clone());
        let bytes = frame.to_bytes().unwrap();
        let decoded = Frame::decode(&bytes).unwrap();
        
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
}
```

**Step 3**: Compare sizes
```rust
#[test]
fn test_size_comparison() {
    // Create typical OpenTunnel message
    use phirepass::web::{web_frame_data, OpenTunnel, WebFrameData};
    
    let msg = WebFrameData {
        message: Some(web_frame_data::Message::OpenTunnel(OpenTunnel {
            protocol: 0,
            node_id: "production-db-01".to_string(),
            msg_id: Some(12345),
            username: Some("admin".to_string()),
            password: Some("secretpass123".to_string()),
        })),
    };
    
    let frame = Frame::from(msg);
    let size = frame.to_bytes().unwrap().len();
    
    println!("Protobuf OpenTunnel size: {} bytes", size);
    assert!(size < 150); // Should be ~140 bytes vs 280 with JSON
}
```

---

## Common Migration Patterns

### Pattern 1: Simple Messages
**Before**:
```rust
NodeFrameData::Ping { sent_at }
```

**After**:
```rust
NodeFrameData {
    message: Some(node_frame_data::Message::Ping(Ping {
        sent_at,
    })),
}
```

### Pattern 2: Optional Fields
**Before**:
```rust
WebFrameData::OpenTunnel {
    protocol,
    node_id,
    msg_id: Some(123),
    username: None,
    password: None,
}
```

**After** (same!):
```rust
WebFrameData {
    message: Some(web_frame_data::Message::OpenTunnel(OpenTunnel {
        protocol,
        node_id,
        msg_id: Some(123),
        username: None,  // Still use Option
        password: None,
    })),
}
```

### Pattern 3: Nested Messages
**Before**:
```rust
NodeFrameData::Heartbeat { stats }
```

**After**:
```rust
use phirepass::node::{Heartbeat, Stats};

NodeFrameData {
    message: Some(node_frame_data::Message::Heartbeat(Heartbeat {
        stats: Some(Stats {
            host_cpu: stats.host_cpu,
            host_mem_used_bytes: stats.host_mem_used_bytes,
            // ... etc
        }),
    })),
}
```

---

## Expected Timeline

| Task | Time | Files |
|------|------|-------|
| Generate protobuf code | 2 min | Auto-generated |
| Update Frame implementation | 30 min | `common/src/protocol/common.rs` |
| Update web messages | 1 hour | `common/src/protocol/web.rs` |
| Update node messages | 1 hour | `common/src/protocol/node.rs` |
| Update daemon handlers | 2 hours | `daemon/src/ws.rs`, `daemon/src/ssh.rs`, `daemon/src/sftp.rs` |
| Update server handlers | 2 hours | `server/src/web.rs`, `server/src/node.rs` |
| Update channel (WASM) | 1 hour | `channel/src/lib.rs` |
| Testing | 1 hour | Tests in all modules |
| **TOTAL** | **~8-10 hours** | |

---

## Troubleshooting

### Error: "cannot find module `phirepass`"
**Solution**: Run `cargo build` in `common/` to generate code first

### Error: "missing field in protobuf message"
**Solution**: Check that optional fields use `optional` in .proto and `Option<T>` in Rust

### Error: "encoding variant not found"
**Solution**: Use `FrameEncoding::Protobuf` (capital P), it's an enum

### Large binary size with WASM
**Solution**: Use `wasm-opt -Oz` and ensure `opt-level = "s"` in Cargo.toml

---

## Verification Checklist

After implementation:

- [ ] `cargo build` succeeds in all packages
- [ ] Generated files exist in `common/src/protocol/generated/`
- [ ] All tests pass (`cargo test`)
- [ ] Protobuf frames are 40-60% smaller than JSON (benchmark)
- [ ] Server accepts both JSON and Protobuf (backward compat)
- [ ] WASM compiles successfully
- [ ] End-to-end test: browser → server → daemon works

---

## Benefits You'll Get

✅ **40-60% smaller messages** (vs 30-50% with MessagePack)
✅ **Strong typing** - Compile-time error detection
✅ **Better IDE support** - Auto-completion for all fields
✅ **Forward/backward compatibility** - Can add fields without breaking old clients
✅ **Industry standard** - Same format as gRPC, widely adopted
✅ **Schema documentation** - .proto files are self-documenting

---

## Quick Commands

```bash
# Generate protobuf code
cd common && cargo build

# Run tests
cargo test --package phirepass-common

# Check generated files
ls common/src/protocol/generated/

# Build everything
cargo build --workspace

# Build for release
cargo build --workspace --release
```

---

Ready to implement? Start with Step 1 (build & generate code) and work through the checklist! 

**Estimated total time: 1-2 days of focused work**

For detailed code examples, see [PROTOBUF_IMPLEMENTATION.md](PROTOBUF_IMPLEMENTATION.md)
