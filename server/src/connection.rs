use phirepass_common::protocol::{Frame, NodeControlMessage};
use phirepass_common::stats::Stats;
use std::net::SocketAddr;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone)]
pub(crate) struct WebConnection {
    pub(crate) connected_at: Instant,
    pub(crate) last_heartbeat: Instant,
    pub(crate) addr: SocketAddr,
    pub(crate) tx: UnboundedSender<Frame>,
}

impl WebConnection {
    pub(crate) fn new(addr: SocketAddr, tx: UnboundedSender<Frame>) -> Self {
        Self {
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
            addr,
            tx,
        }
    }
}

#[derive(Clone)]
pub(crate) struct NodeConnection {
    pub(crate) connected_at: Instant,
    pub(crate) last_heartbeat: Instant,
    pub(crate) addr: SocketAddr,
    pub(crate) last_stats: Option<Stats>,
    pub(crate) tx: UnboundedSender<NodeControlMessage>,
}

impl NodeConnection {
    pub(crate) fn new(addr: SocketAddr, tx: UnboundedSender<NodeControlMessage>) -> Self {
        Self {
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
            addr,
            last_stats: None,
            tx,
        }
    }
}
