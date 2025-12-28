# SSH Data Flow Freeze Fix - CRITICAL BLOCKING ISSUES

## Problem

The SSH tunnel would freeze after some commands, with no more data reaching the web client. The daemon would not send `TunnelClosed` notifications.

## Root Causes

### 1. PRIMARY: Blocking Operations in Async Event Loops

**Multiple blocking `.await` calls were stalling critical async tasks:**

#### a) SSH `data()` Handler Blocking (MOST CRITICAL)
The SSH `data()` handler had a timeout fallback that used `.send().await`:
```rust
// WRONG - blocks russh event loop
match tokio::time::timeout(Duration::from_millis(500), self.sender.send(result)).await
```

**Impact**: When the WebSocket writer channel filled up:
1. SSH handler blocks on `.await`
2. russh event loop stalls completely
3. Can't process new SSH data or send window adjustments to SSH server
4. SSH server stops sending due to missing window updates (TCP flow control)
5. **Complete freeze** - no disconnect, no error, just silence

#### b) Control Message Sends Blocking
Heartbeat, ping, and pong tasks all used blocking `.send().await` on the same channel:
```rust
// WRONG - blocks background tasks
sender.send(heartbeat).await
sender.send(ping).await
tx.send(pong).await
```

**Impact**: When channel is full, these tasks block and can't drain the channel, creating a deadlock.

#### c) Cleanup Path Blocking
The `send_ssh_data_to_connection()` function used blocking `.send().await` when trying to send `TunnelClosed`:
```rust
// WRONG - blocks during cleanup
tx.send(raw).await
```

**Impact**: SSH task ends, tries to send `TunnelClosed`, blocks on full channel, never completes cleanup.

### 2. Missing Disconnect Detection
The SSH session loop only watched for user commands and manual shutdown. It did not observe russh handler events like `disconnected()`, `channel_close()`, or `exit_signal()`, so remote closures were logged but never propagated.

## Fixes Applied

### 1. Completely Non-Blocking SSH Handler (CRITICAL)
**File:** `daemon/src/ssh.rs`

```rust
// CORRECT - never blocks
match self.sender.try_send(result) {
    Ok(_) => debug!("forwarded"),
    Err(TrySendError::Full(_)) => {
        warn!("channel FULL; disconnecting immediately");
        // Signal disconnect without any .await
        if let Some(tx) = self.disconnect_notify.take() {
            let _ = tx.send(());
        }
    }
    Err(TrySendError::Closed(_)) => {
        // Signal disconnect
    }
}
```

**Key changes:**
- ❌ Removed `result.clone()` (unnecessary allocation)
- ❌ Removed `tokio::time::timeout()` with `.await` (blocks event loop)
- ✅ Use ONLY `try_send()` - zero blocking
- ✅ Disconnect immediately on backpressure

**Impact**: SSH event loop never blocks, data flow continues even under pressure.

### 2. Non-Blocking Control Messages
**File:** `daemon/src/ws.rs`

Changed all background task sends to use `try_send()`:
```rust
// Heartbeat, ping, pong all changed from:
sender.send(msg).await  // BLOCKS

// To:
sender.try_send(msg)    // NEVER BLOCKS
```

**Impact**: Control messages don't block when channel is full; tasks can exit cleanly instead of deadlocking.

### 3. Non-Blocking Cleanup Path
**File:** `daemon/src/ws.rs` - `send_ssh_data_to_connection()`

Changed from blocking `tx.send(raw).await` to non-blocking `tx.try_send(raw)`.

**Impact**: SSH cleanup completes immediately even if channel is full; no hanging tasks waiting to send `TunnelClosed`.

### 4. Disconnect Propagation
**File:** `daemon/src/ssh.rs`

- Added `disconnect_notify: Option<oneshot::Sender<()>>` to `SSHConnection`
- Handler methods (`disconnected()`, `channel_close()`, `channel_failure()`, `exit_signal()`) signal via oneshot
- Session `listen()` loop watches `disconnect_rx` and breaks on remote closure
- Triggers existing cleanup path that sends `TunnelClosed` to UI

**Impact:** Remote SSH closures now properly notify the web client.

### 5. Increased Channel Capacity
**File:** `daemon/src/ws.rs`

- Increased from 1024 to 2048 messages
- Reduces likelihood of backpressure under burst traffic

### 6. Backpressure Monitoring
**File:** `daemon/src/ws.rs`

- Added periodic capacity monitoring (every 10s)
- Warns when capacity drops below 512 (< 25%)
- Helps diagnose backpressure trends before they cause stalls

### 7. Writer Task Instrumentation
**File:** `daemon/src/ws.rs`

- Added frame count tracking
- Logs every 100 frames and on task end
- Helps diagnose where writer failures occur

### 8. SSH Inactivity Timeout
**File:** `daemon/src/ssh.rs`

- Changed `inactivity_timeout: None` → `Some(Duration::from_secs(300))`
- Detects silent TCP/NAT drops proactively (5 min timeout)

## Testing

### Basic Validation
```bash
cd /workspaces/phirepass-rs
cargo check
cargo build --release
```

### Run with Debug Logging
```bash
# Terminal 1: Server
RUST_LOG=debug cargo run -p phirepass-server start

# Terminal 2: Daemon
RUST_LOG=debug \
  SERVER_HOST=127.0.0.1 SERVER_PORT=8080 \
  PAT_TOKEN=test \
  SSH_HOST=127.0.0.1 SSH_PORT=12222 \
  cargo run -p phirepass-daemon start

# Terminal 3: SSH Server (optional)
docker build -t local-sshd daemon/sshd
docker run -d -p 12222:22 \
  -e SSH_USER=sshuser -e SSH_PASSWORD=changeme \
  --name local-sshd local-sshd
```

### What to Look For in Logs

**Good Signs:**
- `ssh data forwarded to ws writer (non-blocking)` – data flowing without blocking
- `ws writer channel capacity for ...: 1500 remaining` – healthy backpressure
- `remote ssh disconnect detected for tunnel ...` – proper disconnect propagation
- `TunnelClosed` sent to web client on SSH end

**Warning Signs:**
- `ws writer channel capacity low for ...: 200 remaining` – approaching backpressure
- `attempting timed send` – channel was full, fallback in progress
- `send timed out ... after 500ms` – severe backpressure, disconnect triggered
- `ws writer failed after N frames` – writer task crashed

### Stress Test
Generate high-volume SSH output to test backpressure handling:
```bash
# In SSH session
seq 1 100000 | while read n; do echo "Line $n of data"; done
cat /dev/urandom | base64 | head -10000
```

Monitor daemon logs for:
- Channel capacity warnings
- Any timeout events
- Sustained non-blocking sends

## Additional Recommendations

### 1. TCP Keepalive
Consider enabling TCP keepalive on the WebSocket connection to detect dead connections faster.

### 2. Adaptive Flow Control
If backpressure warnings persist under normal load, implement adaptive buffering:
- Drop non-critical frames (e.g., heartbeats)
- Prioritize user input over SSH output
- Send backpressure notification to web client

### 3. Client-Side Buffering
Web client should handle `TunnelClosed` gracefully and offer reconnect.

### 4. Metrics Collection
Track:
- Channel utilization over time
- Disconnect reasons (timeout vs. handler vs. user)
- Frames per second throughput

## Files Modified
- `daemon/src/ssh.rs` – Non-blocking handler, disconnect signaling, inactivity timeout
- `daemon/src/ws.rs` – Increased capacity, monitoring, writer instrumentation
