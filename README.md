# 🚀 Reverse Tunneling Multiplexed WebSocket Gateway
### A Rust-powered multi-protocol reverse tunnel (SSH, RDP, TCP, HTTP) over a single WebSocket connection

This project creates a **reverse tunneling system** allowing clients behind restrictive NAT/firewalls (e.g., ISP blocking inbound ports) to expose services securely by connecting *outbound* to a public VPS via **one WebSocket connection**.

### In other words:
- Client/daemon initiates outbound WebSocket → VPS
- VPS multiplexes SSH / RDP / TCP / HTTP streams over that WebSocket
- VPS forwards traffic to internal services or exposes them to the Internet
- No inbound ports needed on the client’s side
- Works behind CGNAT, blocked ports, hotels, 4G/5G carriers, etc.

# 🎯 Purpose

Many ISPs block all inbound ports, preventing self-hosting or remote access.  
This system solves it by allowing:

- Reverse SSH tunnels  
- Remote Desktop (RDP) forwarding  
- Raw TCP tunnels  
- HTTP proxying  
- Custom multiplexed protocol streams  

Everything runs over:

✔ A single WebSocket  
✔ On port 443 (WSS), indistinguishable from normal HTTPS traffic  
✔ With multiple streams multiplexed inside it  

# 🧠 High-Level Architecture

```
[ Local Daemon ] --WebSocket--> [ Public VPS WebSocket Server ]
        |
      SSH / RDP / TCP / HTTP multiplexed streams
```

# 🧩 Protocol Design

Every WebSocket message is a **binary frame**:

```
[protocol_id: u8][payload_length: u32 BE][payload bytes]
```

Protocol IDs:
- 0 → Control JSON frames  
- 1 → SSH stream  
- 2 → RDP stream  
- 3 → Raw TCP stream  
- 4 → Reserved  

# 🗂 Control Frames (Protocol 0)

JSON messages used for:
- Authentication  
- Heartbeat  
- Tunnel management  
- Error reporting  

Example:
```json
{ "type": "Auth", "token": "YOUR_TOKEN" }
```

# 🔌 Binary Streaming Protocols

SSH, RDP, HTTP, and generic TCP are streamed as **raw binary**.

A custom `WsChannel` abstracts WebSocket frames into an `AsyncRead + AsyncWrite` stream so each protocol works naturally with Tokio IO.

- SSH integrates via `russh::server::run_stream()`
- RDP and TCP use `tokio::io::copy_bidirectional`

# 🔁 Multiplexing

- All protocols share one WebSocket connection.
- Each protocol gets a logical `WsChannel`.
- The server demultiplexes frames using protocol_id.

# 🛡️ Authentication

Client must send:
```json
{ "type": "Auth", "token": "my_secure_token" }
```

# ❤️ Heartbeats

Server sends:
```json
{ "type": "Heartbeat" }
```

Client must respond to avoid disconnection.

# 📚 Tech Stack

- Rust + Tokio  
- WebSockets (axum)  
- SSH (russh)  
- JSON control frames  
- Binary multiplexed protocol streams  

# 🛠 Current Status

- ✔ Multiplexer working  
- ✔ Control protocol  
- ✔ TCP / RDP forwarding  
- ✔ Heartbeat  
- ✔ Authentication  
- ⏳ SSH full integration in progress  

# 🧩 How to Extend

AI agents can:
- Add new protocols by defining new protocol_id handlers  
- Extend control protocol  
- Support multi-tunnel sessions  
- Harden authentication  
- Build full client daemon  

# 🤖 Why This README Helps AI

It is structured, explicit, and fully describes:
- Architecture  
- Binary and JSON frame formats  
- All protocol behaviors  
- Extension points  
- Expected runtime behavior  

Perfect for future reasoning and code generation.
