use phirepass_common::protocol::{Frame, NodeControlMessage};
use phirepass_common::stats::Stats;
use std::net::IpAddr;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone)]
pub(crate) struct WebConnection {
    pub(crate) connected_at: Instant,
    pub(crate) last_heartbeat: Instant,
    pub(crate) ip: IpAddr,
    pub(crate) tx: UnboundedSender<Frame>,
}

impl WebConnection {
    pub(crate) fn new(ip: IpAddr, tx: UnboundedSender<Frame>) -> Self {
        Self {
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
            ip,
            tx,
        }
    }
}

#[derive(Clone)]
pub(crate) struct NodeConnection {
    pub(crate) connected_at: Instant,
    pub(crate) last_heartbeat: Instant,
    pub(crate) ip: IpAddr,
    pub(crate) last_stats: Option<Stats>,
    pub(crate) tx: UnboundedSender<NodeControlMessage>,
}

impl NodeConnection {
    pub(crate) fn new(ip: IpAddr, tx: UnboundedSender<NodeControlMessage>) -> Self {
        Self {
            connected_at: Instant::now(),
            last_heartbeat: Instant::now(),
            ip,
            last_stats: None,
            tx,
        }
    }
}
