# Phirepass

Rust workspace for the Phirepass remote-access relay. The system has three main pieces:
- `server`: accepts web clients and node daemons over WebSocket, forwards traffic, and exposes basic HTTP status endpoints.
- `daemon`: runs on a target host, dials the server via WebSocket, and opens SSH sessions on behalf of web clients.
- `channel`: WASM client helpers for browsers (frames, heartbeat, auth prompts).
- `common`: shared protocol, env, and stats code.

## How traffic flows
1) A browser (via the `channel` package or your own client) connects to `ws://<server-host>:8080/api/web/ws`.
2) A node daemon connects to `ws://<server-host>:8080/api/nodes/ws`.
3) Web clients send `WebControlMessage::OpenTunnel` with protocol/target/credentials; the server forwards it to the matching node as `NodeControlMessage::OpenTunnel`.
4) The daemon opens a local SSH session (`russh`), pipes stdin/stdout over WebSocket frames, and mirrors resize events. When the SSH channel closes it sends `TunnelClosed`.
5) Heartbeats and ping/pong frames keep both directions alive; stats are logged server-side.

## Protocol snapshot
- Frames: 1 byte protocol + 4 byte BE payload length + payload. `Protocol::Control = 0`, `Protocol::SSH = 1`.
- Control messages web→server: `Heartbeat`, `OpenTunnel`, `TunnelData` (payload for SSH), `Resize`, `TunnelClosed`, `Error`, `Ok`.
- Control messages server→daemon: `Auth` (stubbed), `Heartbeat { stats }`, `OpenTunnel`, `TunnelData`, `Resize`, `Ping/Pong`, `ConnectionDisconnect`, `Frame { frame, cid }`, `Error`, `Ok`.
- Errors back to web use `WebControlMessage::Error` with kinds `Generic`, `RequiresPassword`.

## HTTP endpoints (server)
- `GET /api/web/ws` and `GET /api/nodes/ws`: WebSocket upgrades for web clients and nodes.
- `GET /api/nodes`: connected nodes with last heartbeat and stats.
- `GET /api/connections`: active web connections.
- `GET /stats`: server process stats plus counts of nodes/connections.
- `GET /version`: workspace version string.

## Configuration
Server env (defaults): `APP_MODE=development|production`, `IP_SOURCE=ConnectInfo|XForwardedFor|Forwarded`, `HOST=0.0.0.0`, `PORT=8080`, `STATS_REFRESH_INTERVAL=15`, `ACCESS_CONTROL_ALLOW_ORIGIN` (set in production).

Daemon env (defaults): `APP_MODE=development|production`, `HOST=0.0.0.0`, `PORT=8081`, `PAT_TOKEN` (sent in Auth, not yet enforced server-side), `STATS_REFRESH_INTERVAL=15`, `PING_INTERVAL=30`, `SERVER_HOST=0.0.0.0`, `SERVER_PORT=8080`, `SSH_HOST=0.0.0.0`, `SSH_PORT=22`, `SSH_AUTH_METHOD=credentials_prompt`.

## Local development
- Build everything: `cargo build --all` (or `make build`); release: `make prod`.
- Run server: `make server` (binds `0.0.0.0:8080`, uses `RUST_LOG=info`).
- Run a node on the same box: `SSH_USER=$USER make daemon` (connects back to the server, opens SSH to `SSH_HOST:SSH_PORT`).
- WASM client (optional): requires `wasm-pack`; `make wasm-dev` or `make wasm-prod` builds `channel/pkg`, `make web` serves the static demo with `http-server` on `:8083`.
- Formatting: `make format`.

## Current gaps and notes
- Node authentication is a stub: `NodeControlMessage::Auth` is received but not validated yet.
- No persistent storage; node/connection lists are in-memory.
- Open tasks live in `TASKS.md` (UI, OAuth device flow, PAT revocation, packaging).

## Directory map
- `server/`: axum HTTP and WebSocket bridge, connection/node registries, metrics.
- `daemon/`: reconnecting WebSocket client, SSH tunnel management, optional HTTP `/version`.
- `common/`: protocol types, stats gathering, logging helpers, env mode.
- `channel/`: browser channel helper and demo assets (served via `make web`).
- `Makefile`: convenience targets for running, building, Docker images, and wasm artifacts.
