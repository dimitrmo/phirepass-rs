use bytes::Bytes;
use log::{debug, warn};
use phirepass_common::protocol::Protocol;
use phirepass_common::protocol::common::{Frame, FrameError};
use phirepass_common::protocol::node::{NodeFrameData, WebFrameId};
use phirepass_common::protocol::web::WebFrameData;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::error::TrySendError;
use ulid::Ulid;

#[inline]
pub fn send_frame_data(sender: &Sender<Frame>, data: NodeFrameData) {
    if sender.is_closed() {
        warn!("frame sender is closed, client may have been disconnected");
        return;
    }

    match sender.try_send(data.into()) {
        Ok(_) => debug!("frame response sent"),
        Err(err) => match err {
            TrySendError::Closed(err) => {
                warn!("failed to send frame to closed channel: {err:?}");
            }
            TrySendError::Full(err) => {
                debug!(
                    "frame channel full for client, potential slow client or backpressure: {err:?}"
                );
            }
        },
    }
}

#[inline]
pub async fn send_tunnel_data(tx: &Sender<Frame>, sid: u32, node_id: String, data: Bytes) {
    send_frame_data(
        tx,
        NodeFrameData::WebFrame {
            id: WebFrameId::SessionId(sid),
            frame: WebFrameData::TunnelData {
                protocol: Protocol::SSH as u8,
                node_id: node_id.to_string(),
                sid,
                data,
            },
        },
    );
}

#[inline]
pub fn send_requires_username_error(sender: &Sender<Frame>, cid: Ulid, msg_id: Option<u32>) {
    send_frame_data(
        sender,
        NodeFrameData::WebFrame {
            id: WebFrameId::ConnectionId(cid),
            frame: WebFrameData::Error {
                kind: FrameError::RequiresUsername,
                message: String::from("Username is missing"),
                msg_id,
            },
        },
    );
}

#[inline]
pub fn send_requires_password_error(sender: &Sender<Frame>, cid: Ulid, msg_id: Option<u32>) {
    send_frame_data(
        sender,
        NodeFrameData::WebFrame {
            id: WebFrameId::ConnectionId(cid),
            frame: WebFrameData::Error {
                kind: FrameError::RequiresPassword,
                message: String::from("Password is missing"),
                msg_id,
            },
        },
    );
}
